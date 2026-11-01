//! `caskade-theme://localhost/<id>/theme.css` (`http://caskade-theme.localhost/…`
//! on Windows): a theme's compiled stylesheet, and its assets and previews
//! under the same prefix (#139). Only valid themes are served, read-only.
use crate::plugin_scheme::{base, decode, empty, encode, mime, served};
use caskade_core::themes::ThemeService;
use std::path::Path;
use tauri::{
    Manager,
    http::{Method, Response, StatusCode},
};

pub const SCHEME: &str = "caskade-theme";

pub fn register<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder.register_asynchronous_uri_scheme_protocol(SCHEME, |context, request, responder| {
        let app = context.app_handle().clone();
        let method = request.method().clone();
        let path = request.uri().path().to_owned();
        tauri::async_runtime::spawn_blocking(move || {
            let response = match app.try_state::<ThemeService>() {
                Some(themes) => respond(&themes, &method, &path),
                None => empty(StatusCode::SERVICE_UNAVAILABLE),
            };
            responder.respond(response);
        });
    })
}

/// The URL of a theme's compiled stylesheet, or of a file it ships.
pub fn url(id: &str, path: &str) -> String {
    let path: Vec<String> = path.split('/').map(encode).collect();
    format!("{}/{}/{}", base(SCHEME), encode(id), path.join("/"))
}

pub(crate) fn respond(themes: &ThemeService, method: &Method, path: &str) -> Response<Vec<u8>> {
    if method != Method::GET {
        return empty(StatusCode::METHOD_NOT_ALLOWED);
    }
    let Some((id, relative)) = path.trim_start_matches('/').split_once('/') else {
        return empty(StatusCode::NOT_FOUND);
    };
    let (Some(id), Some(relative)) = (decode(id), decode(relative)) else {
        return empty(StatusCode::BAD_REQUEST);
    };
    let (bytes, kind) = if relative == "theme.css" {
        match themes.stylesheet(&id) {
            Ok(css) => (css.as_bytes().to_vec(), "text/css"),
            Err(_) => return empty(StatusCode::NOT_FOUND),
        }
    } else {
        match themes.asset(&id, &relative) {
            Some(bytes) => (bytes, mime(Path::new(&relative))),
            None => return empty(StatusCode::NOT_FOUND),
        }
    };
    served(bytes, kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::http::header;

    fn service() -> (tempfile::TempDir, ThemeService) {
        let temp = tempfile::tempdir().unwrap();
        let folder = temp.path().join("neon");
        std::fs::create_dir_all(folder.join("assets")).unwrap();
        std::fs::write(
            folder.join("theme.json"),
            r#"{"id": "dev.example.neon", "name": "Neon", "version": "0.1.0", "extends": "caskade.default"}"#,
        )
        .unwrap();
        std::fs::write(
            folder.join("theme.css"),
            "[data-cs=\"hero\"] { background: url(assets/bg.png); }",
        )
        .unwrap();
        std::fs::write(folder.join("assets/bg.png"), "png").unwrap();
        let themes = ThemeService::new(temp.path());
        (temp, themes)
    }

    #[test]
    fn serves_compiled_stylesheets_and_assets_but_nothing_else() {
        let (_temp, themes) = service();
        let get = |path: &str| respond(&themes, &Method::GET, path);
        let css = get("/dev.example.neon/theme.css");
        assert_eq!(css.status(), StatusCode::OK);
        assert_eq!(css.headers()[header::CONTENT_TYPE], "text/css");
        let text = String::from_utf8(css.into_body()).unwrap();
        assert!(text.contains("@layer cs.theme {"));
        assert!(text.contains(r#"url("/dev.example.neon/assets/bg.png")"#));
        let asset = get("/dev.example.neon/assets/bg.png");
        assert_eq!(
            (
                asset.status(),
                asset.headers()[header::CONTENT_TYPE].clone()
            ),
            (StatusCode::OK, "image/png".parse().unwrap())
        );
        assert_eq!(get("/caskade.default/theme.css").status(), StatusCode::OK);
        for path in [
            "/dev.example.neon/theme.json",
            "/dev.example.neon/tokens.json",
            "/dev.example.neon/assets/../theme.json",
            "/dev.example.neon/%2E%2E/theme.json",
            "/dev.example.missing/theme.css",
            "/dev.example.neon",
        ] {
            assert_eq!(get(path).status(), StatusCode::NOT_FOUND, "{path}");
        }
        assert_eq!(
            respond(&themes, &Method::POST, "/dev.example.neon/theme.css").status(),
            StatusCode::METHOD_NOT_ALLOWED
        );
    }

    #[test]
    fn urls_follow_the_platform_scheme() {
        let expected = if cfg!(windows) {
            "http://caskade-theme.localhost/dev.example.neon/assets/My%20Font.woff2"
        } else {
            "caskade-theme://localhost/dev.example.neon/assets/My%20Font.woff2"
        };
        assert_eq!(url("dev.example.neon", "assets/My Font.woff2"), expected);
    }
}
