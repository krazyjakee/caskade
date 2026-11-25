//! Updates, launch actions, emulation, owned store releases and peer copies
//! on the game page.

use super::super::{ONE, open, play_button};
use super::{PageScript, page, page_script, ready_emulation};
use crate::game::data::UpdateChange;
use caskade_core::launch::ReleaseActions;
use caskade_core::model::{
    ActionKind, ActionTracking, EntryActionSettings, GameAction, LaunchSpec,
};
use caskade_core::updates::GameUpdate;

fn update(entry_id: &str, game_id: &str) -> GameUpdate {
    GameUpdate {
        entry_id: entry_id.to_owned(),
        game_id: game_id.to_owned(),
        provider: "github".to_owned(),
        project: "tide/water".to_owned(),
        project_url: Some("https://github.com/tide/water".to_owned()),
        installed: Some("1.0".to_owned()),
        latest: Some("1.1".to_owned()),
        latest_url: None,
        available: true,
        skipped: None,
        prereleases: false,
        checked_at_ms: 1_700_000_000_000,
        error: None,
    }
}

fn action(id: &str, entry_id: &str, name: &str, kind: ActionKind) -> GameAction {
    GameAction {
        id: id.to_owned(),
        entry_id: entry_id.to_owned(),
        name: name.to_owned(),
        kind,
        target: LaunchSpec::Uri {
            uri: format!("fixture://{id}"),
        },
        tracking: ActionTracking::default(),
        proton: None,
    }
}

#[test]
fn a_followed_install_offers_its_newer_release_and_skips_it() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _script = page_script(PageScript {
        updates: vec![update("entry-1", "fixture-1")],
        ..PageScript::default()
    });
    let mut window = open("/games/fixture-1");
    window.settle_until("the update", |window| {
        window.find("data-part", "update").is_some()
    });
    let panel = window.get("data-part", "update");
    assert_eq!(
        window.attr(panel, "data-status").as_deref(),
        Some("available")
    );
    let said = window.text(panel);
    assert!(
        said.contains("v1.1 is out") && said.contains("you have v1.0"),
        "{said}"
    );
    let skip = window
        .find_in(panel, "data-part", "skip")
        .expect("Skip this version");
    window.click(skip);
    window.settle_until("the skip", |_| page(|script| script.changes.len()) == 1);
    assert_eq!(
        page(|script| script.changes[0].clone()),
        (
            "entry-1".to_owned(),
            UpdateChange::Skip(Some("1.1".to_owned()))
        )
    );
    let panel = window.get("data-part", "update");
    let install = window
        .find_in(panel, "data-part", "install-update")
        .expect("Update");
    window.click(install);
    window.settle_until("the install", |_| page(|script| script.installs.len()) == 1);
}

#[test]
fn a_release_with_several_launch_actions_picks_the_one_play_starts_and_lists_its_tools() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _script = page_script(PageScript {
        actions: vec![ReleaseActions {
            entry_id: "fixture-emulated-entry-0-0".to_owned(),
            editable_built_in: false,
            has_folder: false,
            proton: false,
            actions: vec![
                action(
                    "main",
                    "fixture-emulated-entry-0-0",
                    "Play",
                    ActionKind::Play,
                ),
                action(
                    "vr",
                    "fixture-emulated-entry-0-0",
                    "Play in VR",
                    ActionKind::Play,
                ),
                action(
                    "config",
                    "fixture-emulated-entry-0-0",
                    "Configure",
                    ActionKind::Utility,
                ),
            ],
            settings: EntryActionSettings::default(),
        }],
        ..PageScript::default()
    });
    let mut window = open("/games/fixture-emulated-0-0");
    window.settle_until("the choice", |window| {
        window.find("data-cs", "play-choice").is_some()
    });
    let choice = window.get("data-cs", "play-choice");
    assert!(
        window.text(choice).contains("Configure"),
        "{}",
        window.text(choice)
    );
    // Play starts "Play in VR" from now on.
    let select = window
        .find_in(choice, "data-part", "play-starts")
        .expect("Play starts");
    window.focus(select);
    window.key(dioxus_native::prelude::keyboard_types::Key::Character(
        " ".into(),
    ));
    window.settle_until("its options", |window| {
        window.find("role", "listbox").is_some()
    });
    window.key(dioxus_native::prelude::keyboard_types::Key::Named(
        dioxus_native::prelude::keyboard_types::NamedKey::ArrowDown,
    ));
    window.key(dioxus_native::prelude::keyboard_types::Key::Named(
        dioxus_native::prelude::keyboard_types::NamedKey::Enter,
    ));
    window.settle_until("the save", |_| page(|script| script.saved.len()) == 1);
    let (entry, settings) = page(|script| script.saved[0].clone());
    assert_eq!(entry, "fixture-emulated-entry-0-0");
    assert_eq!(settings.default_action.as_deref(), Some("vr"));
    assert!(!settings.ask);
}

#[test]
fn an_emulated_release_shows_its_emulator_and_picks_another() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _script = page_script(PageScript::default());
    let mut window = open("/games/fixture-emulated-0-0");
    window.settle_until("the emulator", |window| {
        window.find("data-cs", "emulation").is_some()
    });
    let panel = window.get("data-cs", "emulation");
    assert_eq!(window.attr(panel, "data-status").as_deref(), Some("ready"));
    assert!(
        window.text(panel).contains("Ready"),
        "{}",
        window.text(panel)
    );
    let select = window
        .find_in(panel, "data-part", "launch-with")
        .expect("Launch with");
    window.focus(select);
    window.key(dioxus_native::prelude::keyboard_types::Key::Character(
        " ".into(),
    ));
    window.settle_until("its options", |window| {
        window.find("role", "listbox").is_some()
    });
    window.key(dioxus_native::prelude::keyboard_types::Key::Named(
        dioxus_native::prelude::keyboard_types::NamedKey::ArrowDown,
    ));
    window.key(dioxus_native::prelude::keyboard_types::Key::Named(
        dioxus_native::prelude::keyboard_types::NamedKey::Enter,
    ));
    window.settle_until("the emulator set", |_| {
        page(|script| script.emulator_set.len()) == 1
    });
    let (_, target) = page(|script| script.emulator_set[0].clone());
    assert_eq!(
        target,
        caskade_core::emulation::EmulatorTarget::Profile {
            emulator_id: "retroarch".to_owned(),
            profile_id: "mupen".to_owned(),
        }
    );
}

