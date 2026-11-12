//! Plugins' UI from API 0.2 (#109): a backend draws the contributions its
//! manifest declares as Caskade's components, in its own catalog's
//! language, and hears what the user did; Caskade refuses what it can't
//! draw. Backends built for 0.1 still load while 0.1 is deprecated.

use crate::{guest, guest_v0_1};
use caskade_core::db::LibraryStore;
use caskade_plugin_host::{
    HostError, HostOptions, InterceptResult, Limits, PluginHost, PluginServices, PluginStatus,
};
use caskade_plugin_ui::{Context, Mode, Node, Outcome, heading, note};
use serde_json::json;
use std::path::Path;

const SV: &str = r#"msgid ""
msgstr ""
"Plural-Forms: nplurals=2; plural=(n != 1);\n"

msgid "Hello"
msgstr "Hej"

msgid "Show as"
msgstr "Visa som"

msgid "{count} game"
msgid_plural "{count} games"
msgstr[0] "{count} spel"
msgstr[1] "{count} spel"

msgid "Ran {what}"
msgstr "Körde {what}"
"#;

fn write_plugin(root: &Path, folder: &str, backend: &Path, manifest: serde_json::Value) {
    let dir = root.join(folder);
    std::fs::create_dir_all(dir.join("dist")).unwrap();
    std::fs::create_dir_all(dir.join("locales")).unwrap();
    std::fs::copy(backend, dir.join("dist/backend.wasm")).unwrap();
    std::fs::write(dir.join("locales/sv.po"), SV).unwrap();
    std::fs::write(dir.join("plugin.json"), manifest.to_string()).unwrap();
}

fn drawn(id: &str) -> serde_json::Value {
    json!({
        "id": id,
        "name": "Drawn",
        "version": "0.2.0",
        "api": "^0.2",
        "backend": "dist/backend.wasm",
        "contributes": {
            "views": [{"id": "panel", "title": "Panel"}],
            "slots": [
                {"slot": "game.actions", "id": "hello", "title": "Say hello"},
                {"slot": "home.sections", "id": "bad", "title": "Bad"},
                {"slot": "game.details.panels", "id": "fail"}
            ]
        }
    })
}

