//! What the game editor reads and changes through the app's services:
//! the game's details, artwork, launch actions and save locations.
//! [`EditData`] is the running app's; a test provides one of its own
//! (`use_context_provider`). Fixtures mode has neither, so it has no
//! editor.

use crate::data::change;
use crate::game::launch::Answer;
use caskade_app::commands::game_edit::ArtworkChange;
use caskade_core::error::ErrorReport;
use caskade_core::install::ProtonBuilds;
use caskade_core::launch::ReleaseActions;
use caskade_core::model::{
    ArtworkKind, ArtworkSlot, EntryActionSettings, Game, GameAction, GameEdit, GameEditOutcome,
    GameEditState,
};
use caskade_core::saves::{LocationSpec, SaveMatch, SavePreferences, SaveState, Saves};
use dioxus_native::prelude::*;
use std::rc::Rc;

/// A save location and how many files the core found there.
#[derive(Debug, Clone, PartialEq)]
pub struct SaveLocationRow {
    pub spec: LocationSpec,
    pub files: usize,
}

/// One release's save locations on this device.
#[derive(Debug, Clone, PartialEq)]
pub struct SaveLocations {
    /// The edit's starting point: a change made elsewhere since is refused.
    pub revision: u32,
    pub preferences: SavePreferences,
    pub matched: SaveMatch,
    pub automatic: Vec<SaveLocationRow>,
    pub locations: Vec<SaveLocationRow>,
    pub warnings: Vec<String>,
}

impl From<SaveState> for SaveLocations {
    fn from(state: SaveState) -> Self {
        let rows = |list: Vec<caskade_core::saves::SaveLocation>| {
            list.into_iter()
                .map(|location| SaveLocationRow {
                    spec: location.spec,
                    files: location.files.len(),
                })
                .collect()
        };
        Self {
            revision: state.revision,
            preferences: state.preferences,
            matched: state.matched,
            automatic: rows(state.automatic),
            locations: rows(state.locations),
            warnings: state.warnings,
        }
    }
}

#[derive(Clone)]
pub struct EditData {
    /// Game `0` as the editor shows it; `None` once it's gone.
    pub state: Rc<dyn Fn(String) -> Answer<Option<GameEditState>>>,
    /// Saves a patch to game `0`, refusing fields changed since `2`.
    pub save: Rc<dyn Fn(String, GameEdit, Option<Game>) -> Answer<GameEditOutcome>>,
    /// Game `0`'s cover, background and icon; `None` once it's gone.
    pub artwork: Rc<dyn Fn(String) -> Answer<Option<Vec<ArtworkSlot>>>>,
    /// Changes game `0`'s artwork of kind `1`: its slots after.
    pub set_artwork: Rc<dyn Fn(String, ArtworkKind, ArtworkChange) -> Answer<Vec<ArtworkSlot>>>,
    /// Game `0`'s releases with their launch actions.
    pub actions: Rc<dyn Fn(String) -> Answer<Vec<ReleaseActions>>>,
    /// Adds (an empty id) or changes one of game `0`'s actions.
    pub save_action: Rc<dyn Fn(String, GameAction) -> Answer<GameAction>>,
    /// Deletes game `0`'s action `1`.
    pub delete_action: Rc<dyn Fn(String, String) -> Answer<bool>>,
    /// Puts release `1`'s (of game `0`) added actions in the order of `2`.
    pub order_actions: Rc<dyn Fn(String, String, Vec<String>) -> Answer<()>>,
    /// Saves release `1`'s (of game `0`) choice of action.
    pub action_settings:
        Rc<dyn Fn(String, String, EntryActionSettings) -> Answer<EntryActionSettings>>,
    /// The Proton builds an action can choose.
    pub proton: Rc<dyn Fn() -> Answer<ProtonBuilds>>,
    /// Entry `0`'s save locations.
    pub saves: Rc<dyn Fn(String) -> Answer<SaveLocations>>,
    /// Saves entry `0`'s save locations, from revision `2`.
    pub edit_saves: Rc<dyn Fn(String, SavePreferences, u32) -> Answer<SaveLocations>>,
}

impl PartialEq for EditData {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.state, &other.state)
    }
}

