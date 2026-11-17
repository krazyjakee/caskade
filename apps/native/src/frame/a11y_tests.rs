//! What screen readers hear of the real pages (#141): Blitz's AccessKit
//! tree of each page in the frame over the fixture library, headless, as
//! `a11y_snapshot` writes it. Every control a screen reader can land on
//! (a button, link, tab, field, switch, menu item…) has a name, on Home,
//! the library, a game's page, the Store, Collections, Activity, every
//! Settings section and the frame around them; and in the dialogs opened
//! over them (#153): the game editor's tabs and its question, and what
//! Play asks first. The flows' own dialogs (sharing, pairing, storage) are
//! checked in `flows/tests` with [`unnamed_in_dialogs`]. The components'
//! own trees are `ui/tests`; the tree following the UI and its actions
//! reaching the handlers is `tests/accessibility.rs`.

use crate::frame::tests::{ONE, Window};
use crate::game::launch_tests::{Script, script, started, version};
use crate::router::SETTINGS_TABS;
use blitz_dom::{BaseDocument, LocalName, NodeId};

/// AccessKit's roles (as `a11y_snapshot` writes them) for what a screen
/// reader moves to and acts on: each needs a name.
const CONTROLS: &[&str] = &[
    "Button",
    "CheckBox",
    "ComboBox",
    "EditableComboBox",
    "Link",
    "ListBoxOption",
    "MenuItem",
    "MenuItemCheckBox",
    "MenuItemRadio",
    "MultilineTextInput",
    "RadioButton",
    "SearchInput",
    "Slider",
    "SpinButton",
    "Switch",
    "Tab",
    "TextInput",
    "TreeItem",
];

/// The roles AccessKit names from the text inside them when they have no
/// label of their own (`accesskit_consumer`'s `labelled_by`). A tab, a
/// field or a switch needs its own.
const NAMED_BY_CONTENTS: &[&str] = &[
    "Button",
    "CheckBox",
    "DefaultButton",
    "Link",
    "MenuItem",
    "MenuItemCheckBox",
    "MenuItemRadio",
    "RadioButton",
];

/// The lines of `tree` that are controls without a name: no name of their
/// own, and for the roles named by their contents no text inside them
/// either (a quoted line indented under them).
fn unnamed(tree: &str) -> Vec<String> {
    let lines: Vec<&str> = tree.lines().collect();
    let indent = |line: &str| line.len() - line.trim_start().len();
    lines
        .iter()
        .enumerate()
        .filter(|(at, line)| {
            let trimmed = line.trim_start();
            let role = trimmed.split(' ').next().unwrap_or_default();
            if !CONTROLS.contains(&role) || trimmed[role.len()..].starts_with(" \"") {
                return false;
            }
            let has_text = NAMED_BY_CONTENTS.contains(&role)
                && lines[at + 1..]
                    .iter()
                    .take_while(|inner| indent(inner) > indent(line))
                    .any(|inner| inner.trim_start().starts_with('"'));
            !has_text
        })
        .map(|(_, line)| line.trim_start().to_owned())
        .collect()
}

fn heard(window: &Window, node: NodeId) -> String {
    crate::a11y_snapshot::snapshot_of(&window.doc.inner.borrow(), node)
}

/// Every open dialog in `doc` (`role="dialog"` or `"alertdialog"`): how
/// many there are, and their controls without a name, each under its
/// dialog's `data-cs` (or role).
pub(crate) fn unnamed_in_dialogs(doc: &BaseDocument) -> (usize, Vec<String>) {
    let mut dialogs = Vec::new();
    doc.visit(|id, node| {
        let role = node.attr(LocalName::from("role"));
        if matches!(role, Some("dialog" | "alertdialog")) {
            let name = node
                .attr(LocalName::from("data-cs"))
                .or(role)
                .unwrap_or_default()
                .to_owned();
            dialogs.push((id, name));
        }
    });
    let mut found = Vec::new();
    for (id, name) in &dialogs {
        let tree = crate::a11y_snapshot::snapshot_of(doc, *id);
        assert!(!tree.trim().is_empty(), "{name} has an empty tree");
        for line in unnamed(&tree) {
            found.push(format!("{name}: {line}"));
        }
    }
    (dialogs.len(), found)
}

/// The dialog around `node` (the element with `role="dialog"` or
/// `"alertdialog"`), or `node` itself.
fn dialog_around(window: &Window, node: NodeId) -> NodeId {
    let doc = window.doc.inner.borrow();
    std::iter::successors(Some(node), |&id| doc.get_node(id).and_then(|n| n.parent))
        .find(|&id| {
            doc.get_node(id).is_some_and(|n| {
                matches!(
                    n.attr(LocalName::from("role")),
                    Some("dialog" | "alertdialog")
                )
            })
        })
        .unwrap_or(node)
}

/// Adds what `node` (one page, or a part of the frame) has unnamed to
/// `found`, under `place`.
fn check(window: &Window, place: &str, node: NodeId, found: &mut Vec<String>) {
    let tree = heard(window, node);
    assert!(!tree.trim().is_empty(), "{place} has an empty tree");
    for line in unnamed(&tree) {
        found.push(format!("{place}: {line}"));
    }
}

