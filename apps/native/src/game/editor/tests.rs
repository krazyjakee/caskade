//! The game editor (#100) in the frame over the fixture library, headless,
//! as `GameEditor.test.tsx`, `ArtworkTab.test.tsx`, `ActionsTab.test.tsx`
//! and `SavesTab.test.tsx` have it: opened from the page's Edit details and
//! the menu's Edit…, the details saved as one patch, a field saved
//! elsewhere kept as a conflict, the question before unsaved work goes, and
//! each tab's own changes.

use super::data::{EditData, SaveLocationRow, SaveLocations};
use crate::frame::tests::{ONE, Window};
use caskade_app::commands::game_edit::ArtworkChange;
use caskade_core::launch::ReleaseActions;
use caskade_core::model::{
    ActionKind, ActionTracking, Artwork, ArtworkChoice, ArtworkKind, ArtworkSlot,
    EntryActionSettings, FieldError, FieldProblem, Game, GameAction, GameEdit, GameEditOutcome,
    GameEditState, GameField, LaunchSpec, built_in_action_id,
};
use caskade_core::saves::{LocationKind, LocationSpec, SaveMatch, SavePreferences};
use caskade_ui_input::Action;
use dioxus_native::prelude::keyboard_types::{Key, Modifiers, NamedKey};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Mutex;

/// What the editor's data answers, and what it was asked.
#[derive(Default)]
pub(crate) struct EditScript {
    pub(crate) state: Option<GameEditState>,
    /// The next save's errors; a save without them is kept.
    pub(crate) refuse: Vec<FieldError>,
    pub(crate) saves: Vec<(GameEdit, Option<Game>)>,
    pub(crate) slots: Vec<ArtworkSlot>,
    pub(crate) artwork: Vec<(ArtworkKind, String)>,
    pub(crate) releases: Vec<ReleaseActions>,
    pub(crate) saved_actions: Vec<GameAction>,
    pub(crate) deleted: Vec<String>,
    pub(crate) locations: Option<SaveLocations>,
    pub(crate) location_edits: Vec<(SavePreferences, u32)>,
}

static EDIT: Mutex<Option<EditScript>> = Mutex::new(None);

fn edit<T>(f: impl FnOnce(&mut EditScript) -> T) -> T {
    let mut script = caskade_core::lock(&EDIT);
    f(script.get_or_insert_with(EditScript::default))
}

pub(crate) struct EditScripted;

pub(crate) fn edit_script(script: EditScript) -> EditScripted {
    *caskade_core::lock(&EDIT) = Some(script);
    EditScripted
}

impl Drop for EditScripted {
    fn drop(&mut self) {
        *caskade_core::lock(&EDIT) = None;
    }
}

fn answer<T: 'static>(value: T) -> crate::game::launch::Answer<T> {
    Box::pin(async move { Ok(value) })
}

