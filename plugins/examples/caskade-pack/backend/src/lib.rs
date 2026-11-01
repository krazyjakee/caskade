//! An example Caskade plugin (#55): it teaches the installer the `.cpk`
//! game pack format and finds the download on a game's page.
//!
//! A `.cpk` file is `CPAK`, then entries to the end of the file. An entry
//! is a header (name length as u16 LE, the UTF-8 relative path, flags as a
//! u8 where 1 means executable, size as u64 LE) followed by its bytes.
//!
//! The plugin has no `fs` permission. It reads the pack through
//! `host.install-input-read` and answers with plan steps; the installer
//! writes the files. Pages are fetched through `wasi:http`, so only hosts
//! in its `network` permission are reachable.
//!
//! Its messages show in the language Caskade is in (#219): the backend
//! asks the host with `host.locale` and picks its translation from
//! [`SV`]. The manifest's and the UI half's texts come from
//! `locales/sv.po`, which the host reads; a backend can't read files, so it
//! carries its own.

wit_bindgen::generate!({
    path: "../../../../wit",
    inline: "
        package example:caskade-pack;

        world pack {
            include caskade:plugin/plugin@0.1.1;
            export caskade:plugin/install-handler@0.1.1;
            export caskade:plugin/url-resolver@0.1.1;
        }
    ",
    world: "example:caskade-pack/pack",
    generate_all,
});

use caskade::plugin::host;
use caskade::plugin::types::{InterceptResult, PluginError};

const MAGIC: &[u8] = b"CPAK";
const MAX_ENTRIES: usize = 4096;
const MAX_PAGE: usize = 1 << 20;

struct Pack;

fn error(code: &str, message: impl Into<String>) -> PluginError {
    PluginError {
        code: code.into(),
        message: message.into(),
    }
}

/// The backend's messages in Swedish, by their English text. The same
/// entries are in `locales/sv.po`, so translators see them in one place.
const SV: &[(&str, &str)] = &[
    (
        "the pack ends in the middle of an entry",
        "paketet tar slut mitt i en post",
    ),
    (
        "a pack holds at most 4096 entries",
        "ett paket rymmer högst 4096 poster",
    ),
    ("an entry name isn't UTF-8", "ett postnamn är inte UTF-8"),
    (
        "{path} runs past the end of the pack",
        "{path} går förbi slutet av paketet",
    ),
    ("the pack is empty", "paketet är tomt"),
    (
        "a pack is a file of known size",
        "ett paket är en fil med känd storlek",
    ),
];

/// `english` in the language Caskade shows text in, with `{path}` filled
/// in after the lookup, so a translation can move it.
fn tr(english: &str, path: &str) -> String {
    let locale = host::locale();
    let translated = match locale.split('-').next() {
        Some("sv") => SV.iter().find(|(id, _)| *id == english).map(|(_, sv)| *sv),
        _ => None,
    };
    translated.unwrap_or(english).replace("{path}", path)
}

/// Whether the user prefers packs (`preferPacks` in its settings, on by
/// default): its plan then ranks above the built-in handlers'.
fn prefers_packs() -> bool {
    host::settings_get().is_none_or(|settings| !settings.contains("\"preferPacks\":false"))
}

/// Exactly `length` bytes of the input from `offset`.
fn read(offset: u64, length: u32) -> Result<Vec<u8>, PluginError> {
    let bytes = host::install_input_read(offset, length)?;
    if bytes.len() != length as usize {
        return Err(error(
            "truncated",
            tr("the pack ends in the middle of an entry", ""),
        ));
    }
    Ok(bytes)
}

struct Entry {
    path: String,
    executable: bool,
    offset: u64,
    size: u64,
}

/// Walks the entry headers; the data is never read.
fn entries(total: u64) -> Result<Vec<Entry>, PluginError> {
    let mut offset = MAGIC.len() as u64;
    let mut found = Vec::new();
    while offset < total {
        if found.len() == MAX_ENTRIES {
            return Err(error(
                "too_large",
                tr("a pack holds at most 4096 entries", ""),
            ));
        }
        let length = u16::from_le_bytes([read(offset, 2)?[0], read(offset + 1, 1)?[0]]);
        let path = String::from_utf8(read(offset + 2, u32::from(length))?)
            .map_err(|_| error("bad_pack", tr("an entry name isn't UTF-8", "")))?;
        let meta = read(offset + 2 + u64::from(length), 9)?;
        let size = u64::from_le_bytes(meta[1..].try_into().expect("8 bytes"));
        let start = offset + 2 + u64::from(length) + 9;
        offset = start
            .checked_add(size)
            .filter(|end| *end <= total)
            .ok_or_else(|| {
                error(
                    "truncated",
                    tr("{path} runs past the end of the pack", &path),
                )
            })?;
        found.push(Entry {
            path,
            executable: meta[0] & 1 == 1,
            offset: start,
            size,
        });
    }
    if found.is_empty() {
        return Err(error("bad_pack", tr("the pack is empty", "")));
    }
    Ok(found)
}

