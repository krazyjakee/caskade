//! Saves, achievements and fan ports panels on the game page.

use super::super::{ONE, open};
use super::{PageScript, page, page_script};

#[test]
fn the_saves_panel_restores_shows_where_they_are_and_settles_a_conflict() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    use caskade_core::saves::versions::{SaveHistory, VersionState};
    let ours = crate::game::launch_tests::version("ours", true);
    let theirs = crate::game::launch_tests::version("theirs", false);
    let _script = page_script(PageScript {
        saves: Some(SaveHistory {
            account_id: String::new(),
            versions: vec![
                VersionState {
                    version: ours.clone(),
                    current: true,
                    complete: true,
                },
                VersionState {
                    version: theirs.clone(),
                    current: false,
                    complete: true,
                },
            ],
            conflict: vec!["ours".to_owned(), "theirs".to_owned()],
            syncing: true,
        }),
        save_paths: vec!["/home/me/.local/share/Tidewater/saves".to_owned()],
        ..PageScript::default()
    });
    let mut window = open("/games/fixture-emulated-0-0");
    window.settle_until("the saves", |window| {
        window.find("data-cs", "saves-panel").is_some()
    });
    let panel = window.get("data-cs", "saves-panel");
    let said = window.text(panel);
    assert!(
        said.contains("In use") && said.contains("After playing"),
        "{said}"
    );
    assert!(
        said.contains("/home/me/.local/share/Tidewater/saves"),
        "{said}"
    );
    // Show in folder.
    let reveal = window
        .find_in(panel, "data-part", "reveal")
        .expect("Show in folder");
    window.click(reveal);
    window.settle_until("revealed", |_| page(|script| script.revealed.len()) == 1);
    // Restore the other device's.
    let versions = window.find_all("data-part", "version");
    let theirs_row = versions
        .into_iter()
        .find(|&row| window.attr(row, "data-status").as_deref() == Some("kept"))
        .expect("the kept version");
    let restore = window
        .find_in(theirs_row, "data-part", "restore")
        .expect("Restore");
    window.click(restore);
    window.settle_until("restored", |_| page(|script| script.restored.len()) == 1);
    assert_eq!(page(|script| script.restored[0].1.clone()), "theirs");
    // Both played: choose one.
    let panel = window.get("data-cs", "saves-panel");
    let conflict = window
        .find_in(panel, "data-part", "conflict")
        .expect("the conflict");
    let choose = window
        .find_in(conflict, "data-part", "choose")
        .expect("Choose a save…");
    window.click(choose);
    window.settle_until("the question", |window| {
        window.find("data-cs", "save-conflict").is_some()
    });
    let dialog = window.get("data-cs", "save-conflict");
    let confirm = window
        .find_in(dialog, "data-part", "choose")
        .expect("Use this save");
    assert!(window.text(confirm).contains("Use this save"));
    window.click(confirm);
    window.settle_until("chosen", |_| page(|script| script.save_chosen.len()) == 1);
    assert_eq!(page(|script| script.save_chosen[0].1.clone()), "ours");
}

fn achievement(
    id: &str,
    name: &str,
    unlocked: bool,
    hidden: bool,
) -> caskade_achievements::achievements::Achievement {
    use caskade_achievements::achievements::{
        Achievement, AchievementDefinition, AchievementState,
    };
    Achievement {
        definition: AchievementDefinition {
            id: id.to_owned(),
            name: name.to_owned(),
            description: Some(format!("{name}'s description")),
            hidden,
            icon_url: None,
            locked_icon_url: None,
            progress_max: None,
            rarity_percent: Some(12.5),
            points: Some(10),
        },
        state: AchievementState {
            unlocked,
            ..AchievementState::default()
        },
    }
}

#[test]
fn achievements_show_the_counts_and_hidden_ones_stay_concealed_until_revealed() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    use caskade_achievements::achievements::{
        AchievementCapabilities, AchievementSet, SetAccount, SetStatus, SetSync,
    };
    let _script = page_script(PageScript {
        achievement_sets: vec![AchievementSet {
            id: Some("set-1".to_owned()),
            account: SetAccount {
                store_account_id: "steam-me".to_owned(),
                provider: "steam".to_owned(),
                provider_name: "Steam".to_owned(),
                display_name: "Robin".to_owned(),
            },
            product_id: "1".to_owned(),
            entry_ids: Vec::new(),
            sync: SetSync {
                status: SetStatus::Ready,
                ..SetSync::default()
            },
            capabilities: AchievementCapabilities {
                points: true,
                rarity: true,
                ..AchievementCapabilities::default()
            },
            unlocked: 1,
            total: 2,
            hardcore_unlocked: None,
            achievements: vec![
                achievement("first", "First steps", true, false),
                achievement("secret", "The secret ending", false, true),
            ],
        }],
        ..PageScript::default()
    });
    let mut window = open("/games/fixture-1");
    window.settle_until("the achievements", |window| {
        window.find("data-cs", "achievements").is_some()
    });
    let panel = window.get("data-cs", "achievements");
    assert_eq!(window.attr(panel, "data-state").as_deref(), Some("ready"));
    let said = window.text(panel);
    assert!(said.contains("1 of 2 unlocked (50%)"), "{said}");
    assert!(said.contains("10 of 20 points"), "{said}");
    assert!(
        said.contains("Hidden achievement") && !said.contains("The secret ending"),
        "{said}"
    );
    let reveal = window
        .find_in(panel, "data-part", "reveal")
        .expect("Reveal");
    window.click(reveal);
    window.settle_until("revealed", |window| {
        window
            .text(window.get("data-cs", "achievements"))
            .contains("The secret ending")
    });
    // The hero's summary moves to them.
    let summary = window.get("data-cs", "achievement-summary");
    assert!(
        window.text(summary).contains("1/2 achievements"),
        "{}",
        window.text(summary)
    );
    window.click(summary);
    window.settle_until("the heading focused", |window| {
        window.focused() == window.find("id", "achievements-heading")
    });
    let refresh = window.get("data-part", "refresh");
    window.click(refresh);
    window.settle_until("refreshed", |_| {
        page(|script| script.achievement_refreshes) == 1
    });
}

