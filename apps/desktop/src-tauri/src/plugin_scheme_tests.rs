use super::*;
use caskade_plugin_host::{HostOptions, Limits, PluginServices};
use serde_json::json;

struct Fixture {
    temp: tempfile::TempDir,
    host: PluginHost,
}

/// One plugin with a UI module, a stylesheet and a backend-less manifest.
fn fixture() -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let plugin = temp.path().join("plugins/hltb");
    std::fs::create_dir_all(plugin.join("dist")).unwrap();
    std::fs::write(plugin.join("dist/ui.js"), b"export default {}").unwrap();
    std::fs::write(plugin.join("dist/ui.css"), b"p{}").unwrap();
    std::fs::write(temp.path().join("secret.txt"), b"outside").unwrap();
    std::fs::write(
        plugin.join("plugin.json"),
        json!({
            "id": "dev.example.hltb",
            "name": "HowLongToBeat",
            "version": "0.1.0",
            "api": "^0.1",
            "ui": "dist/ui.js",
            "contributes": {
                "slots": [{"slot": "game.details.tabs", "id": "hltb", "title": "How long to beat"}],
                "views": [{"id": "stats", "title": "Stats"}],
                "commands": [{"id": "refresh", "title": "Refresh times"}]
            }
        })
        .to_string(),
    )
    .unwrap();
    let store =
        tauri::async_runtime::block_on(caskade_core::db::LibraryStore::in_memory()).unwrap();
    let host = PluginHost::new(
        PluginServices {
            store,
            bus: None,
            settings: None,
        },
        HostOptions {
            folders: vec![temp.path().join("plugins")],
            data: temp.path().join("plugin-data"),
            limits: Limits::default(),
        },
    )
    .unwrap();
    tauri::async_runtime::block_on(host.discover()).unwrap();
    Fixture { temp, host }
}

fn get(host: &PluginHost, path: &str) -> Response<Vec<u8>> {
    tauri::async_runtime::block_on(respond(host, &Method::GET, path))
}

#[test]
fn serves_only_enabled_plugins_files_inside_their_folder() {
    let Fixture { temp, host } = fixture();
    let outside = temp.path().join("secret.txt");
    assert!(outside.is_file());
    // Disabled plugins serve nothing.
    assert_eq!(get(&host, "/dev.example.hltb/dist/ui.js").status(), 404);
    tauri::async_runtime::block_on(host.set_enabled("dev.example.hltb", true)).unwrap();

    let module = get(&host, "/dev.example.hltb/dist/ui.js");
    assert_eq!(module.status(), 200);
    assert_eq!(module.headers()[header::CONTENT_TYPE], "text/javascript");
    assert_eq!(module.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
    assert_eq!(module.headers()[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert_eq!(module.body(), b"export default {}");
    assert_eq!(
        get(&host, "/dev.example.hltb/dist/ui.css").headers()[header::CONTENT_TYPE],
        "text/css"
    );

    for escape in [
        "/dev.example.hltb/../secret.txt",
        "/dev.example.hltb/dist/%2e%2e/%2e%2e/secret.txt",
        "/dev.example.hltb/dist%2f..%2f..%2fsecret.txt",
        "/dev.example.hltb//etc/passwd",
        "/dev.example.hltb/dist\\ui.js",
    ] {
        assert_eq!(get(&host, escape).status(), 400, "{escape}");
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, temp.path().join("plugins/hltb/dist/link.txt"))
            .unwrap();
        assert_eq!(get(&host, "/dev.example.hltb/dist/link.txt").status(), 403);
    }
    assert_eq!(
        get(&host, "/dev.example.hltb/dist/missing.js").status(),
        404
    );
    assert_eq!(get(&host, "/dev.example.other/ui.js").status(), 404);
    assert_eq!(get(&host, "/dev.example.hltb").status(), 404);
    assert_eq!(
        tauri::async_runtime::block_on(respond(
            &host,
            &Method::POST,
            "/dev.example.hltb/dist/ui.js"
        ))
        .status(),
        405
    );
}

#[test]
fn ui_plugins_lists_entry_urls_style_and_declared_contributions() {
    let Fixture { temp: _temp, host } = fixture();
    let listed = tauri::async_runtime::block_on(crate::commands::plugin_ui::ui_plugins(&host));
    assert!(listed.is_empty(), "disabled plugins load no UI");
    tauri::async_runtime::block_on(host.set_enabled("dev.example.hltb", true)).unwrap();
    let listed = tauri::async_runtime::block_on(crate::commands::plugin_ui::ui_plugins(&host));
    let base = if cfg!(windows) {
        "http://caskade-plugin.localhost"
    } else {
        "caskade-plugin://localhost"
    };
    assert_eq!(listed.len(), 1);
    assert_eq!(
        listed[0].entry,
        format!("{base}/dev.example.hltb/dist/ui.js")
    );
    assert_eq!(
        listed[0].style.as_deref(),
        Some(format!("{base}/dev.example.hltb/dist/ui.css").as_str())
    );
    let contributes: Vec<_> = listed[0]
        .contributes
        .iter()
        .map(|item| (item.slot.as_str(), item.id.as_str()))
        .collect();
    assert_eq!(
        contributes,
        [
            ("game.details.tabs", "hltb"),
            ("views", "stats"),
            ("commands", "refresh")
        ]
    );
}

#[test]
fn urls_percent_encode_segments() {
    assert!(url("dev.x", Path::new("dist/my ui.js")).ends_with("/dev.x/dist/my%20ui.js"));
    assert_eq!(decode("my%20ui.js").as_deref(), Some("my ui.js"));
    assert_eq!(decode("bad%2"), None);
}
