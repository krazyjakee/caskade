use super::*;
use serde_json::json;

fn valid() -> serde_json::Value {
    json!({
        "$schema": "https://caskade.dev/schemas/plugin.json",
        "id": "dev.example.howlongtobeat",
        "name": "HowLongToBeat",
        "version": "0.1.0",
        "api": "^0.1",
        "ui": "dist/ui.js",
        "backend": "dist/backend.wasm",
        "permissions": {"network": ["howlongtobeat.com"], "fs": ["$HOME/ROMs:ro"], "process": false, "library": "read"},
        "activation": ["onView:game.details", "onEvent:game.exited"],
        "contributes": {
            "slots": [{"slot": "game.details.tabs", "id": "hltb", "title": "How long to beat"}],
            "views": [{"id": "stats", "title": "Stats", "icon": "chart-bar", "modes": ["desktop"]}],
            "hooks": ["game.exited", "launch.before"],
            "settings": {"type": "object", "properties": {}}
        }
    })
}

fn problems(value: serde_json::Value) -> Vec<ManifestError> {
    match Manifest::parse(&value.to_string()) {
        Err(errors) => errors,
        Ok(manifest) => manifest.validate(None).err().unwrap_or_default(),
    }
}

fn keys(value: serde_json::Value) -> Vec<String> {
    problems(value).into_iter().map(|error| error.key).collect()
}

#[test]
fn the_plan_example_is_valid() {
    let manifest = Manifest::parse(&valid().to_string()).unwrap();
    manifest.validate(None).unwrap();
    assert_eq!(manifest.permissions.library, LibraryAccess::Read);
    assert_eq!(
        manifest.contributes.views[0].icon.as_deref(),
        Some("chart-bar")
    );
}

#[test]
fn unknown_keys_and_bad_values_name_the_key() {
    let mut value = valid();
    value["permissions"]["camera"] = json!(true);
    let errors = problems(value);
    assert_eq!(errors[0].key, "permissions.camera");
    assert!(
        errors[0].message.contains("unknown field `camera`"),
        "{errors:?}"
    );

    let mut value = valid();
    value["permissions"]["library"] = json!("everything");
    assert_eq!(keys(value), ["permissions.library"]);

    let mut value = valid();
    value["version"] = json!("one");
    assert_eq!(keys(value), ["version"]);
}

#[test]
fn semantic_problems_name_the_key() {
    let cases = [
        ("id", json!("NoDots")),
        ("api", json!("^1.0")),
        ("ui", json!("../outside.js")),
        ("backend", json!("/abs/backend.wasm")),
    ];
    for (key, bad) in cases {
        let mut value = valid();
        value[key] = bad;
        assert_eq!(keys(value), [key]);
    }
    let mut value = valid();
    value["contributes"]["hooks"] = json!(["game.exploded"]);
    assert_eq!(keys(value), ["contributes.hooks[0]"]);
    let mut value = valid();
    value["contributes"]["slots"][0]["slot"] = json!("game.details.sidebar");
    assert_eq!(keys(value), ["contributes.slots[0].slot"]);
    let mut value = valid();
    value["activation"] = json!(["whenever"]);
    assert_eq!(keys(value), ["activation[0]"]);
    for host in [
        "*.com",
        "*.0.1",
        "*",
        "api.*.example.com",
        "exa mple.com",
        "example.com:",
        "example.com:0",
        "example.com:http",
        "::1",
        "[::1",
        "[example.com]:80",
    ] {
        let mut value = valid();
        value["permissions"]["network"] = json!([host]);
        assert_eq!(keys(value), ["permissions.network[0]"], "{host}");
    }
    let mut value = valid();
    value["permissions"]["network"] = json!([
        "*.example.com",
        "*.localhost",
        "localhost",
        "example.com:8443",
        "127.0.0.1:8384",
        "[::1]",
        "[::1]:8080"
    ]);
    assert!(keys(value).is_empty());
    let mut value = valid();
    value["permissions"]["fs"] = json!(["$HOME/ROMs:rx"]);
    assert_eq!(keys(value), ["permissions.fs[0]"]);
    // #272: a drive letter's colon isn't a mode; a bad mode after one is.
    let mut value = valid();
    value["permissions"]["fs"] = json!([
        r"C:\Games",
        r"C:\Games:ro",
        r"D:\Emulation\ROMs:rw",
        "C:/Games",
        "/mnt/a:b/ROMs",
        "~/ROMs"
    ]);
    assert!(keys(value).is_empty());
    for folder in [r"C:\Games:rx", r"C:\Games:", "C:", ":ro", "$HOME/ROMs:RO"] {
        let mut value = valid();
        value["permissions"]["fs"] = json!([folder]);
        assert_eq!(keys(value), ["permissions.fs[0]"], "{folder}");
    }
    let mut value = valid();
    value["contributes"]["settings"] = json!([1]);
    assert_eq!(keys(value), ["contributes.settings"]);
    // Hooks run in the backend.
    let mut value = valid();
    value.as_object_mut().unwrap().remove("backend");
    assert_eq!(
        keys(value),
        ["contributes.hooks[0]", "contributes.hooks[1]"]
    );
}