/// The editor data every test window reads (`frame::tests`' app provides
/// it).
pub(crate) fn fake_edit_data() -> EditData {
    EditData {
        state: Rc::new(|_| answer(edit(|script| script.state.clone()))),
        save: Rc::new(|_, patch, base| {
            answer(edit(|script| {
                script.saves.push((patch.clone(), base));
                let state = script.state.clone().expect("a game to save");
                if script.refuse.is_empty() {
                    let mut state = state;
                    if let Some(caskade_core::model::TextEdit::Set { value }) = &patch.title {
                        state.game.title = value.clone();
                    }
                    if let Some(caskade_core::model::TextEdit::Set { value }) = &patch.sort_title {
                        state.game.sort_title = value.clone();
                    }
                    script.state = Some(state.clone());
                    GameEditOutcome::Saved {
                        state,
                        changed: vec![GameField::Title],
                    }
                } else {
                    GameEditOutcome::Rejected {
                        state,
                        errors: std::mem::take(&mut script.refuse),
                    }
                }
            }))
        }),
        artwork: Rc::new(|_| answer(edit(|script| Some(script.slots.clone())))),
        set_artwork: Rc::new(|_, kind, change| {
            answer(edit(|script| {
                let what = match change {
                    ArtworkChange::Choose { artwork_id } => format!("choose {artwork_id}"),
                    ArtworkChange::Import { path } => format!("import {}", path.display()),
                    ArtworkChange::Remove => "remove".to_owned(),
                    ArtworkChange::Reset => "reset".to_owned(),
                };
                script.artwork.push((kind, what));
                script.slots.clone()
            }))
        }),
        actions: Rc::new(|_| answer(edit(|script| script.releases.clone()))),
        save_action: Rc::new(|_, action| {
            answer(edit(|script| {
                let mut action = action;
                if action.id.is_empty() {
                    action.id = format!("added-{}", script.saved_actions.len());
                }
                script.saved_actions.push(action.clone());
                if let Some(release) = script
                    .releases
                    .iter_mut()
                    .find(|release| release.entry_id == action.entry_id)
                {
                    release.actions.retain(|known| known.id != action.id);
                    release.actions.push(action.clone());
                }
                action
            }))
        }),
        delete_action: Rc::new(|_, id| {
            answer(edit(|script| {
                script.deleted.push(id);
                true
            }))
        }),
        order_actions: Rc::new(|_, _, _| answer(())),
        action_settings: Rc::new(|_, _, settings| answer(settings)),
        proton: Rc::new(|| answer(Default::default())),
        saves: Rc::new(|_| {
            answer(edit(|script| {
                script.locations.clone().unwrap_or_else(|| SaveLocations {
                    revision: 0,
                    preferences: SavePreferences::default(),
                    matched: SaveMatch::Unknown,
                    automatic: Vec::new(),
                    locations: Vec::new(),
                    warnings: Vec::new(),
                })
            }))
        }),
        edit_saves: Rc::new(|_, preferences, revision| {
            answer(edit(|script| {
                script.location_edits.push((preferences.clone(), revision));
                let mut locations = script.locations.clone().expect("scripted save locations");
                locations.preferences = preferences;
                locations.revision += 1;
                script.locations = Some(locations.clone());
                locations
            }))
        }),
    }
}

fn state() -> GameEditState {
    let mut game = Game::new("Fixture Game 0002");
    game.id = "fixture-1".to_owned();
    game.tags = vec!["Fixture".to_owned()];
    let automatic = game.clone();
    GameEditState {
        game,
        automatic,
        manual: Vec::new(),
    }
}

/// `fixture-1`'s page with its editor open from Edit details.
fn open_editor() -> Window {
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
    window
}

fn editor_text(window: &Window) -> String {
    window.text(window.get("data-cs", "game-editor"))
}

/// Types `text` over field `field`'s value.
fn retype(window: &mut Window, field: &str, text: &str) {
    let frame = window.get("data-field", field);
    let input = window
        .find_in(frame, "data-part", "input")
        .expect("the field's input");
    window.focus(input);
    window.key_with(Key::Character("a".into()), Modifiers::CONTROL);
    window.key(Key::Named(NamedKey::Backspace));
    window.type_text(text);
}

fn tab(window: &mut Window, label: &str) {
    let tab = window
        .find_all("role", "tab")
        .into_iter()
        .find(|&tab| window.text(tab).starts_with(label))
        .unwrap_or_else(|| panic!("the {label} tab"));
    window.click(tab);
}

fn button(window: &Window, label: &str) -> blitz_dom::NodeId {
    window
        .find_all("data-spatial", "focusable")
        .into_iter()
        .find(|&node| window.text(node).trim() == label)
        .unwrap_or_else(|| panic!("{label} in {}", editor_text(window)))
}