#[test]
fn every_control_on_every_page_has_a_name_for_screen_readers() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = Window::open();
    let mut found = Vec::new();
    for (href, view) in [
        ("/", "home"),
        ("/library?filter=all", "library"),
        ("/games/fixture-1", "game"),
        ("/store?tab=browse", "store"),
        ("/collections", "collections"),
        ("/activity?tab=jobs", "activity"),
    ] {
        window.go(href);
        window.settle_until(href, |window| window.find("data-view", view).is_some());
        // What loads after the page (cards, rows) settles too.
        window.settle();
        check(&window, href, window.get("data-cs", "view"), &mut found);
    }
    for tab in SETTINGS_TABS {
        let href = format!("/settings?tab={tab}");
        window.go(&href);
        let panel = format!("settings-tab-{tab}-panel");
        window.settle_until(&href, |window| window.find("id", &panel).is_some());
        window.settle();
        check(&window, &href, window.get("data-cs", "view"), &mut found);
    }
    // The frame around them.
    for part in ["sidebar", "topbar", "status-bar"] {
        check(&window, part, window.get("data-cs", part), &mut found);
    }
    assert!(
        found.is_empty(),
        "controls a screen reader can't name:\n{}",
        found.join("\n")
    );
}

/// `fixture-1` as the editor shows it.
fn edit_state() -> caskade_core::model::GameEditState {
    let mut game = caskade_core::model::Game::new("Fixture Game 0002");
    game.id = "fixture-1".to_owned();
    let automatic = game.clone();
    caskade_core::model::GameEditState {
        game,
        automatic,
        manual: Vec::new(),
    }
}

/// The game editor (#100) over `fixture-1`'s page, every tab with
/// something to show (artwork to choose, a launch action, a save
/// location): each tab's controls, then the question before unsaved changes
/// go.
#[test]
fn every_control_in_the_game_editor_has_a_name() {
    use crate::game::editor::data::{SaveLocationRow, SaveLocations};
    use crate::game::editor::tests::{EditScript, edit_script};
    use caskade_core::launch::ReleaseActions;
    use caskade_core::model::{
        ActionKind, ActionTracking, Artwork, ArtworkChoice, ArtworkKind, ArtworkSlot,
        EntryActionSettings, GameAction, LaunchSpec, built_in_action_id,
    };
    use caskade_core::saves::{LocationKind, LocationSpec, SaveMatch, SavePreferences};
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let cover = |id: &str, source: &str| {
        let mut art = Artwork::new("fixture-1", ArtworkKind::Cover, id, source, "", 600, 900);
        art.id = id.to_owned();
        art
    };
    let _script = edit_script(EditScript {
        state: Some(edit_state()),
        slots: vec![
            ArtworkSlot {
                kind: ArtworkKind::Cover,
                choice: ArtworkChoice::Automatic,
                shown: Some(cover("cover-1", "igdb")),
                automatic: vec![cover("cover-1", "igdb"), cover("cover-2", "steam")],
            },
            ArtworkSlot {
                kind: ArtworkKind::Hero,
                choice: ArtworkChoice::Removed,
                shown: None,
                automatic: Vec::new(),
            },
        ],
        releases: vec![ReleaseActions {
            entry_id: "entry-1".to_owned(),
            editable_built_in: false,
            has_folder: true,
            proton: false,
            actions: vec![GameAction {
                id: built_in_action_id("entry-1"),
                entry_id: "entry-1".to_owned(),
                name: "Play".to_owned(),
                kind: ActionKind::Play,
                target: LaunchSpec::Executable {
                    path: "game.sh".into(),
                    args: Vec::new(),
                    cwd: None,
                    env: Default::default(),
                },
                tracking: ActionTracking::default(),
                proton: None,
            }],
            settings: EntryActionSettings::default(),
        }],
        locations: Some(SaveLocations {
            revision: 1,
            preferences: SavePreferences::default(),
            matched: SaveMatch::Unknown,
            automatic: vec![SaveLocationRow {
                spec: LocationSpec {
                    kind: LocationKind::Folder,
                    path: "/saves/fixture".to_owned(),
                },
                files: 3,
            }],
            locations: Vec::new(),
            warnings: Vec::new(),
        }),
        ..EditScript::default()
    });
    let mut window = Window::open();
    window.go("/games/fixture-1");
    window.settle_until("Edit details", |window| {
        window.find("data-part", "edit").is_some()
    });
    let edit = window.get("data-part", "edit");
    window.click(edit);
    window.settle_until("the editor", |window| {
        window.find("data-cs", "game-editor").is_some()
            && window.find("data-field", "title").is_some()
    });
    let editor = window.get("data-cs", "game-editor");
    let mut found = Vec::new();
    let tabs = window.find_all_in(editor, "role", "tab");
    assert!(tabs.len() >= 4, "the editor's tabs: {tabs:?}");
    for tab in tabs {
        let label = window.text(tab).trim().to_owned();
        window.click(tab);
        // What the tab loads settles too.
        window.settle();
        window.settle();
        check(&window, &format!("editor, {label}"), editor, &mut found);
    }
    // A change, then Cancel: the question before it goes.
    let details = tabs_first(&window, editor);
    window.click(details);
    window.settle_until("the details", |window| {
        window.find("data-field", "notes").is_some()
    });
    let notes = window.get("data-field", "notes");
    let area = window
        .find_in(notes, "data-part", "input")
        .expect("the notes");
    window.focus(area);
    window.type_text("Named");
    let cancel = window.get("data-part", "cancel");
    window.click(cancel);
    window.settle_until("the question", |window| {
        window.find("data-cs", "game-editor-discard").is_some()
    });
    let question = window.get("data-cs", "game-editor-discard");
    check(&window, "the editor's question", question, &mut found);
    assert!(
        found.is_empty(),
        "controls a screen reader can't name:\n{}",
        found.join("\n")
    );
}

