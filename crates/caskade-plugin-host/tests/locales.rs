//! Plugins' own translations (#219) against the real test guest: the
//! manifest's texts come from `locales/<lang>.po` with the English as id,
//! the UI half gets the catalog, the backend reads the language through
//! `host.locale`, and a broken catalog is a warning that leaves English.

use crate::guest;
use caskade_core::db::LibraryStore;
use caskade_plugin_host::{HostOptions, Limits, PluginHost, PluginServices, PluginStatus};
use serde_json::{Value, json};
use std::path::Path;

const TRANSLATED: &str = "dev.caskade.translated";
const BROKEN: &str = "dev.caskade.broken-po";
const ENGLISH: &str = "dev.caskade.english";

const SV: &str = r#"msgid ""
msgstr ""
"Language: sv\n"
"Plural-Forms: nplurals=2; plural=(n != 1);\n"

msgid "Pack tools"
msgstr "Paketverktyg"

msgid "Installs and checks game packs."
msgstr "Installerar och kontrollerar spelpaket."

msgid "Pack info"
msgstr "Paketinfo"

msgid "Pack stats"
msgstr "Paketstatistik"

msgid "Check packs"
msgstr "Kontrollera paket"

msgid "Repack"
msgstr "Packa om"

msgid "Prefer packs"
msgstr "Föredra paket"

msgid "Rank the pack's plan above the built-in handlers."
msgstr "Placera paketets plan före de inbyggda hanterarna."

msgid "One pack"
msgid_plural "{n} packs"
msgstr[0] "Ett paket"
msgstr[1] "{n} paket"
"#;

fn manifest(id: &str) -> Value {
    json!({
        "id": id,
        "name": "Pack tools",
        "version": "1.0.0",
        "api": "^0.1.1",
        "description": "Installs and checks game packs.",
        "backend": "dist/backend.wasm",
        "permissions": {},
        "activation": ["onRequest"],
        "contributes": {
            "slots": [
                {"slot": "game.details.tabs", "id": "info", "title": "Pack info"},
                {"slot": "game.actions", "id": "repack", "title": "Repack"}
            ],
            "views": [{"id": "stats", "title": "Pack stats"}],
            "commands": [{"id": "check", "title": "Check packs"}],
            "settings": {
                "type": "object",
                "properties": {
                    "preferPacks": {
                        "type": "boolean",
                        "title": "Prefer packs",
                        "description": "Rank the pack's plan above the built-in handlers.",
                        "default": true
                    }
                },
                "additionalProperties": false
            }
        }
    })
}

fn write_plugin(root: &Path, id: &str, catalogs: &[(&str, &str)]) {
    let dir = root.join(id);
    std::fs::create_dir_all(dir.join("dist")).unwrap();
    std::fs::copy(guest(), dir.join("dist/backend.wasm")).unwrap();
    std::fs::write(dir.join("plugin.json"), manifest(id).to_string()).unwrap();
    if !catalogs.is_empty() {
        std::fs::create_dir_all(dir.join("locales")).unwrap();
    }
    for (name, text) in catalogs {
        std::fs::write(dir.join("locales").join(name), text).unwrap();
    }
}

async fn setup() -> (tempfile::TempDir, PluginHost) {
    let temp = tempfile::tempdir().unwrap();
    let plugins = temp.path().join("plugins");
    write_plugin(&plugins, TRANSLATED, &[("sv.po", SV)]);
    // An unterminated string: the catalog can't be read.
    write_plugin(
        &plugins,
        BROKEN,
        &[("sv.po", "msgid \"Pack tools\"\nmsgstr \"Paketverktyg\n")],
    );
    write_plugin(&plugins, ENGLISH, &[]);
    let host = PluginHost::new(
        PluginServices {
            store: LibraryStore::in_memory().await.unwrap(),
            bus: None,
            settings: None,
        },
        HostOptions {
            folders: vec![plugins],
            data: temp.path().join("plugin-data"),
            limits: Limits::default(),
        },
    )
    .unwrap();
    host.pin_locale(Some("sv"));
    (temp, host)
}