#[test]
fn the_current_and_previous_minor_api_are_accepted() {
    let req = |text: &str| semver::VersionReq::parse(text).unwrap();
    assert!(api_supported(&req("^0.1")));
    assert!(api_supported(&req("0.1.0")));
    // 0.1.1 added `host.locale` (#219); a plugin that needs it asks for it.
    assert!(api_supported(&req("^0.1.1")));
    assert!(!api_supported(&req("^0.1.2")));
    assert!(!api_supported(&req("^0.2")));
    assert!(!api_supported(&req("^1")));
}

#[cfg(unix)]
#[test]
fn halves_must_stay_inside_the_folder() {
    let temp = tempfile::tempdir().unwrap();
    let plugin = temp.path().join("plugin");
    std::fs::create_dir_all(plugin.join("dist")).unwrap();
    std::fs::write(temp.path().join("secret.wasm"), b"").unwrap();
    std::os::unix::fs::symlink(
        temp.path().join("secret.wasm"),
        plugin.join("dist/backend.wasm"),
    )
    .unwrap();
    std::fs::write(plugin.join("dist/ui.js"), b"").unwrap();
    std::fs::write(plugin.join(MANIFEST), valid().to_string()).unwrap();
    let errors = Manifest::read(&plugin).unwrap_err();
    assert_eq!(errors[0].key, "backend");
    assert!(
        errors[0].message.contains("leaves the plugin folder"),
        "{errors:?}"
    );

    std::fs::remove_file(plugin.join("dist/backend.wasm")).unwrap();
    let errors = Manifest::read(&plugin).unwrap_err();
    assert!(errors[0].message.contains("does not exist"), "{errors:?}");
    std::fs::write(plugin.join("dist/backend.wasm"), b"").unwrap();
    Manifest::read(&plugin).unwrap();
}

#[test]
fn a_plugin_may_bundle_only_themes_inside_its_folder() {
    let temp = tempfile::tempdir().unwrap();
    let plugin = temp.path().join("plugin");
    std::fs::create_dir_all(plugin.join("themes/neon")).unwrap();
    std::fs::write(plugin.join("themes/neon/theme.json"), b"{}").unwrap();
    let manifest = json!({
        "id": "dev.example.neon",
        "name": "Neon",
        "version": "0.1.0",
        "api": "^0.1",
        "contributes": {"themes": ["themes/neon", "themes/missing", "../elsewhere"]}
    });
    std::fs::write(plugin.join(MANIFEST), manifest.to_string()).unwrap();
    let errors = Manifest::read(&plugin).unwrap_err();
    let found: Vec<(&str, &str)> = errors
        .iter()
        .map(|error| (error.key.as_str(), error.message.as_str()))
        .collect();
    assert_eq!(
        found,
        [
            (
                "contributes.themes[1]",
                "themes/missing/theme.json does not exist"
            ),
            (
                "contributes.themes[2]",
                "must be a path inside the plugin folder"
            ),
        ]
    );
}

/// `docs/plugins/plugin.schema.json` is generated; regenerate it with
/// `CASKADE_WRITE_SCHEMA=1 cargo test -p caskade-plugin-host`.
#[test]
fn committed_manifest_schema_is_current() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/plugins/plugin.schema.json");
    let current = format!("{}\n", serde_json::to_string_pretty(&schema()).unwrap());
    if std::env::var_os("CASKADE_WRITE_SCHEMA").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &current).unwrap();
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(
        committed, current,
        "regenerate docs/plugins/plugin.schema.json"
    );
}

#[test]
fn views_and_slots_declare_known_modes_only() {
    let mut manifest = valid();
    manifest["contributes"]["slots"][0]["modes"] = json!(["console", "desktop"]);
    assert!(problems(manifest.clone()).is_empty());
    manifest["contributes"]["slots"][0]["modes"] = json!(["couch"]);
    manifest["contributes"]["views"][0]["modes"] = json!(["tv"]);
    assert_eq!(
        keys(manifest),
        ["contributes.slots[0].modes", "contributes.views[0].modes"]
    );
}

#[test]
fn a_plugin_may_follow_achievement_fetches_but_not_unlock_announcements() {
    let mut value = valid();
    value["contributes"]["hooks"] = json!(["achievements.changed"]);
    assert!(problems(value.clone()).is_empty());
    // Unlock announcements name accounts and achievements: not offered.
    value["contributes"]["hooks"] = json!(["achievements.unlocked"]);
    assert_eq!(keys(value), ["contributes.hooks[0]"]);
}
