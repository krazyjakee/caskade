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
            ("caskade.cobalt", ThemeSource::Builtin, 0),
            ("caskade.emerald", ThemeSource::Builtin, 0),
            ("caskade.engage", ThemeSource::Builtin, 0),
            ("dev.example.broken", ThemeSource::User, 2),
            ("caskade.default", ThemeSource::User, 1),
            ("dev.example.neon", ThemeSource::User, 0),
            ("folder:not-a-theme", ThemeSource::User, 1),
            ("dev.example.orphan", ThemeSource::User, 1),
        ]
    );
    assert_eq!(
        themes[5].problems[0].message,
        "id caskade.default is already used by the built-in theme Nexus"
    );
    assert_eq!(
        themes[8].problems[0].message,
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

/// Paper left the built-in set (#125): someone who had it chosen gets the
/// default theme with no notice, and their setting moves there as the app
/// starts, unless Paper is installed (from the catalogue, say).
#[tokio::test]
async fn a_retired_built_in_theme_falls_back_quietly_and_its_choice_moves_to_the_default() {
    use crate::{AppPaths, db::LibraryStore, settings::SettingsService};
    assert_eq!(replacement("caskade.paper"), Some(DEFAULT_THEME));
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(temp.path());
    let service = ThemeService::new(temp.path().join("themes"));
    assert!(
        service
            .list()
            .iter()
            .all(|theme| theme.id != "caskade.paper"),
        "Paper isn't built in"
    );
    let active = service.resolve("caskade.paper");
    assert_eq!(
        (active.id.as_str(), active.requested.as_str(), active.notice),
        (DEFAULT_THEME, "caskade.paper", None)
    );
    // Another theme that went missing still says so.
    assert!(service.resolve("dev.example.gone").notice.is_some());

    let settings = SettingsService::new(
        LibraryStore::open(&temp.path().join("library.db"))
            .await
            .unwrap(),
    );
    settings.register_core(&paths).unwrap();
    let mut value = settings.get("core.appearance").await.unwrap().value;
    value["theme"] = "caskade.paper".into();
    value["accent"] = "#ff8800".into();
    settings.set("core.appearance", value, None).await.unwrap();
    assert!(retire(&settings, &service).await.unwrap());
    let value = settings.get("core.appearance").await.unwrap().value;
    assert_eq!(value["theme"], DEFAULT_THEME);
    assert_eq!(value["accent"], "#ff8800", "the rest is kept");
    assert!(!retire(&settings, &service).await.unwrap(), "once");

    // Installed under its old id, it stays chosen and applies.
    write(
        &temp.path().join("themes/paper"),
        &[(
            "theme.json",
            r#"{"id": "caskade.paper", "name": "Paper", "version": "1.0.0", "extends": "caskade.default"}"#,
        )],
    );
    let service = ThemeService::new(temp.path().join("themes"));
    let mut value = settings.get("core.appearance").await.unwrap().value;
    value["theme"] = "caskade.paper".into();
    settings.set("core.appearance", value, None).await.unwrap();
    assert!(!retire(&settings, &service).await.unwrap());
    assert_eq!(service.resolve("caskade.paper").id, "caskade.paper");
}

/// Cobalt, Emerald and Engage were renamed before any release (#125): a
/// choice of an old id applies the new theme with no notice and moves to
/// it as the app starts. Every replacement is built in.
#[tokio::test]
async fn a_renamed_built_in_theme_applies_under_its_new_id_and_its_choice_moves_there() {
    use crate::{AppPaths, db::LibraryStore, settings::SettingsService};
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(temp.path());
    let service = ThemeService::new(temp.path().join("themes"));
    let built_in: Vec<String> = service.list().into_iter().map(|theme| theme.id).collect();
    for (old, next) in RETIRED {
        assert!(built_in.iter().all(|id| id != old), "{old} isn't built in");
        assert!(built_in.iter().any(|id| id == next), "{next} is built in");
    }
    for (old, next) in [
        ("caskade.cobalt-five", "caskade.cobalt"),
        ("caskade.emerald-one", "caskade.emerald"),
        ("caskade.toggle-two", "caskade.engage"),
    ] {
        let active = service.resolve(old);
        assert_eq!(
            (active.id.as_str(), active.requested.as_str(), active.notice),
            (next, old, None)
        );
    }

    let settings = SettingsService::new(
        LibraryStore::open(&temp.path().join("library.db"))
            .await
            .unwrap(),
    );
    settings.register_core(&paths).unwrap();
    let mut value = settings.get("core.appearance").await.unwrap().value;
    value["theme"] = "caskade.toggle-two".into();
    settings.set("core.appearance", value, None).await.unwrap();
    assert!(retire(&settings, &service).await.unwrap());
    let value = settings.get("core.appearance").await.unwrap().value;
    assert_eq!(value["theme"], "caskade.engage");
    assert!(!retire(&settings, &service).await.unwrap(), "once");
}

/// The first-party themes are inspired by console home screens, never
/// copies of them (#125): no built-in theme names a console or its maker
/// anywhere in its text (manifest, tokens, CSS, translations, previews).
/// `switch` is also one of the anatomy's roots, so a selector naming it
/// (`[data-cs="switch"]`) isn't a mark.
#[test]
fn no_built_in_theme_names_a_console_or_its_maker() {
    const MARKS: &[&str] = &[
        "playstation",
        "ps",
        "ps1",
        "ps2",
        "ps3",
        "ps4",
        "ps5",
        "psn",
        "sony",
        "dualsense",
        "dualshock",
        "xbox",
        "microsoft",
        "nintendo",
        "switch",
        "joy-con",
        "joycon",
        "wii",
        "mario",
        "zelda",
        "halo",
        "kinect",
    ];
    /// Whether `mark` is in `text` as a word of its own.
    fn has_word(text: &str, mark: &str) -> bool {
        let is_word = |c: char| c.is_ascii_alphanumeric();
        text.match_indices(mark).any(|(at, _)| {
            let before = text[..at].chars().next_back();
            let after = text[at + mark.len()..].chars().next();
            !before.is_some_and(is_word) && !after.is_some_and(is_word)
        })
    }
    let mut found = Vec::new();
    for files in builtins() {
        let id = files
            .read("theme.json")
            .and_then(|json| serde_json::from_slice::<serde_json::Value>(&json).ok())
            .and_then(|manifest| manifest["id"].as_str().map(str::to_owned))
            .unwrap_or_default();
        for path in files.list() {
            if [".woff2", ".woff", ".ttf", ".otf", ".png", ".webp", ".jpg"]
                .iter()
                .any(|binary| path.ends_with(binary))
            {
                continue;
            }
            let text = String::from_utf8_lossy(&files.read(&path).unwrap_or_default())
                .to_lowercase()
                .replace("data-cs=\"switch\"", "data-cs=\"\"")
                .replace("data-cs='switch'", "data-cs=''")
                .replace("data-cs=switch", "data-cs=");
            for mark in MARKS {
                if has_word(&text, mark) {
                    found.push(format!("{id} {path}: {mark}"));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "marks in built-in themes:\n{}",
        found.join("\n")
    );
    // The check itself sees a mark.
    assert!(has_word("a playful nod to the ps5 era", "ps5"));
    assert!(!has_word("steps(4)", "ps"));
}

/// #149: the native app's backdrop is the `backdrop.json` nearest a theme
/// in its chain, unless a theme's CSS restyles the backdrop without one.
#[test]
fn a_theme_shows_the_nearest_backdrop_composition_in_its_chain() {
    let temp = tempfile::tempdir().unwrap();
    let user = temp.path().join("themes");
    // Tokens only: Nexus's composition, in Neon's colours.
    neon(&user.join("neon"), "#e0457b");
    // Its own CSS backdrop and no composition: drawn in CSS.
    write(
        &user.join("plain"),
        &[
            (
                "theme.json",
                r#"{"id": "dev.example.plain", "name": "Plain", "version": "0.1.0", "extends": "caskade.default"}"#,
            ),
            (
                "theme.css",
                r#"[data-cs="backdrop"] { background: #102030; }"#,
            ),
        ],
    );
    // Its own composition, with an image.
    write(
        &user.join("stars"),
        &[
            (
                "theme.json",
                r#"{"id": "dev.example.stars", "name": "Stars", "version": "0.1.0", "extends": "dev.example.plain"}"#,
            ),
            (
                "backdrop.json",
                r#"{"layers": [{"type": "image", "src": "assets/stars.png", "size": "cover", "repeat": false}]}"#,
            ),
            ("assets/stars.png", "png"),
        ],
    );
    let themes = ThemeService::new(&user);
    let of = |id: &str| themes.backdrop(id).map(|backdrop| backdrop.theme);
    assert_eq!(of(DEFAULT_THEME).as_deref(), Some(DEFAULT_THEME));
    assert_eq!(of("caskade.cobalt").as_deref(), Some("caskade.cobalt"));
    assert_eq!(of("caskade.emerald").as_deref(), Some("caskade.emerald"));
    assert_eq!(of("caskade.engage").as_deref(), Some("caskade.engage"));
    assert_eq!(of("dev.example.neon").as_deref(), Some(DEFAULT_THEME));
    assert_eq!(of("dev.example.plain"), None);
    let stars = themes.backdrop("dev.example.stars").unwrap();
    assert_eq!(stars.theme, "dev.example.stars");
    assert_eq!(
        stars.images,
        vec![("assets/stars.png".to_owned(), b"png".to_vec())]
    );
    assert_eq!(of("dev.example.missing"), None);
}