#[test]
fn achievements_show_their_services_pictures_and_hidden_ones_stay_drawn() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    use caskade_achievements::achievements::{
        AchievementCapabilities, AchievementSet, SetAccount, SetStatus, SetSync,
    };
    let mut first = achievement("first", "First steps", true, false);
    first.definition.icon_url = Some("https://example.invalid/first.png".to_owned());
    let mut secret = achievement("secret", "The secret ending", false, true);
    secret.definition.locked_icon_url = Some("https://example.invalid/secret.png".to_owned());
    let _script = page_script(PageScript {
        achievement_sets: vec![AchievementSet {
            id: Some("set-1".to_owned()),
            account: SetAccount {
                store_account_id: "steam-me".to_owned(),
                provider: "steam".to_owned(),
                provider_name: "Steam".to_owned(),
                display_name: "Robin".to_owned(),
            },
            product_id: "1".to_owned(),
            entry_ids: Vec::new(),
            sync: SetSync {
                status: SetStatus::Ready,
                ..SetSync::default()
            },
            capabilities: AchievementCapabilities::default(),
            unlocked: 1,
            total: 2,
            hardcore_unlocked: None,
            achievements: vec![first, secret],
        }],
        achievement_icons: vec![(
            "first".to_owned(),
            "caskade-asset://localhost/first.png".to_owned(),
        )],
        ..PageScript::default()
    });
    let mut window = open("/games/fixture-1");
    window.settle_until("the achievements", |window| {
        window.find("data-cs", "achievements").is_some()
    });
    // The panel comes near the screen.
    let summary = window.get("data-cs", "achievement-summary");
    window.click(summary);
    window.settle_until("the picture", |window| {
        window
            .find_all("data-part", "icon")
            .into_iter()
            .any(|icon| {
                window
                    .find_in(icon, "src", "caskade-asset://localhost/first.png")
                    .is_some()
            })
    });
    // A hidden one's picture would give it away: not even asked for.
    assert_eq!(page(|script| script.icons_asked.clone()), ["first"]);
}

#[test]
fn a_fan_port_shows_what_it_needs_and_installs_after_asking() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    use caskade_core::ports::{
        GamePorts, Port, PortFileHow, PortHere, PortMatch, PortNeeds, PortsStatus,
    };
    let _script = page_script(PageScript {
        ports: Some(GamePorts {
            status: PortsStatus {
                enabled: true,
                available: true,
                projects: 1,
                checked_at_ms: None,
                credit_url: "https://example.invalid/decomp-buddy".to_owned(),
            },
            ports: vec![Port {
                project: "tide-recomp".to_owned(),
                name: "TideRecomp".to_owned(),
                owner: "fans".to_owned(),
                url: "https://example.invalid/fans/TideRecomp".to_owned(),
                game_title: "Tidewater".to_owned(),
                console: "N64".to_owned(),
                matched_by: PortMatch::Igdb,
                runs_on: vec!["linux".to_owned(), "windows".to_owned()],
                here: PortHere::Installable {
                    asset: "TideRecomp-linux.zip".to_owned(),
                    release: "v1.0".to_owned(),
                    checked: true,
                },
                needs: vec![PortNeeds {
                    label: "Tidewater ROM".to_owned(),
                    kind: "game".to_owned(),
                    region: Some("USA".to_owned()),
                    how: PortFileHow::InAppPicker,
                    have: true,
                }],
                notes: Vec::new(),
                unsure: Vec::new(),
                entry_id: None,
            }],
        }),
        ..PageScript::default()
    });
    let mut window = open("/games/fixture-1");
    window.settle_until("the ports", |window| {
        window.find("data-part", "port").is_some()
    });
    let port = window.get("data-part", "port");
    let said = window.text(port);
    assert!(
        said.contains("fans/TideRecomp") && said.contains("runs on Linux, Windows"),
        "{said}"
    );
    assert!(
        said.contains("Tidewater ROM")
            && said.contains("(USA version)")
            && said.contains("in your library"),
        "{said}"
    );
    let install = window
        .find_in(port, "data-part", "install")
        .expect("Install");
    window.click(install);
    window.settle_until("the question", |window| {
        window.find("data-part", "confirm").is_some()
    });
    let confirm = window.get("data-part", "confirm");
    window.click(confirm);
    window.settle_until("the install", |_| {
        page(|script| script.ports_installed.len()) == 1
    });
    assert_eq!(
        page(|script| script.ports_installed[0].clone()),
        "tide-recomp"
    );
}
