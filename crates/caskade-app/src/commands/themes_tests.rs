use super::*;
use crate::{
    error::ErrorCode,
    events::{Emit, Event},
};
use caskade_core::{AppPaths, db::LibraryStore, themes::ThemeSource};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

fn neon(themes: &Path) {
    let folder = themes.join("neon");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("theme.json"),
        r#"{"id": "dev.example.neon", "name": "Neon", "version": "0.1.0", "extends": "caskade.default", "modes": ["light", "dark"]}"#,
    )
    .unwrap();
}

/// The shell's address for a path in a theme.
fn url(id: &str, path: &str) -> String {
    format!("caskade-theme://{id}/{path}")
}

/// What reached the UI.
#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<Event>>>);

impl Emit for Sink {
    fn emit(&self, event: Event) -> Result<(), String> {
        self.0.lock().unwrap().push(event);
        Ok(())
    }
}

/// One run of the app: the themes folder and library database stay put
/// between runs, like a restart.
struct Run {
    settings: SettingsService,
    themes: ThemeService,
    safe: bool,
}

impl Run {
    async fn start(root: &Path, safe: bool) -> Self {
        let store = LibraryStore::open(&root.join("library.db")).await.unwrap();
        let settings = SettingsService::new(store);
        settings.register_core(&AppPaths::under(root)).unwrap();
        Self {
            settings,
            themes: ThemeService::new(root.join("themes")),
            safe,
        }
    }

    async fn resolve(&self, id: Option<String>) -> ResolvedTheme {
        theme_resolve(&self.themes, Some(&self.settings), None, self.safe, url, id)
            .await
            .unwrap()
    }

    async fn active(&self) -> ResolvedTheme {
        self.resolve(None).await
    }

    async fn chosen(&self) -> serde_json::Value {
        crate::commands::settings::settings_get(&self.settings, "core.appearance")
            .await
            .unwrap()
            .value["theme"]
            .clone()
    }

    async fn choose(&self, theme: &str) {
        let mut value = crate::commands::settings::settings_get(&self.settings, "core.appearance")
            .await
            .unwrap()
            .value;
        value["theme"] = theme.into();
        crate::commands::settings::settings_set(&self.settings, "core.appearance", value, None)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn the_chosen_theme_survives_a_restart_and_a_removed_one_falls_back_with_a_notice() {
    let temp = tempfile::tempdir().unwrap();
    neon(&temp.path().join("themes"));
    {
        let run = Run::start(temp.path(), false).await;
        let first = run.active().await;
        assert_eq!(first.id, "caskade.default");
        assert_eq!(first.notice, None);
        assert_eq!(first.modes, ["dark"]);
        run.choose("dev.example.neon").await;
        assert_eq!(
            run.active().await,
            ResolvedTheme {
                id: "dev.example.neon".into(),
                name: "Neon".into(),
                requested: "dev.example.neon".into(),
                modes: vec!["light".into(), "dark".into()],
                url: url("dev.example.neon", "theme.css"),
                notice: None,
                safe_mode: false,
            }
        );
        // An explicit id (the picker's live switch) resolves the same way.
        assert_eq!(
            run.resolve(Some("caskade.default".into())).await.url,
            url("caskade.default", "theme.css")
        );
    }

    // Restart: still Neon.
    let run = Run::start(temp.path(), false).await;
    assert_eq!(run.active().await.id, "dev.example.neon");
    drop(run);

    // Removed while selected: the default, with a notice; the choice stays.
    std::fs::remove_dir_all(temp.path().join("themes/neon")).unwrap();
    let run = Run::start(temp.path(), false).await;
    let fallback = run.active().await;
    assert_eq!(fallback.id, "caskade.default");
    assert_eq!(fallback.requested, "dev.example.neon");
    assert_eq!(fallback.url, url("caskade.default", "theme.css"));
    assert!(
        fallback
            .notice
            .as_deref()
            .unwrap()
            .contains("isn't installed any more")
    );
    let gone = theme_stylesheet(&run.themes, url, "dev.example.neon").unwrap_err();
    assert_eq!(gone.code, ErrorCode::Invalid, "{gone:?}");
    assert!(
        !gone.summary.contains("dev.example.neon"),
        "{}",
        gone.summary
    );
    assert_eq!(run.chosen().await, "dev.example.neon");
}

#[tokio::test]
async fn safe_mode_starts_on_the_default_theme() {
    let temp = tempfile::tempdir().unwrap();
    neon(&temp.path().join("themes"));
    let run = Run::start(temp.path(), false).await;
    run.choose("dev.example.neon").await;
    assert!(!run.active().await.safe_mode);
    drop(run);

    let run = Run::start(temp.path(), true).await;
    let active = run.active().await;
    assert_eq!(active.id, "caskade.default");
    assert_eq!(active.requested, "dev.example.neon");
    assert!(active.safe_mode);
    assert_eq!(active.notice.as_deref(), Some(SAFE_MODE_NOTICE));
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

#[tokio::test(flavor = "multi_thread")]
async fn a_dev_theme_folder_is_watched_and_the_ui_told_of_each_edit() {
    let temp = tempfile::tempdir().unwrap();
    let work = temp.path().join("work");
    neon(&work);
    let folder = work.join("neon");
    assert!(is_theme_folder(&folder));
    assert!(!is_theme_folder(&work));
    let run = Run::start(temp.path(), false).await;
    let sink = Sink::default();
    let id = load_dev(Events::new(sink.clone()), Some(&run.themes), &folder).unwrap();
    assert_eq!(id, "dev.example.neon");
    let listed = themes_list(&run.themes, None, url).await.unwrap();
    assert_eq!(listed[0].source, ThemeSource::Dev);

    std::fs::write(
        folder.join("tokens.json"),
        r##"{"color": {"accent": {"$root": {"$value": "#22c55e"}}}}"##,
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    let reloaded = loop {
        let found = sink.0.lock().unwrap().iter().find_map(|event| match event {
            Event::ThemeReloaded(reloaded) => Some(reloaded.clone()),
            _ => None,
        });
        if let Some(reloaded) = found {
            break reloaded;
        }
        assert!(
            Instant::now() < deadline,
            "the UI hears of the edit within a second"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    assert_eq!(
        reloaded,
        ThemeReloaded {
            theme: "dev.example.neon".into(),
            error: None,
        }
    );
    assert!(
        run.themes
            .stylesheet("dev.example.neon")
            .unwrap()
            .contains("--cs-color-accent: #22c55e;")
    );
}