#[test]
fn edit_details_saves_the_changed_fields_as_one_patch_and_closes() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _script = edit_script(EditScript {
        state: Some(state()),
        ..EditScript::default()
    });
    let mut window = open_editor();
    let said = editor_text(&window);
    for tab in ["Details", "Artwork", "Actions", "Scripts", "Saves"] {
        assert!(said.contains(tab), "{tab} in {said}");
    }
    // A sort title that followed the title keeps following it.
    retype(&mut window, "title", "Renamed");
    window.settle_until("the change marked", |window| {
        window
            .find_all("role", "tab")
            .into_iter()
            .any(|tab| window.attr(tab, "data-dirty").as_deref() == Some("true"))
    });
    let save = window.get("data-part", "save");
    assert_eq!(window.attr(save, "data-disabled"), None);
    // A pad's Start saves, from any control (in a field it ends typing).
    window.focus(save);
    window.pad(Action::Menu);
    window.settle_until("saved and closed", |window| {
        window.find("data-cs", "game-editor").is_none()
    });
    let (patch, base) = edit(|script| script.saves[0].clone());
    assert_eq!(
        patch.title,
        Some(caskade_core::model::TextEdit::Set {
            value: "Renamed".into()
        })
    );
    assert_eq!(
        patch.sort_title,
        Some(caskade_core::model::TextEdit::Set {
            value: "Renamed".into()
        })
    );
    assert_eq!(patch.tags, None, "only changed fields go");
    assert_eq!(
        base.map(|game| game.title).as_deref(),
        Some("Fixture Game 0002")
    );
}

#[test]
fn a_field_saved_elsewhere_meanwhile_is_a_conflict_to_settle() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _script = edit_script(EditScript {
        state: Some(state()),
        refuse: vec![FieldError {
            field: GameField::Title,
            problem: FieldProblem::Conflict,
            message: "changed elsewhere".into(),
        }],
        ..EditScript::default()
    });
    let mut window = open_editor();
    retype(&mut window, "title", "Mine");
    // Meanwhile, another device renamed it.
    edit(|script| {
        if let Some(state) = &mut script.state {
            state.game.title = "Theirs".into();
            state.manual.push(GameField::Title);
        }
    });
    let save = window.get("data-part", "save");
    window.click(save);
    window.settle_until("the conflict", |window| {
        window.find("data-part", "keep-mine").is_some()
    });
    let said = editor_text(&window);
    assert!(
        said.contains("Nothing was saved. Check the marked fields."),
        "{said}"
    );
    let field = window.get("data-field", "title");
    assert_eq!(window.attr(field, "data-invalid").as_deref(), Some("true"));
    // Keep mine: the next save overwrites theirs, from their value.
    let keep = window.get("data-part", "keep-mine");
    window.click(keep);
    window.settle_until("settled", |window| {
        window.find("data-part", "keep-mine").is_none()
    });
    let save = window.get("data-part", "save");
    window.click(save);
    window.settle_until("saved again", |_| edit(|script| script.saves.len()) == 2);
    let (patch, base) = edit(|script| script.saves[1].clone());
    assert_eq!(
        patch.title,
        Some(caskade_core::model::TextEdit::Set {
            value: "Mine".into()
        })
    );
    assert_eq!(base.map(|game| game.title).as_deref(), Some("Theirs"));
}