async fn host(root: &Path) -> PluginHost {
    PluginHost::new(
        PluginServices {
            store: LibraryStore::in_memory().await.unwrap(),
            bus: None,
            settings: None,
        },
        HostOptions {
            folders: vec![root.join("plugins")],
            data: root.join("plugin-data"),
            limits: Limits::default(),
        },
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_backend_draws_its_contributions_and_hears_what_the_user_did() {
    let temp = tempfile::tempdir().unwrap();
    let plugins = temp.path().join("plugins");
    write_plugin(&plugins, "drawn", guest(), drawn("dev.caskade.drawn"));
    let host = host(temp.path()).await;
    host.discover().await.unwrap();
    host.set_enabled("dev.caskade.drawn", true).await.unwrap();
    host.pin_locale(Some("sv"));
    let id = "dev.caskade.drawn";

    // In the plugin's own catalog's language, with the slot and mode.
    let console = Context {
        mode: Mode::Console,
        ..Context::default()
    };
    let tree = host
        .ui_render(id, "views", "panel", &console)
        .await
        .unwrap();
    assert_eq!(tree[0], heading("Hej"));
    let Node::Segmented { label, value, .. } = &tree[1] else {
        panic!("{tree:?}")
    };
    assert_eq!((label.as_str(), value.as_str()), ("Visa som", "chart"));
    assert_eq!(tree[2], note("2 spel"));
    assert_eq!(tree[3], caskade_plugin_ui::text("views Console"));

    // The state an action returns comes back with the next render.
    let outcome = host
        .ui_act(id, "views", "panel", "view", &json!("table"), &console)
        .await
        .unwrap();
    assert_eq!(
        outcome,
        Outcome {
            state: Some(json!({"view": "table"})),
            notice: None
        }
    );
    let again = host
        .ui_render(
            id,
            "views",
            "panel",
            &Context {
                state: outcome.state.unwrap(),
                ..console.clone()
            },
        )
        .await
        .unwrap();
    assert!(matches!(&again[1], Node::Segmented { value, .. } if value == "table"));

    // A game action runs with its game, and its notice is translated.
    let game = Context {
        game_id: Some("g1".into()),
        ..Context::default()
    };
    let ran = host
        .ui_act(id, "game.actions", "hello", "run", &json!(null), &game)
        .await
        .unwrap();
    assert_eq!(ran.notice.as_deref(), Some("Körde game.actions hello"));
    let panel = host.ui_render(id, "game.details.tabs", "nope", &game).await;
    assert!(
        matches!(panel, Err(HostError::Undeclared { ref slot, .. }) if slot == "game.details.tabs"),
        "{panel:?}"
    );

    // What can't be drawn, and a plugin's own failure, are errors; the
    // plugin keeps working.
    let bad = host
        .ui_render(id, "home.sections", "bad", &Context::default())
        .await;
    assert!(
        matches!(&bad, Err(HostError::BadUi { message, .. }) if message.contains("has space")),
        "{bad:?}"
    );
    let failed = host
        .ui_render(id, "game.details.panels", "fail", &game)
        .await;
    assert!(
        matches!(&failed, Err(HostError::Plugin { code, .. }) if code == "lookup"),
        "{failed:?}"
    );
    let unknown = host
        .ui_act(id, "views", "panel", "dance", &json!(null), &console)
        .await;
    assert!(
        matches!(unknown, Err(HostError::Plugin { .. })),
        "{unknown:?}"
    );
    assert!(
        host.ui_render(id, "views", "panel", &Context::default())
            .await
            .is_ok()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_backend_built_for_0_1_still_loads_and_draws_nothing() {
    let temp = tempfile::tempdir().unwrap();
    let plugins = temp.path().join("plugins");
    let old = json!({
        "id": "dev.caskade.old",
        "name": "Old",
        "version": "0.1.0",
        "api": "^0.1",
        "backend": "dist/backend.wasm",
        "contributes": {
            "hooks": ["launch.before"],
            "slots": [{"slot": "game.details.tabs", "id": "info", "title": "Info"}]
        }
    });
    write_plugin(&plugins, "old", guest_v0_1(), old);
    // A 0.2 manifest whose backend was built for 0.1 has no `ui` export.
    write_plugin(&plugins, "stale", guest_v0_1(), drawn("dev.caskade.stale"));
    let host = host(temp.path()).await;
    let listed = host.discover().await.unwrap();
    assert!(
        listed
            .iter()
            .all(|plugin| plugin.status != PluginStatus::Invalid)
    );
    host.set_enabled("dev.caskade.old", true).await.unwrap();
    host.set_enabled("dev.caskade.stale", true).await.unwrap();
    host.pin_locale(Some("sv"));

    assert_eq!(
        host.request("dev.caskade.old", "echo", "{\"a\":1}")
            .await
            .unwrap(),
        "{\"a\":1}"
    );
    assert_eq!(
        host.request("dev.caskade.old", "locale", "").await.unwrap(),
        "sv"
    );
    assert_eq!(
        host.request("dev.caskade.old", "kv", "kept").await.unwrap(),
        "kept"
    );
    assert_eq!(
        host.intercept("dev.caskade.old", "launch.before", "{}")
            .await
            .unwrap(),
        InterceptResult::Cancel("launch.before: old".into())
    );
    let unknown = host.request("dev.caskade.old", "dance", "").await;
    assert!(
        matches!(&unknown, Err(HostError::Plugin { code, .. }) if code == "unknown_method"),
        "{unknown:?}"
    );

    // Its UI is a JavaScript half's (the webview's); the host draws none.
    let old_ui = host
        .ui_render(
            "dev.caskade.old",
            "game.details.tabs",
            "info",
            &Context::default(),
        )
        .await;
    assert!(matches!(old_ui, Err(HostError::NoUi(_))), "{old_ui:?}");
    let stale = host
        .ui_render("dev.caskade.stale", "views", "panel", &Context::default())
        .await;
    assert!(matches!(stale, Err(HostError::NoUi(_))), "{stale:?}");
}
