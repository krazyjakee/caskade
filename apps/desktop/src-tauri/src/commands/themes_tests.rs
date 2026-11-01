use super::*;
use crate::commands::test_ipc::invoke;
use caskade_core::{AppPaths, db::LibraryStore};
use serde_json::{Value, json};
use tauri::{
    Listener, Manager,
    test::{MockRuntime, mock_builder, mock_context, noop_assets},
};
use tauri_specta::Event;

fn neon(themes: &Path) {
    let folder = themes.join("neon");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("theme.json"),
        r#"{"id": "dev.example.neon", "name": "Neon", "version": "0.1.0", "extends": "caskade.default", "modes": ["light", "dark"]}"#,
    )
    .unwrap();
}

/// One run of the app: the themes folder and library database stay put
/// between runs, like a restart.
struct Run {
    app: tauri::App<MockRuntime>,
    window: tauri::WebviewWindow<MockRuntime>,
}

impl Run {
    fn start(root: &Path, safe: bool) -> Self {
        let store =
            tauri::async_runtime::block_on(LibraryStore::open(&root.join("library.db"))).unwrap();
        let settings = SettingsService::new(store);
        settings.register_core(&AppPaths::under(root)).unwrap();
        let specta = tauri_specta::Builder::<MockRuntime>::new()
            .commands(tauri_specta::collect_commands![
                theme_resolve::<MockRuntime>,
                appearance_safe_mode::<MockRuntime>,
                themes_list::<MockRuntime>,
                crate::commands::settings::settings_get,
                crate::commands::settings::settings_set,
            ])
            .events(tauri_specta::collect_events![ThemeReloaded]);
        let app = mock_builder()
            .manage(settings)
            .manage(ThemeService::new(root.join("themes")))
            .manage(SafeMode(safe))
            .invoke_handler(specta.invoke_handler())
            .build(mock_context(noop_assets()))
            .unwrap();
        specta.mount_events(&app);
        let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        Self { app, window }
    }

    fn active(&self) -> Value {
        invoke(&self.window, "theme_resolve", json!({ "id": null })).unwrap()
    }

    fn choose(&self, theme: &str) {
        let mut value = invoke(
            &self.window,
            "settings_get",
            json!({ "namespace": "core.appearance" }),
        )
        .unwrap()["value"]
            .clone();
        value["theme"] = theme.into();
        invoke(
            &self.window,
            "settings_set",
            json!({ "namespace": "core.appearance", "value": value, "expectedRevision": null }),
        )
        .unwrap();
    }
}

fn url(id: &str) -> String {
    crate::theme_scheme::url(id, "theme.css")
}

#[test]
fn the_chosen_theme_survives_a_restart_and_a_removed_one_falls_back_with_a_notice() {
    let temp = tempfile::tempdir().unwrap();
    neon(&temp.path().join("themes"));
    {
        let run = Run::start(temp.path(), false);
        let first = run.active();
        assert_eq!(first["id"], "caskade.default");
        assert_eq!(first["notice"], Value::Null);
        assert_eq!(first["modes"], json!(["dark"]));
        run.choose("dev.example.neon");
        assert_eq!(
            run.active(),
            json!({
                "id": "dev.example.neon",
                "name": "Neon",
                "requested": "dev.example.neon",
                "modes": ["light", "dark"],
                "url": url("dev.example.neon"),
                "notice": null,
                "safeMode": false,
            })
        );
        // An explicit id (the picker's live switch) resolves the same way.
        assert_eq!(
            invoke(
                &run.window,
                "theme_resolve",
                json!({ "id": "caskade.default" })
            )
            .unwrap()["url"],
            url("caskade.default")
        );
    }

    // Restart: still Neon.
    let run = Run::start(temp.path(), false);
    assert_eq!(run.active()["id"], "dev.example.neon");
    drop(run);

    // Removed while selected: the default, with a notice; the choice stays.
    std::fs::remove_dir_all(temp.path().join("themes/neon")).unwrap();
    let run = Run::start(temp.path(), false);
    let fallback = run.active();
    assert_eq!(fallback["id"], "caskade.default");
    assert_eq!(fallback["requested"], "dev.example.neon");
    assert_eq!(fallback["url"], url("caskade.default"));
    assert!(
        fallback["notice"]
            .as_str()
            .unwrap()
            .contains("isn't installed any more")
    );
    let settings = invoke(
        &run.window,
        "settings_get",
        json!({ "namespace": "core.appearance" }),
    )
    .unwrap();
    assert_eq!(settings["value"]["theme"], "dev.example.neon");
}