#[test]
fn closing_with_unsaved_changes_asks_first() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _script = edit_script(EditScript {
        state: Some(state()),
        ..EditScript::default()
    });
    let mut window = open_editor();
    // Nothing changed: Close closes.
    let cancel = window.get("data-part", "cancel");
    assert_eq!(window.text(cancel).trim(), "Close");
    window.click(cancel);
    window.settle_until("closed", |window| {
        window.find("data-cs", "game-editor").is_none()
    });
    let edit_button = window.get("data-part", "edit");
    window.click(edit_button);
    window.settle_until("the editor again", |window| {
        window.find("data-field", "notes").is_some()
    });
    let notes = window.get("data-field", "notes");
    let area = window
        .find_in(notes, "data-part", "input")
        .expect("the notes");
    window.focus(area);
    window.type_text("Great");
    // Escape asks; Keep editing keeps the work.
    window.pad(Action::Back);
    window.settle_until("the question", |window| {
        window.find("data-cs", "game-editor-discard").is_some()
    });
    let keep = window
        .find_all("data-focus-default", "true")
        .into_iter()
        .find(|&node| window.text(node).contains("Keep editing"))
        .expect("Keep editing");
    window.click(keep);
    window.settle_until("still editing", |window| {
        window.find("data-cs", "game-editor-discard").is_none()
            && window.find("data-cs", "game-editor").is_some()
    });
    let cancel = window.get("data-part", "cancel");
    assert_eq!(window.text(cancel).trim(), "Cancel");
    window.click(cancel);
    window.settle_until("the question again", |window| {
        window.find("data-cs", "game-editor-discard").is_some()
    });
    let question = window.get("data-cs", "game-editor-discard");
    assert!(
        window
            .text(question)
            .contains("Your unsaved changes to Fixture Game 0002 will be lost."),
        "{}",
        window.text(question)
    );
    let discard = button(&window, "Discard");
    window.click(discard);
    window.settle_until("closed", |window| {
        window.find("data-cs", "game-editor").is_none()
    });
    assert!(edit(|script| script.saves.is_empty()));
}

#[test]
fn the_games_menu_offers_edit_which_opens_the_editor() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _script = edit_script(EditScript {
        state: Some(state()),
        ..EditScript::default()
    });
    let mut window = Window::open();
    window.go("/games/fixture-1");
    window.settle_until("the page", |window| {
        window.find("data-cs", "hero").is_some()
    });
    let hero = window.get("data-cs", "hero");
    window.right_click(hero);
    window.settle_until("the menu", |window| window.find("role", "menu").is_some());
    let item = window
        .find_all("role", "menuitem")
        .into_iter()
        .find(|&item| window.text(item).contains("Edit…"))
        .expect("Edit…");
    window.click(item);
    window.settle_until("the editor", |window| {
        window.find("data-cs", "game-editor").is_some()
    });
    assert!(editor_text(&window).contains("Edit Fixture Game 0002"));
}

fn art(kind: ArtworkKind, id: &str, source: &str) -> Artwork {
    let mut art = Artwork::new("fixture-1", kind, id, source, "", 600, 900);
    art.id = id.to_owned();
    art
}

#[test]
fn artwork_shows_each_kinds_choice_and_changes_it_at_once() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let cover = art(ArtworkKind::Cover, "cover-1", "igdb");
    let other = art(ArtworkKind::Cover, "cover-2", "steam");
    let _script = edit_script(EditScript {
        state: Some(state()),
        slots: vec![
            ArtworkSlot {
                kind: ArtworkKind::Cover,
                choice: ArtworkChoice::Automatic,
                shown: Some(cover.clone()),
                automatic: vec![cover, other],
            },
            ArtworkSlot {
                kind: ArtworkKind::Hero,
                choice: ArtworkChoice::Removed,
                shown: None,
                automatic: Vec::new(),
            },
        ],
        ..EditScript::default()
    });
    let mut window = open_editor();
    tab(&mut window, "Artwork");
    window.settle_until("the slots", |window| {
        window.find("data-part", "slot").is_some()
    });
    let slots = window.find_all("data-part", "slot");
    assert_eq!(
        slots
            .iter()
            .map(|&slot| window.attr(slot, "data-choice").unwrap_or_default())
            .collect::<Vec<_>>(),
        ["automatic", "removed"]
    );
    let candidates = window.find_all("data-part", "candidate");
    assert_eq!(
        window.attr(candidates[0], "data-selected").as_deref(),
        Some("true")
    );
    assert_eq!(window.attr(candidates[1], "data-selected"), None);
    // A hidden kind offers Reset, not Show none.
    assert!(window.find_in(slots[1], "data-part", "clear").is_none());
    assert!(window.find_in(slots[1], "data-part", "reset").is_some());
    window.click(candidates[1]);
    window.settle_until("chosen", |_| edit(|script| script.artwork.len()) == 1);
    let clear = window.find_in(slots[0], "data-part", "clear").unwrap();
    window.click(clear);
    window.settle_until("removed", |_| edit(|script| script.artwork.len()) == 2);
    assert_eq!(
        edit(|script| script.artwork.clone()),
        [
            (ArtworkKind::Cover, "choose cover-2".to_owned()),
            (ArtworkKind::Cover, "remove".to_owned())
        ]
    );
}

