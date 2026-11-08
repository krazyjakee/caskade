//! `caskade-plugin://localhost/<id>/<path>` (`http://caskade-plugin.localhost/…`
//! on Windows): enabled plugins' UI files (#127). Only files
//! inside the plugin's folder are served, read-only.
use caskade_plugin_host::PluginHost;
use std::path::{Component, Path, PathBuf};
use tauri::{
    Manager,
    http::{Method, Response, StatusCode, header},
};

pub const SCHEME: &str = "caskade-plugin";

pub fn register<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder.register_asynchronous_uri_scheme_protocol(SCHEME, |context, request, responder| {
        let app = context.app_handle().clone();
        tauri::async_runtime::spawn(async move {
            let response = match app.try_state::<PluginHost>() {
                Some(host) => respond(&host, request.method(), request.uri().path()).await,
                None => empty(StatusCode::SERVICE_UNAVAILABLE),
            };
            responder.respond(response);
        });
    })
}

/// The URL the webview loads a plugin file from.
pub fn url(id: &str, relative: &Path) -> String {
    let path: Vec<String> = relative
        .components()
        .filter_map(|part| match part {
            Component::Normal(part) => Some(encode(&part.to_string_lossy())),
            _ => None,
        })
        .collect();
    format!("{}/{}/{}", base(SCHEME), encode(id), path.join("/"))
}

async fn respond(host: &PluginHost, method: &Method, path: &str) -> Response<Vec<u8>> {
    if method != Method::GET {
        return empty(StatusCode::METHOD_NOT_ALLOWED);
    }
    let Some((id, relative)) = path.trim_start_matches('/').split_once('/') else {
        return empty(StatusCode::NOT_FOUND);
    };
    let Some(id) = decode(id) else {
        return empty(StatusCode::BAD_REQUEST);
    };
    let Some(folder) = host.folder(&id).filter(|_| host.enabled(&id)) else {
        return empty(StatusCode::NOT_FOUND);
    };
    match locate(&folder, relative) {
        Ok(file) => match tauri::async_runtime::spawn_blocking({
            let file = file.clone();
            move || std::fs::read(file)
        })
        .await
        {
            Ok(Ok(bytes)) => served(bytes, mime(&file)),
            _ => empty(StatusCode::NOT_FOUND),
        },
        Err(status) => empty(status),
    }
}

/// A file inside `folder` named by the percent-encoded `relative` path.
/// Rejects `..`, absolute and empty segments, and anything that resolves
/// (through symlinks) outside the folder.
fn locate(folder: &Path, relative: &str) -> Result<PathBuf, StatusCode> {
    let mut path = PathBuf::new();
    for segment in relative.split('/') {
        let segment = decode(segment).ok_or(StatusCode::BAD_REQUEST)?;
        if segment.is_empty()
            || segment == "."
            || segment == ".."
            || segment.contains(['/', '\\', '\0', ':'])
        {
            return Err(StatusCode::BAD_REQUEST);
        }
        path.push(segment);
    }
    let root = std::fs::canonicalize(folder).map_err(|_| StatusCode::NOT_FOUND)?;
    let file = std::fs::canonicalize(root.join(path)).map_err(|_| StatusCode::NOT_FOUND)?;
    if !file.starts_with(&root) {
        return Err(StatusCode::FORBIDDEN);
    }
    if !file.is_file() {
        return Err(StatusCode::NOT_FOUND);
    }
    Ok(file)
}

// The helpers below are shared with `theme_scheme`.

/// The `Content-Type` for a served file, by extension.
pub(crate) fn mime(file: &Path) -> &'static str {
    match file
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("js" | "mjs") => "text/javascript",
        Some("css") => "text/css",
        Some("json" | "map") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("avif") => "image/avif",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        _ => "application/octet-stream",
    }
}

/// Where `scheme`'s URLs start (Windows serves custom schemes over http).
pub(crate) fn base(scheme: &str) -> String {
    if cfg!(windows) {
        format!("http://{scheme}.localhost")
    } else {
        format!("{scheme}://localhost")
    }
}

/// A served file's response, read-only and never sniffed.
pub(crate) fn served(bytes: Vec<u8>, kind: &str) -> Response<Vec<u8>> {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, kind)
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(bytes)
        .unwrap_or_else(|_| empty(StatusCode::INTERNAL_SERVER_ERROR))
}

pub(crate) fn empty(status: StatusCode) -> Response<Vec<u8>> {
    let mut response = Response::new(Vec::new());
    *response.status_mut() = status;
    response
}

/// Percent-encodes everything but unreserved URL characters.
pub(crate) fn encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Percent-decodes `text`; `None` on a bad escape or non-UTF-8 result.
pub(crate) fn decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            let hex = std::str::from_utf8(bytes.get(at + 1..at + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
#[path = "plugin_scheme_tests.rs"]
mod tests;