#[test]
fn an_emulated_release_that_needs_setup_says_how_and_play_waits() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let mut emulation = ready_emulation("fixture-emulated-entry-0-0");
    emulation.readiness = caskade_core::emulation::Readiness::NoEmulator;
    emulation.message = Some("No emulator for N64".to_owned());
    let _script = page_script(PageScript {
        emulations: vec![emulation],
        ..PageScript::default()
    });
    let mut window = open("/games/fixture-emulated-0-0");
    window.settle_until("the setup note", |window| {
        window.find("data-part", "readiness").is_some()
    });
    let note = window.text(window.get("data-part", "readiness"));
    assert!(
        note.contains("No emulator for N64") && note.contains("Configure"),
        "{note}"
    );
    window.settle_until("Play waiting", |window| {
        window.attr(play_button(window), "data-disabled").as_deref() == Some("true")
    });
}

#[test]
fn an_owned_release_installs_through_its_store() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    use caskade_core::store_accounts::{OwnedInstallState, OwnedRelease};
    let _script = page_script(PageScript {
        owned: vec![OwnedRelease {
            entry_id: "owned-1".to_owned(),
            store_account_id: "steam-me".to_owned(),
            provider: "steam".to_owned(),
            provider_name: "Steam".to_owned(),
            account_name: "me".to_owned(),
            product_id: "1".to_owned(),
            title: "Fixture Game 0001".to_owned(),
            state: OwnedInstallState::Available {
                launcher: "Steam".to_owned(),
            },
            refreshed_at_ms: None,
        }],
        ..PageScript::default()
    });
    let mut window = open("/games/fixture-0");
    window.settle_until("the owned release", |window| {
        window.find("data-part", "owned-release").is_some()
    });
    let owned = window.get("data-part", "owned-release");
    assert!(
        window.text(owned).contains("Owned on Steam (me)"),
        "{}",
        window.text(owned)
    );
    let install = window
        .find_in(owned, "data-part", "install")
        .expect("Install with Steam");
    assert!(window.text(install).contains("Install with Steam"));
    window.click(install);
    window.settle_until("the install", |_| {
        page(|script| script.owned_installs.len()) == 1
    });
}

#[test]
fn a_stores_own_download_says_to_pause_it_in_the_launcher() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    use caskade_core::store_accounts::{OwnedInstallState, OwnedRelease};
    let _script = page_script(PageScript {
        owned: vec![OwnedRelease {
            entry_id: "owned-1".to_owned(),
            store_account_id: "steam-me".to_owned(),
            provider: "steam".to_owned(),
            provider_name: "Steam".to_owned(),
            account_name: "me".to_owned(),
            product_id: "1".to_owned(),
            title: "Fixture Game 0001".to_owned(),
            state: OwnedInstallState::Installing {
                launcher: "Steam".to_owned(),
                since_ms: 1,
            },
            refreshed_at_ms: None,
        }],
        ..PageScript::default()
    });
    let mut window = open("/games/fixture-0");
    window.settle_until("the owned release", |window| {
        window.find("data-part", "owned-release").is_some()
    });
    let owned = window.get("data-part", "owned-release");
    let text = window.text(owned);
    assert!(
        text.contains("Waiting for Steam to finish installing it."),
        "{text}"
    );
    // Caskade can't pause Steam's download, and says so (#131).
    let note = window
        .find_in(owned, "data-part", "pause-elsewhere")
        .expect("the pause note");
    assert_eq!(
        window.text(note).trim(),
        "To pause or cancel it, use Steam: Caskade can't pause a store's own download."
    );
}

#[test]
fn a_copy_on_another_device_copies_here_or_plays_there() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    use caskade_core::db::CopyLocation;
    use caskade_core::devices::{DeviceKind, DeviceLink, DeviceOs};
    use caskade_core::sync::copies::PeerCopy;
    let _script = page_script(PageScript {
        copies: vec![PeerCopy {
            device_id: "den".to_owned(),
            device_name: "Den PC".to_owned(),
            kind: DeviceKind::Desktop,
            os: DeviceOs::Linux,
            link: DeviceLink::Lan,
            size_bytes: Some(2 * 1024 * 1024 * 1024),
            location: CopyLocation::Managed,
            blocked: None,
            through_proton: false,
            playing: false,
            wakeable: false,
        }],
        ..PageScript::default()
    });
    let mut window = open("/games/fixture-0");
    window.settle_until("the copy", |window| {
        window.find("data-part", "peer-copy").is_some()
    });
    let row = window.get("data-part", "peer-copy");
    assert!(
        window.text(row).contains("On Den PC"),
        "{}",
        window.text(row)
    );
    let copy = window
        .find_in(row, "data-part", "copy-here")
        .expect("Copy to this computer");
    window.click(copy);
    window.settle_until("the copy", |_| page(|script| script.copied.len()) == 1);
    let row = window.get("data-part", "peer-copy");
    let play = window
        .find_in(row, "data-part", "play-there")
        .expect("Play on Den PC");
    window.click(play);
    window.settle_until("played there", |_| {
        page(|script| script.played_on.len()) == 1
    });
}