fn release() -> ReleaseActions {
    ReleaseActions {
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
                env: BTreeMap::new(),
            },
            tracking: ActionTracking::default(),
            proton: None,
        }],
        settings: EntryActionSettings::default(),
    }
}

#[test]
fn actions_add_a_play_action_that_saves_on_its_own() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _script = edit_script(EditScript {
        state: Some(state()),
        releases: vec![release()],
        ..EditScript::default()
    });
    let mut window = open_editor();
    tab(&mut window, "Actions");
    window.settle_until("the actions", |window| {
        window.find("data-cs", "game-actions").is_some()
    });
    let actions = window.get("data-cs", "game-actions");
    // The release's own launch comes from its source: locked.
    assert!(window.find_in(actions, "data-part", "locked").is_some());
    let list = window
        .find_in(actions, "data-part", "actions")
        .expect("the actions");
    assert_eq!(window.text(list).trim(), "Play");
    let add = button(&window, "Add play action");
    window.click(add);
    window.settle_until("the new action", |window| {
        window.find("data-part", "locked").is_none()
    });
    // Unsaved: its tab says so, and Close asks.
    let dirty = window
        .find_all("role", "tab")
        .into_iter()
        .find(|&tab| window.text(tab).starts_with("Actions"))
        .unwrap();
    window.settle_until("the tab marked", |window| {
        window.attr(dirty, "data-dirty").as_deref() == Some("true")
    });
    let save = button(&window, "Save action");
    window.click(save);
    window.settle_until("saved", |_| edit(|script| script.saved_actions.len()) == 1);
    let saved = edit(|script| script.saved_actions[0].clone());
    assert_eq!(saved.name, "Play (modded)");
    assert_eq!(saved.kind, ActionKind::Play);
    assert_eq!(saved.entry_id, "entry-1");
}

#[test]
fn saves_change_a_releases_locations_from_their_revision() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let folder = LocationSpec {
        kind: LocationKind::Folder,
        path: "/saves/fixture".to_owned(),
    };
    let _script = edit_script(EditScript {
        state: Some(state()),
        locations: Some(SaveLocations {
            revision: 4,
            preferences: SavePreferences::default(),
            matched: SaveMatch::Unknown,
            automatic: vec![SaveLocationRow {
                spec: folder,
                files: 3,
            }],
            locations: Vec::new(),
            warnings: Vec::new(),
        }),
        ..EditScript::default()
    });
    let mut window = open_editor();
    tab(&mut window, "Saves");
    window.settle_until("the locations", |window| {
        window.find("data-part", "location").is_some()
    });
    let saves = window.get("data-cs", "game-saves");
    assert_eq!(
        window.attr(saves, "data-choice").as_deref(),
        Some("automatic")
    );
    let location = window.get("data-part", "location");
    let said = window.text(location);
    assert!(
        said.contains("Folder · 3 save files") && said.contains("/saves/fixture"),
        "{said}"
    );
    let remove = window
        .find_in(location, "data-spatial", "focusable")
        .expect("Remove");
    window.click(remove);
    window.settle_until("chosen locations", |window| {
        window
            .attr(window.get("data-cs", "game-saves"), "data-choice")
            .as_deref()
            == Some("chosen")
    });
    let save = button(&window, "Save locations");
    window.click(save);
    window.settle_until("saved", |_| edit(|script| script.location_edits.len()) == 1);
    let (preferences, revision) = edit(|script| script.location_edits[0].clone());
    assert_eq!(preferences.locations, Some(Vec::new()));
    assert_eq!(revision, 4);
    window.settle_until("said", |window| {
        window
            .text(window.get("data-cs", "game-saves"))
            .contains("Save locations saved.")
    });
}
