//! Console mode: the first mode, and the mode the toggle remembers. The
//! window's part (fullscreen and back, held closes) is the shell's.
use super::*;
use caskade_core::db::LibraryStore;

fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let pairs: Vec<(String, String)> = pairs
        .iter()
        .map(|(key, value)| ((*key).into(), (*value).into()))
        .collect();
    move |name| {
        pairs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    }
}

fn settings(start: &str, active: bool) -> ConsoleSettings {
    ConsoleSettings {
        start: start.into(),
        active,
    }
}

async fn service() -> SettingsService {
    let store = LibraryStore::in_memory().await.unwrap();
    let service = SettingsService::new(store);
    service
        .register_core(&caskade_core::AppPaths::under(std::path::Path::new(
            "fixture",
        )))
        .unwrap();
    service
}

/// #244: a new handheld or TV starts in console mode, as the first
/// settings by kind of device leave it; a new desktop doesn't.
#[tokio::test]
async fn a_new_handheld_starts_in_console_mode_and_a_desktop_doesnt() {
    use caskade_core::device_type::{DeviceKind, settle_new_device};
    async fn first_mode(kind: DeviceKind) -> bool {
        let temp = tempfile::tempdir().unwrap();
        let store = LibraryStore::in_memory().await.unwrap();
        let service = SettingsService::new(store.clone());
        service
            .register_core(&caskade_core::paths::AppPaths::under(temp.path()))
            .unwrap();
        settle_new_device(&service, &store, kind).await.unwrap();
        starts_in_console(&Startup::from(&["caskade".to_owned()], env(&[])), &service).await
    }
    assert!(first_mode(DeviceKind::Handheld).await);
    assert!(first_mode(DeviceKind::Tv).await);
    assert!(!first_mode(DeviceKind::Desktop).await);
    assert!(!first_mode(DeviceKind::Laptop).await);
}

#[test]
fn the_flag_then_the_environment_then_the_setting_decide_the_first_mode() {
    let args = |extra: &[&str]| {
        std::iter::once("caskade")
            .chain(extra.iter().copied())
            .map(String::from)
            .collect::<Vec<_>>()
    };
    let plain = Startup::from(&args(&[]), env(&[]));
    assert!(!plain.console(&settings("auto", false)));
    assert!(plain.console(&settings("auto", true)), "auto remembers");
    assert!(plain.console(&settings("console", false)));
    assert!(!plain.console(&settings("desktop", true)));

    let flagged = Startup::from(&args(&["--console"]), env(&[("CASKADE_CONSOLE", "0")]));
    assert!(
        flagged.console(&settings("desktop", false)),
        "--console wins"
    );

    let deck = Startup::from(&args(&[]), env(&[("SteamGamepadUI", "1")]));
    assert!(deck.gamescope);
    assert!(deck.console(&settings("auto", false)));
    assert!(
        !deck.console(&settings("desktop", false)),
        "the setting overrides"
    );

    let forced_off = Startup::from(
        &args(&[]),
        env(&[
            ("GAMESCOPE_WAYLAND_DISPLAY", "gamescope-0"),
            ("CASKADE_CONSOLE", "0"),
        ]),
    );
    assert!(!forced_off.console(&settings("console", true)));
    let forced_on = Startup::from(&args(&[]), env(&[("CASKADE_CONSOLE", "1")]));
    assert!(forced_on.console(&settings("desktop", false)));
}

#[test]
fn gamescope_is_detected_from_its_environment() {
    assert!(in_gamescope(env(&[(
        "GAMESCOPE_WAYLAND_DISPLAY",
        "gamescope-0"
    )])));
    assert!(in_gamescope(env(&[("XDG_CURRENT_DESKTOP", "gamescope")])));
    assert!(!in_gamescope(env(&[("GAMESCOPE_WAYLAND_DISPLAY", "")])));
    assert!(!in_gamescope(env(&[("XDG_CURRENT_DESKTOP", "GNOME")])));
    // The program name is never the flag.
    assert!(!wants_console(&["--console".to_string()]));
}

/// The toggle's choice is remembered once, and an `auto` start comes back
/// in it.
#[tokio::test]
async fn the_toggle_remembers_the_choice_for_auto_starts() {
    let service = service().await;
    // Nothing asks for console mode: Caskade starts on the desktop.
    assert!(!starts_in_console(&Startup::default(), &service).await);

    remember(&service, true).await.unwrap();
    let stored = service.get(SETTINGS).await.unwrap();
    assert_eq!(stored.value["active"], true);
    assert!(starts_in_console(&Startup::default(), &service).await);

    // Asking again changes nothing.
    remember(&service, true).await.unwrap();
    assert_eq!(
        service.get(SETTINGS).await.unwrap().revision,
        stored.revision
    );
    remember(&service, false).await.unwrap();
    let stored = service.get(SETTINGS).await.unwrap();
    assert_eq!(stored.value["active"], false);
    assert!(!starts_in_console(&Startup::default(), &service).await);

    // A restart with `auto` comes back in the remembered mode.
    let mut remembered = stored.value.clone();
    remembered["active"] = true.into();
    service.set(SETTINGS, remembered, None).await.unwrap();
    let preferences: ConsoleSettings = service.read_or_default(SETTINGS).await;
    assert!(Startup::default().console(&preferences));
}