/// A top-level string or number field of the host's JSON input.
fn field<'a>(json: &'a str, name: &str) -> Option<&'a str> {
    let rest = json.split(&format!("\"{name}\":")).nth(1)?;
    match rest.strip_prefix('"') {
        Some(text) => text.split('"').next(),
        None => rest.split([',', '}']).next(),
    }
}

fn quoted(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

impl exports::caskade::plugin::install_handler::Guest for Pack {
    fn claims(_input: String) -> bool {
        host::install_input_read(0, MAGIC.len() as u32).is_ok_and(|magic| magic == MAGIC)
    }

    /// One `copy_range` per entry, and `set_executable` for programs.
    fn plan(input: String) -> Result<String, PluginError> {
        let artifact =
            field(&input, "artifactId").ok_or_else(|| error("bad_input", "no artifactId"))?;
        let total = field(&input, "size")
            .and_then(|size| size.parse().ok())
            .ok_or_else(|| error("bad_input", tr("a pack is a file of known size", "")))?;
        let mut steps = Vec::new();
        for entry in entries(total)? {
            let path = quoted(&entry.path);
            steps.push(format!(
                r#"{{"type":"copy_range","value":{{"from":{},"offset":{},"length":{},"path":{path}}}}}"#,
                quoted(artifact),
                entry.offset,
                entry.size
            ));
            if entry.executable {
                steps.push(format!(
                    r#"{{"type":"set_executable","value":{{"path":{path}}}}}"#
                ));
            }
        }
        let confidence = if prefers_packs() { 0.9 } else { 0.5 };
        Ok(format!(
            r#"{{"steps":[{}],"confidence":{confidence}}}"#,
            steps.join(",")
        ))
    }
}

impl exports::caskade::plugin::url_resolver::Guest for Pack {
    /// The page links its pack with `rel="caskade-download"`.
    fn resolve(url: String) -> Result<String, PluginError> {
        let page = get(&url).map_err(|message| error("http", message))?;
        let Some(href) = page
            .split('<')
            .filter(|tag| tag.contains("caskade-download"))
            .find_map(|tag| tag.split("href=\"").nth(1)?.split('"').next())
        else {
            return Ok("[]".into());
        };
        let download = absolute(&url, href);
        let name = download.rsplit('/').next().unwrap_or("game.cpk");
        Ok(format!(
            r#"[{{"url":{},"fileName":{}}}]"#,
            quoted(&download),
            quoted(name)
        ))
    }
}

/// `href` resolved against the page it is on.
fn absolute(page: &str, href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.into();
    }
    let (scheme, rest) = page.split_once("://").unwrap_or(("https", page));
    let authority = rest.split('/').next().unwrap_or(rest);
    if href.starts_with('/') {
        format!("{scheme}://{authority}{href}")
    } else {
        let folder = page.rsplit_once('/').map_or(page, |(folder, _)| folder);
        format!("{folder}/{href}")
    }
}

/// The body of a GET request.
fn get(url: &str) -> Result<String, String> {
    use wasi::http::{outgoing_handler, types};
    let (scheme, rest) = match url.split_once("://") {
        Some(("https", rest)) => (types::Scheme::Https, rest),
        Some(("http", rest)) => (types::Scheme::Http, rest),
        _ => return Err("only http and https pages".into()),
    };
    let (authority, path) = rest
        .split_once('/')
        .map_or((rest, "/".to_owned()), |(authority, path)| {
            (authority, format!("/{path}"))
        });
    let request = types::OutgoingRequest::new(types::Fields::new());
    request.set_scheme(Some(&scheme)).map_err(|()| "scheme")?;
    request
        .set_authority(Some(authority))
        .map_err(|()| "authority")?;
    request
        .set_path_with_query(Some(&path))
        .map_err(|()| "path")?;
    let pending = outgoing_handler::handle(request, None).map_err(|error| format!("{error:?}"))?;
    pending.subscribe().block();
    let response = pending
        .get()
        .ok_or("no response")?
        .map_err(|()| "response taken twice")?
        .map_err(|error| format!("{error:?}"))?;
    if response.status() != 200 {
        return Err(format!("the page answered {}", response.status()));
    }
    let body = response.consume().map_err(|()| "no body")?;
    let stream = body.stream().map_err(|()| "no body stream")?;
    let mut page = Vec::new();
    while let Ok(chunk) = stream.blocking_read(64 * 1024) {
        page.extend(chunk);
        if page.len() > MAX_PAGE {
            return Err("the page is larger than 1 MiB".into());
        }
    }
    drop(stream);
    Ok(String::from_utf8_lossy(&page).into_owned())
}

/// The `plugin` world asks for these; this plugin has no hooks or UI.
impl exports::caskade::plugin::hooks::Guest for Pack {
    fn on_event(_name: String, _payload: String) {}

    fn intercept(_point: String, payload: String) -> Result<InterceptResult, PluginError> {
        Ok(InterceptResult::Continue(payload))
    }
}

impl exports::caskade::plugin::requests::Guest for Pack {
    fn handle_request(method: String, _payload: String) -> Result<String, PluginError> {
        Err(error("unknown_method", method))
    }
}

export!(Pack);