/// The editor's first tab (Details).
fn tabs_first(window: &Window, editor: NodeId) -> NodeId {
    window
        .find_all_in(editor, "role", "tab")
        .into_iter()
        .next()
        .expect("a tab")
}

/// The library's first game, double-clicked: Play, as the library's grid
/// starts it.
fn play_the_first_game() -> (Window, String) {
    let mut window = Window::open();
    window.go("/library?filter=all&sort=title&layout=grid");
    window.settle_until("a card", |window| {
        !window.find_all("data-index", "0").is_empty()
    });
    let card = window.get("data-index", "0");
    let id = window
        .attr(card, "data-focus-key")
        .and_then(|key| key.strip_prefix("game-").map(str::to_owned))
        .expect("the card's game");
    (window, id)
}

/// What Play asks before a game starts (#100), each over the library as
/// the app's answers lead to it: a newer release, the game running on
/// another device, which save to use, a save on its way, and a streaming
/// device busy with another game.
#[test]
fn every_control_in_what_play_asks_has_a_name() {
    use caskade_app::commands::launch::LaunchOutcome;
    use caskade_app::commands::presence::PlayingElsewhere;
    use caskade_core::launch::HostBusy;
    use caskade_core::saves::versions::{BeforePlay, SaveCheck};
    use caskade_core::updates::GameUpdate;
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let mut found = Vec::new();
    type Question = (&'static str, fn(&str) -> Script);
    let questions: [Question; 5] = [
        ("update-offer", |game| Script {
            update: Some(GameUpdate {
                entry_id: "e".to_owned(),
                game_id: game.to_owned(),
                provider: "github".to_owned(),
                project: "tide/water".to_owned(),
                project_url: None,
                installed: Some("1.1".to_owned()),
                latest: Some("1.2".to_owned()),
                latest_url: Some("https://example.invalid/releases".to_owned()),
                available: true,
                skipped: None,
                prereleases: false,
                checked_at_ms: 0,
                error: None,
            }),
            ..Script::default()
        }),
        ("playing-elsewhere", |game| Script {
            playing: vec![PlayingElsewhere {
                device_id: "den".to_owned(),
                device_name: "Den PC".to_owned(),
                game_id: game.to_owned(),
                title: "Tidewater".to_owned(),
                account_id: String::new(),
                started_at_ms: 1_700_000_000_000,
            }],
            ..Script::default()
        }),
        ("save-conflict", |_| Script {
            saves: [BeforePlay {
                check: SaveCheck::Conflict {
                    versions: vec![version("theirs", false), version("ours", true)],
                },
                checked: true,
                warnings: Vec::new(),
            }]
            .into(),
            ..Script::default()
        }),
        ("save-arriving", |_| Script {
            saves: [BeforePlay {
                check: SaveCheck::NotArrived {
                    version: version("theirs", false),
                },
                checked: true,
                warnings: Vec::new(),
            }]
            .into(),
            pause_forever: true,
            ..Script::default()
        }),
        ("stop-host", |game| Script {
            outcomes: [
                LaunchOutcome::HostBusy {
                    busy: HostBusy {
                        entry_id: "e".to_owned(),
                        device: "Den PC".to_owned(),
                        playing: Some("Celeste".to_owned()),
                    },
                },
                started(game),
            ]
            .into(),
            ..Script::default()
        }),
    ];
    for (dialog, answers) in questions {
        let (mut window, game) = play_the_first_game();
        let _script = script(answers(&game));
        let card = window.get("data-index", "0");
        window.double_click(card);
        window.settle_until(dialog, |window| window.find("data-cs", dialog).is_some());
        window.settle();
        let shown = dialog_around(&window, window.get("data-cs", dialog));
        check(&window, dialog, shown, &mut found);
    }
    assert!(
        found.is_empty(),
        "controls a screen reader can't name:\n{}",
        found.join("\n")
    );
}

#[test]
fn a_control_without_a_name_is_found() {
    let tree = "Link \"Home\" current=Page [focus click]\n  Button [focus click]\n  Tab \"General\" selected=true\n  \"Some text\"\n";
    assert_eq!(unnamed(tree), ["Button [focus click]"]);
}