#[tokio::test(flavor = "multi_thread")]
async fn manifest_texts_show_in_the_language_in_use() {
    let (_temp, host) = setup().await;
    let plugins = host.discover().await.unwrap();
    let plugin = plugins.iter().find(|p| p.id == TRANSLATED).unwrap();
    assert_eq!(plugin.name, "Paketverktyg");
    assert_eq!(
        plugin.description.as_deref(),
        Some("Installerar och kontrollerar spelpaket.")
    );
    assert!(plugin.warnings.is_empty(), "{:?}", plugin.warnings);
    let contributes = plugin.contributes.as_ref().unwrap();
    let titles: Vec<_> = contributes
        .slots
        .iter()
        .map(|slot| slot.title.as_deref().unwrap())
        .collect();
    assert_eq!(titles, ["Paketinfo", "Packa om"]);
    assert_eq!(contributes.views[0].title, "Paketstatistik");
    assert_eq!(contributes.commands[0].title, "Kontrollera paket");
    let setting = &contributes.settings.as_ref().unwrap()["properties"]["preferPacks"];
    assert_eq!(setting["title"], "Föredra paket");
    assert_eq!(
        setting["description"],
        "Placera paketets plan före de inbyggda hanterarna."
    );

    // The settings section the app lists is translated the same way, and
    // the stored manifest (ids, logic) stays as written.
    let mut schema = host
        .manifest(TRANSLATED)
        .unwrap()
        .contributes
        .settings
        .unwrap();
    assert_eq!(schema["properties"]["preferPacks"]["title"], "Prefer packs");
    host.translate_settings_schema(TRANSLATED, &mut schema);
    assert_eq!(
        schema["properties"]["preferPacks"]["title"],
        "Föredra paket"
    );
    assert_eq!(
        host.translated_manifest(TRANSLATED)
            .unwrap()
            .contributes
            .slots[1]
            .title
            .as_deref(),
        Some("Packa om")
    );

    // Switching the language switches the texts; English needs no catalog.
    host.pin_locale(Some("en"));
    let plugins = host.list().await;
    let plugin = plugins.iter().find(|p| p.id == TRANSLATED).unwrap();
    assert_eq!(plugin.name, "Pack tools");
}

#[tokio::test(flavor = "multi_thread")]
async fn without_catalogs_a_plugin_is_english_with_no_warnings() {
    let (_temp, host) = setup().await;
    let plugins = host.discover().await.unwrap();
    let plugin = plugins.iter().find(|p| p.id == ENGLISH).unwrap();
    assert_eq!(plugin.name, "Pack tools");
    assert!(plugin.warnings.is_empty());
    assert!(plugin.errors.is_empty());
    let messages = host.messages(ENGLISH).unwrap();
    assert_eq!(messages.locale, "en");
    assert!(messages.messages.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_broken_catalog_is_a_warning_and_english_shows() {
    let (_temp, host) = setup().await;
    let plugins = host.discover().await.unwrap();
    let plugin = plugins.iter().find(|p| p.id == BROKEN).unwrap();
    assert_eq!(plugin.name, "Pack tools");
    assert_eq!(
        plugin.status,
        PluginStatus::Disabled,
        "still a valid plugin"
    );
    assert_eq!(plugin.warnings.len(), 1, "{:?}", plugin.warnings);
    assert_eq!(plugin.warnings[0].key, "locales/sv.po");
    assert_eq!(host.messages(BROKEN).unwrap().locale, "en");
    // It still enables and runs.
    host.set_enabled(BROKEN, true).await.unwrap();
    assert_eq!(host.request(BROKEN, "echo", "1").await.unwrap(), "1");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_ui_half_gets_its_catalog() {
    let (_temp, host) = setup().await;
    host.discover().await.unwrap();
    let messages = host.messages(TRANSLATED).unwrap();
    assert_eq!(messages.locale, "sv");
    let repack = messages
        .messages
        .iter()
        .find(|message| message.id == "Repack")
        .unwrap();
    assert_eq!(repack.translations, ["Packa om"]);
    let packs = messages
        .messages
        .iter()
        .find(|message| message.id == "One pack")
        .unwrap();
    assert_eq!(packs.translations, ["Ett paket", "{n} paket"]);
    assert_eq!(&messages.plurals[..3], [1, 0, 1]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_backend_reads_the_language_through_the_wit_world() {
    let (_temp, host) = setup().await;
    host.discover().await.unwrap();
    host.set_enabled(TRANSLATED, true).await.unwrap();
    assert_eq!(
        host.request(TRANSLATED, "locale", "null").await.unwrap(),
        "sv"
    );
    assert_eq!(
        host.request(TRANSLATED, "greeting", "null").await.unwrap(),
        "Hej"
    );
    // A switch reaches the running backend on its next call.
    host.pin_locale(Some("en"));
    assert_eq!(
        host.request(TRANSLATED, "locale", "null").await.unwrap(),
        "en"
    );
    assert_eq!(
        host.request(TRANSLATED, "greeting", "null").await.unwrap(),
        "Hello"
    );
}
