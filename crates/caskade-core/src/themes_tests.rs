use super::*;

fn write(folder: &Path, files: &[(&str, &str)]) {
    for (path, text) in files {
        let file = folder.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
}

#[test]
fn lists_the_default_and_installed_themes_and_a_broken_one_hides_nothing() {
    let temp = tempfile::tempdir().unwrap();
    write(
        &temp.path().join("neon"),
        &[
            (
                "theme.json",
                r#"{"id": "dev.example.neon", "name": "Neon", "version": "0.1.0", "extends": "caskade.default", "previews": ["assets/shot.png"]}"#,
            ),
            (
                "tokens.json",
                r##"{"color": {"accent": {"$root": {"$value": "#e0457b"}}}}"##,
            ),
            (
                "theme.css",
                "[data-cs=\"hero\"] { background: url(assets/shot.png); }",
            ),
            ("assets/shot.png", "png"),
        ],
    );
    write(
        &temp.path().join("broken"),
        &[
            (
                "theme.json",
                r#"{"id": "dev.example.broken", "name": "Broken", "version": "0.1.0"}"#,
            ),
            ("theme.css", ".card { color: red; }"),
            ("boot.js", "alert(1)"),
        ],
    );
    write(&temp.path().join("not-a-theme"), &[("readme.txt", "hi")]);
    write(
        &temp.path().join("orphan"),
        &[(
            "theme.json",
            r#"{"id": "dev.example.orphan", "name": "Orphan", "version": "0.1.0", "extends": "dev.example.gone"}"#,
        )],
    );
    write(
        &temp.path().join("copy"),
        &[(
            "theme.json",
            r#"{"id": "caskade.default", "name": "Copy", "version": "9.0.0"}"#,
        )],
    );
    let service = ThemeService::new(temp.path());
    let themes = service.list();
    let summary: Vec<(&str, ThemeSource, usize)> = themes
        .iter()
        .map(|theme| (theme.id.as_str(), theme.source, theme.problems.len()))
        .collect();
    assert_eq!(
        summary,
        [
            ("caskade.default", ThemeSource::Builtin, 0),
            ("caskade.paper", ThemeSource::Builtin, 0),
            ("dev.example.broken", ThemeSource::User, 2),
            ("caskade.default", ThemeSource::User, 1),
            ("dev.example.neon", ThemeSource::User, 0),
            ("folder:not-a-theme", ThemeSource::User, 1),
            ("dev.example.orphan", ThemeSource::User, 1),
        ]
    );
    assert_eq!(
        themes[3].problems[0].message,
        "id caskade.default is already used by the built-in theme Nexus"
    );
    assert_eq!(
        themes[6].problems[0].message,
        "dev.example.orphan extends dev.example.gone, which isn't installed or isn't valid"
    );

    let css = service.stylesheet("dev.example.neon").unwrap();
    assert!(css.contains("@layer cs.theme {"));
    assert!(css.contains("--cs-color-accent: #e0457b;"));
    assert!(
        css.contains("/* caskade.default */"),
        "the default theme's effects come first"
    );
    assert!(css.contains(r#"url("/dev.example.neon/assets/shot.png")"#));
    assert_eq!(
        service
            .asset("dev.example.neon", "assets/shot.png")
            .as_deref(),
        Some(&b"png"[..])
    );
    assert_eq!(service.asset("dev.example.neon", "theme.json"), None);
    assert_eq!(service.asset("dev.example.broken", "boot.js"), None);
    let error = service
        .stylesheet("dev.example.broken")
        .unwrap_err()
        .to_string();
    assert!(
        error.starts_with("The theme ") && error.contains(" can't be applied: "),
        "{error}"
    );
    assert!(
        !service
            .stylesheet("dev.example.missing")
            .unwrap_err()
            .to_string()
            .contains("dev.example"),
        "no theme id in the error"
    );
    assert!(service.stylesheet("dev.example.missing").is_err());
}

#[test]
fn the_built_in_default_compiles_into_the_theme_layer() {
    let temp = tempfile::tempdir().unwrap();
    let css = ThemeService::new(temp.path().join("none"))
        .stylesheet(DEFAULT_THEME)
        .unwrap();
    assert!(css.starts_with(
        "/* caskade.default 1.0.0, compiled by caskade-theme. */\n@layer cs.theme {\n"
    ));
    assert!(css.contains("[data-cs=\"backdrop\"]"));
}

fn neon(folder: &Path, accent: &str) {
    let tokens = format!(r#"{{"color": {{"accent": {{"$root": {{"$value": "{accent}"}}}}}}}}"#);
    write(
        folder,
        &[
            (
                "theme.json",
                r#"{"id": "dev.example.neon", "name": "Neon", "version": "0.1.0", "extends": "caskade.default", "modes": ["light", "dark"]}"#,
            ),
            ("tokens.json", tokens.as_str()),
        ],
    );
}

#[test]
fn the_selected_theme_applies_and_falls_back_to_the_default_with_a_notice() {
    let temp = tempfile::tempdir().unwrap();
    neon(&temp.path().join("neon"), "#e0457b");
    let service = ThemeService::new(temp.path());
    let active = service.resolve("dev.example.neon");
    assert_eq!(
        active,
        ActiveTheme {
            id: "dev.example.neon".into(),
            name: "Neon".into(),
            requested: "dev.example.neon".into(),
            modes: vec!["light".into(), "dark".into()],
            notice: None,
        }
    );
    let default = service.resolve(DEFAULT_THEME);
    assert_eq!(
        (default.id.as_str(), default.modes, default.notice),
        (DEFAULT_THEME, vec!["dark".to_owned()], None)
    );

    // Broken while selected: the default, saying why.
    write(
        &temp.path().join("neon"),
        &[("theme.css", ".card { color: red; }")],
    );
    service.reload();
    let broken = service.resolve("dev.example.neon");
    assert_eq!(broken.id, DEFAULT_THEME);
    assert_eq!(broken.requested, "dev.example.neon");
    let notice = broken.notice.unwrap();
    assert!(
        notice.starts_with("The theme Neon can't be applied: ")
            && notice.ends_with("Caskade is using the default theme instead."),
        "{notice}"
    );

    // Removed while selected.
    std::fs::remove_dir_all(temp.path().join("neon")).unwrap();
    service.reload();
    let removed = service.resolve("dev.example.neon");
    assert_eq!(removed.id, DEFAULT_THEME);
    assert_eq!(removed.name, "Nexus");
    assert_eq!(
        removed.notice.as_deref(),
        Some(
            "The chosen theme isn't installed any more. Caskade is using the default theme instead."
        )
    );
}

#[test]
fn the_built_ins_come_first_with_the_default_at_their_head() {
    let temp = tempfile::tempdir().unwrap();
    let themes = ThemeService::new(temp.path()).list();
    let builtins: Vec<&str> = themes
        .iter()
        .filter(|theme| theme.source == ThemeSource::Builtin)
        .map(|theme| theme.id.as_str())
        .collect();
    assert_eq!(builtins.first(), Some(&DEFAULT_THEME));
    assert!(
        themes
            .iter()
            .all(|theme| theme.problems.is_empty() && theme.folder.is_none()),
        "every built-in theme is valid"
    );
}

#[tokio::test]
async fn the_theme_choice_persists_and_legacy_values_migrate_to_theme_ids() {
    use crate::{AppPaths, db::LibraryStore, settings::SettingsService};
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(temp.path());
    let database = temp.path().join("library.db");
    let key = "settings.section.core.appearance";
    let legacy = |theme: &str| {
        serde_json::json!({
            "schemaVersion": 1,
            "revision": 4,
            "value": {
                "theme": theme, "density": "compact", "cardSize": 176, "uiScale": 100,
                "showArtTitles": true, "motion": "system", "reduceEffects": false,
                "accent": "#ff8800"
            }
        })
    };
    for theme in ["nexus", "terminal"] {
        let store = LibraryStore::open(&database).await.unwrap();
        store.set_setting(key, &legacy(theme)).await.unwrap();
        let settings = SettingsService::new(store);
        settings.register_core(&paths).unwrap();
        let snapshot = settings.get("core.appearance").await.unwrap();
        assert_eq!(snapshot.version, 2);
        assert_eq!(snapshot.value["theme"], DEFAULT_THEME, "{theme}");
        // Everything else is kept; user CSS starts empty and enabled.
        assert_eq!(snapshot.value["density"], "compact");
        assert_eq!(snapshot.value["accent"], "#ff8800");
        assert_eq!(
            serde_json::from_value::<crate::settings::AppearanceSettings>(snapshot.value)
                .map(|value| (value.custom_css, value.custom_css_enabled))
                .unwrap(),
            (String::new(), true)
        );
    }

    // Chosen, then read back by a fresh service after a restart.
    {
        let settings = SettingsService::new(LibraryStore::open(&database).await.unwrap());
        settings.register_core(&paths).unwrap();
        let mut value = settings.get("core.appearance").await.unwrap().value;
        value["theme"] = "dev.example.neon".into();
        value["customCss"] = "[data-cs=\"hero\"] { color: red; }".into();
        settings.set("core.appearance", value, None).await.unwrap();
        for bad in [
            serde_json::json!("Not A Theme"),
            serde_json::json!("nexus"),
            serde_json::json!(""),
        ] {
            let mut value = settings.get("core.appearance").await.unwrap().value;
            value["theme"] = bad;
            assert!(settings.set("core.appearance", value, None).await.is_err());
        }
        let mut value = settings.get("core.appearance").await.unwrap().value;
        value["customCss"] = "a".repeat(32 * 1024 + 1).into();
        assert!(
            settings.set("core.appearance", value, None).await.is_err(),
            "custom CSS is bounded"
        );
    }
    let settings = SettingsService::new(LibraryStore::open(&database).await.unwrap());
    settings.register_core(&paths).unwrap();
    let value = settings.get("core.appearance").await.unwrap().value;
    assert_eq!(value["theme"], "dev.example.neon");
    assert_eq!(value["customCss"], "[data-cs=\"hero\"] { color: red; }");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dev_theme_recompiles_within_a_second_of_an_edit() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("work/neon");
    neon(&folder, "#e0457b");
    let service = ThemeService::new(temp.path().join("installed"));
    let (sender, mut changes) = tokio::sync::mpsc::unbounded_channel();
    let id = service
        .watch_dev(&folder, DEV_DEBOUNCE, move |id, error| {
            let _ = sender.send((id.to_owned(), error));
        })
        .unwrap();
    assert_eq!(id, "dev.example.neon");
    let listed = service.list();
    assert_eq!(
        (listed[0].id.as_str(), listed[0].source),
        ("dev.example.neon", ThemeSource::Dev)
    );
    assert!(
        service
            .stylesheet(&id)
            .unwrap()
            .contains("--cs-color-accent: #e0457b;")
    );
    // Watching the same folder again is a no-op.
    assert_eq!(
        service.watch_dev(&folder, DEV_DEBOUNCE, |_, _| {}).unwrap(),
        id
    );

    let edited = std::time::Instant::now();
    neon(&folder, "#22c55e");
    let (changed, error) = tokio::time::timeout(Duration::from_secs(1), changes.recv())
        .await
        .expect("recompiled within a second")
        .unwrap();
    assert!(edited.elapsed() < Duration::from_secs(1));
    assert_eq!((changed.as_str(), error), ("dev.example.neon", None));
    let css = service.stylesheet(&id).unwrap();
    assert!(css.contains("--cs-color-accent: #22c55e;"), "{css}");

    // A broken edit is reported, and the theme falls back until it's fixed.
    write(&folder, &[("theme.css", "#app { color: red; }")]);
    let error = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some((_, Some(error))) = changes.recv().await {
                return error;
            }
        }
    })
    .await
    .expect("the broken edit is reported");
    assert!(
        error.starts_with("The theme ") && error.contains(" can't be applied"),
        "{error}"
    );
    assert_eq!(service.resolve(&id).id, DEFAULT_THEME);
}

#[test]
fn a_broken_translation_catalog_is_a_warning_and_the_theme_still_applies() {
    let temp = tempfile::tempdir().unwrap();
    write(
        &temp.path().join("neon"),
        &[
            (
                "theme.json",
                r#"{"id": "dev.example.neon", "name": "Neon", "version": "0.1.0"}"#,
            ),
            ("locales/sv.po", "msgid \"Neon\"\nmsgstr \"Neonljus\n"),
        ],
    );
    write(
        &temp.path().join("plain"),
        &[(
            "theme.json",
            r#"{"id": "dev.example.plain", "name": "Plain", "version": "0.1.0"}"#,
        )],
    );
    let service = ThemeService::new(temp.path());
    let themes = service.list();
    let neon = themes
        .iter()
        .find(|theme| theme.id == "dev.example.neon")
        .unwrap();
    assert!(neon.problems.is_empty(), "{:?}", neon.problems);
    assert_eq!(neon.warnings.len(), 1, "{:?}", neon.warnings);
    assert_eq!(neon.warnings[0].file, "locales/sv.po");
    assert_eq!(neon.name, "Neon");
    assert!(service.stylesheet("dev.example.neon").is_ok());
    let plain = themes
        .iter()
        .find(|theme| theme.id == "dev.example.plain")
        .unwrap();
    assert!(plain.problems.is_empty() && plain.warnings.is_empty());
}
