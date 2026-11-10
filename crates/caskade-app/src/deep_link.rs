//! `caskade://install` deep links (#57): `?url=` from websites, `?path=`
//! from the CLI. Any web page can open these links, so a deep link only
//! becomes a pending install request; nothing is resolved, downloaded or
//! run until the user confirms it in the app.

use url::Url;

pub const SCHEME: &str = "caskade";

/// The path or URL a `caskade://install?url=…` / `?path=…` link names.
pub fn install_input(link: &Url) -> Result<String, String> {
    if link.scheme() != SCHEME {
        return Err(caskade_core::tr!("not a {scheme}: link", scheme = SCHEME));
    }
    // `caskade://install?…`, or `caskade:install?…` from strict handlers.
    let action = format!("{}{}", link.host_str().unwrap_or_default(), link.path());
    if action.trim_matches('/') != "install" {
        return Err(caskade_core::tr!("unknown deep link action"));
    }
    let mut input = None;
    for (key, value) in link.query_pairs() {
        if !matches!(key.as_ref(), "url" | "path") {
            continue;
        }
        if input.is_some() {
            return Err(caskade_core::tr!("a deep link installs one path or URL"));
        }
        input = Some((key == "url", value.into_owned()));
    }
    let (url, value) =
        input.ok_or_else(|| caskade_core::tr!("a deep link needs ?url= or ?path="))?;
    let web = Url::parse(&value)
        .is_ok_and(|parsed| matches!(parsed.scheme(), "http" | "https" | "magnet"));
    match (url, web) {
        (true, false) => Err(caskade_core::tr!(
            "?url= takes an HTTP(S) URL or a magnet link"
        )),
        (false, true) => Err(caskade_core::tr!("?path= takes a local path")),
        _ => Ok(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<String, String> {
        install_input(&Url::parse(text).unwrap())
    }

    #[test]
    fn a_link_names_one_path_or_url() {
        assert_eq!(
            parse("caskade://install?url=https%3A%2F%2Fexample.com%2Fgame.zip%3Ft%3D1").unwrap(),
            "https://example.com/game.zip?t=1"
        );
        assert_eq!(
            parse("caskade://install/?path=%2Fhome%2Fme%2FGame%20Setup.exe").unwrap(),
            "/home/me/Game Setup.exe"
        );
        assert_eq!(
            parse("caskade:install?path=/tmp/game").unwrap(),
            "/tmp/game"
        );
        // A magnet link (#159), percent-encoded inside the deep link.
        assert_eq!(
            parse("caskade://install?url=magnet%3A%3Fxt%3Durn%3Abtih%3A0123456789abcdef0123456789abcdef01234567").unwrap(),
            "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567"
        );
        for bad in [
            "caskade://install",
            "caskade://uninstall?path=/tmp/game",
            "caskade://install/extra?path=/tmp/game",
            "caskade://install?url=/tmp/game",
            "caskade://install?url=file:///etc/passwd",
            "caskade://install?path=https://example.com/game.zip",
            "caskade://install?url=https://example.com/a&path=/tmp/game",
            "caskade://install?url=https://example.com/a&url=https://example.com/b",
            "https://install?url=https://example.com/game.zip",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }
}