fn services() -> Result<&'static caskade_app::start::Services, ErrorReport> {
    crate::shell::services().ok_or_else(|| {
        ErrorReport::from(caskade_ui_i18n::t!(
            "That isn't available in this copy of Caskade."
        ))
    })
}

fn saves_of(services: &caskade_app::start::Services) -> Saves {
    Saves::new(services.store.clone(), services.ludusavi.clone())
        .with_catalog(services.emulation.catalog())
}

impl EditData {
    /// The running app's.
    pub fn of_app() -> Self {
        use caskade_app::commands::{actions, game_edit};
        Self {
            state: Rc::new(|game_id| {
                Box::pin(async move {
                    let services = services()?;
                    change(
                        async move { game_edit::game_edit_state(&services.store, game_id).await },
                    )
                    .await
                })
            }),
            save: Rc::new(|game_id, edit, base| {
                Box::pin(async move {
                    let services = services()?;
                    change(async move {
                        game_edit::game_edit(&services.store, &services.bus, game_id, edit, base)
                            .await
                    })
                    .await
                })
            }),
            artwork: Rc::new(|game_id| {
                Box::pin(async move {
                    let services = services()?;
                    change(async move { game_edit::game_artwork(&services.store, game_id).await })
                        .await
                })
            }),
            set_artwork: Rc::new(|game_id, kind, artwork| {
                Box::pin(async move {
                    let services = services()?;
                    change(async move {
                        game_edit::game_artwork_set(
                            &services.store,
                            &services.bus,
                            &services.edit_artwork,
                            game_id,
                            kind,
                            artwork,
                        )
                        .await
                    })
                    .await
                })
            }),
            actions: Rc::new(|game_id| {
                Box::pin(async move {
                    let services = services()?;
                    change(async move { actions::game_actions(&services.store, &game_id).await })
                        .await
                })
            }),
            save_action: Rc::new(|game_id, action| {
                Box::pin(async move {
                    let services = services()?;
                    change(async move {
                        actions::game_action_save(&services.store, &services.bus, &game_id, action)
                            .await
                    })
                    .await
                })
            }),
            delete_action: Rc::new(|game_id, id| {
                Box::pin(async move {
                    let services = services()?;
                    change(async move {
                        actions::game_action_delete(&services.store, &services.bus, &game_id, &id)
                            .await
                    })
                    .await
                })
            }),
            order_actions: Rc::new(|game_id, entry_id, ids| {
                Box::pin(async move {
                    let services = services()?;
                    change(async move {
                        actions::game_actions_order(
                            &services.store,
                            &services.bus,
                            &game_id,
                            &entry_id,
                            &ids,
                        )
                        .await
                    })
                    .await
                })
            }),
            action_settings: Rc::new(|game_id, entry_id, settings| {
                Box::pin(async move {
                    let services = services()?;
                    change(async move {
                        actions::game_action_settings_save(
                            &services.store,
                            &services.bus,
                            &game_id,
                            &entry_id,
                            settings,
                        )
                        .await
                    })
                    .await
                })
            }),
            proton: Rc::new(|| {
                Box::pin(async move {
                    let services = services()?;
                    change(async move { actions::proton_builds(&services.settings).await }).await
                })
            }),
            saves: Rc::new(|entry_id| {
                Box::pin(async move {
                    let services = services()?;
                    change(async move {
                        saves_of(services)
                            .state(&entry_id)
                            .await
                            .map_err(|error| ErrorReport::from_error(&error))
                    })
                    .await
                    .map(SaveLocations::from)
                })
            }),
            edit_saves: Rc::new(|entry_id, preferences, revision| {
                Box::pin(async move {
                    let services = services()?;
                    change(async move {
                        saves_of(services)
                            .edit(&entry_id, preferences, revision)
                            .await
                            .map_err(|error| ErrorReport::from_error(&error))
                    })
                    .await
                    .map(SaveLocations::from)
                })
            }),
        }
    }
}

/// A test's editor data, else the running app's; `None` where there's no
/// app to edit through (fixtures mode).
pub fn use_edit_data() -> Option<EditData> {
    let provided = try_use_context::<EditData>();
    use_hook(move || provided.or_else(|| crate::shell::services().map(|_| EditData::of_app())))
}