#[test]
fn safe_mode_starts_on_the_default_theme() {
    let temp = tempfile::tempdir().unwrap();
    neon(&temp.path().join("themes"));
    let run = Run::start(temp.path(), false);
    run.choose("dev.example.neon");
    assert_eq!(
        invoke(&run.window, "appearance_safe_mode", json!({})),
        Ok(json!(false))
    );
    drop(run);

    let run = Run::start(temp.path(), true);
    assert_eq!(
        invoke(&run.window, "appearance_safe_mode", json!({})),
        Ok(json!(true))
    );
    let active = run.active();
    assert_eq!(active["id"], "caskade.default");
    assert_eq!(active["requested"], "dev.example.neon");
    assert_eq!(active["safeMode"], true);
    assert_eq!(active["notice"], SAFE_MODE_NOTICE);
}

#[test]
fn safe_mode_comes_from_the_flag_or_the_environment() {
    let args = |list: &[&str]| list.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
    let none = |_: &str| None;
    let env = |value: &'static str| {
        move |name: &str| (name == "CASKADE_SAFE_MODE").then(|| value.to_owned())
    };
    assert_eq!(
        SafeMode::from(&args(&["caskade", "--safe-mode"]), none),
        SafeMode(true)
    );
    assert_eq!(SafeMode::from(&args(&["caskade"]), none), SafeMode(false));
    assert_eq!(
        SafeMode::from(&args(&["--safe-mode"]), none),
        SafeMode(false),
        "the first argument is the program"
    );
    assert_eq!(
        SafeMode::from(&args(&["caskade"]), env("1")),
        SafeMode(true)
    );
    assert_eq!(
        SafeMode::from(&args(&["caskade"]), env("0")),
        SafeMode(false)
    );
}

#[test]
fn a_dev_theme_folder_is_watched_and_the_webview_told_of_each_edit() {
    let temp = tempfile::tempdir().unwrap();
    let work = temp.path().join("work");
    neon(&work);
    let folder = work.join("neon");
    assert!(is_theme_folder(&folder));
    assert!(!is_theme_folder(&work));
    let run = Run::start(temp.path(), false);
    let (sender, receiver) = std::sync::mpsc::channel();
    run.app.listen(ThemeReloaded::NAME, move |event| {
        let payload: ThemeReloaded = serde_json::from_str(event.payload()).unwrap();
        let _ = sender.send(payload);
    });
    let id = tauri::async_runtime::block_on({
        let handle = run.app.handle().clone();
        let folder = folder.clone();
        async move { load_dev(&handle, &folder) }
    })
    .unwrap();
    assert_eq!(id, "dev.example.neon");
    let listed = invoke(&run.window, "themes_list", json!({})).unwrap();
    assert_eq!(listed[0]["source"], "dev");

    std::fs::write(
        folder.join("tokens.json"),
        r##"{"color": {"accent": {"$root": {"$value": "#22c55e"}}}}"##,
    )
    .unwrap();
    let reloaded = receiver
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("the webview hears of the edit within a second");
    assert_eq!(
        reloaded,
        ThemeReloaded {
            theme: "dev.example.neon".into(),
            error: None,
        }
    );
    let themes = run.app.state::<ThemeService>();
    assert!(
        themes
            .stylesheet("dev.example.neon")
            .unwrap()
            .contains("--cs-color-accent: #22c55e;")
    );
}
