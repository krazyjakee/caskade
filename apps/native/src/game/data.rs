//! What a game's page reads and changes through the app's services, which
//! fixtures mode doesn't have: its installs' updates, its releases' launch
//! actions, separating a release. [`PageData`] is the running app's by
//! default; a test provides one of its own (`use_context_provider`).

use super::launch::Answer;
use crate::data::change;
use caskade_core::error::ErrorReport;
use caskade_core::launch::ReleaseActions;
use caskade_core::model::EntryActionSettings;
use caskade_core::updates::GameUpdate;
use dioxus_native::prelude::*;
use std::rc::Rc;

/// A change to an install's updates.
#[derive(Debug, Clone, PartialEq)]
pub enum UpdateChange {
    /// Stop offering this release; `None` offers every release again.
    Skip(Option<String>),
    /// Ask the provider for its newest release now.
    Check,
    /// Offer prereleases too, or stop.
    Prereleases(bool),
}

#[derive(Clone)]
pub struct PageData {
    /// The updates of game `0`'s followed installs.
    pub updates: Rc<dyn Fn(String) -> Answer<Vec<GameUpdate>>>,
    /// Changes entry `0`'s updates.
    pub change_update: Rc<dyn Fn(String, UpdateChange) -> Answer<GameUpdate>>,
    /// Installs entry `0`'s newest release: the job's id.
    pub install_update: Rc<dyn Fn(String) -> Answer<String>>,
    /// Game `0`'s releases with their launch actions.
    pub actions: Rc<dyn Fn(String) -> Answer<Vec<ReleaseActions>>>,
    /// Saves release `1`'s (of game `0`) action settings.
    pub save_action_settings:
        Rc<dyn Fn(String, String, EntryActionSettings) -> Answer<EntryActionSettings>>,
    /// Moves entry `0` out into a game of its own: that game's id.
    pub separate: Rc<dyn Fn(String) -> Answer<String>>,
}

impl PartialEq for PageData {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.updates, &other.updates)
    }
}

fn unavailable() -> ErrorReport {
    ErrorReport::from(caskade_ui_i18n::t!(
        "That isn't available in this copy of Caskade."
    ))
}

fn services() -> Result<&'static caskade_app::start::Services, ErrorReport> {
    crate::shell::services().ok_or_else(unavailable)
}

impl PageData {
    /// The running app's; without its services, nothing is followed, no
    /// release has actions, and changes say they aren't available.
    pub fn of_app() -> Self {
        Self {
            updates: Rc::new(|game_id| {
                Box::pin(async move {
                    let Some(updates) =
                        crate::shell::services().and_then(|services| services.updates.as_ref())
                    else {
                        return Ok(Vec::new());
                    };
                    change(async move {
                        caskade_app::commands::updates::game_updates(updates, &game_id).await
                    })
                    .await
                })
            }),
            change_update: Rc::new(|entry_id, update| {
                Box::pin(async move {
                    let updates = services()?.updates.as_ref().ok_or_else(unavailable)?;
                    change(async move {
                        use caskade_app::commands::updates as commands;
                        match update {
                            UpdateChange::Skip(version) => {
                                commands::game_update_skip(updates, &entry_id, version).await
                            }
                            UpdateChange::Check => {
                                commands::game_update_check(updates, &entry_id).await
                            }
                            UpdateChange::Prereleases(on) => {
                                commands::game_update_prereleases(updates, &entry_id, on).await
                            }
                        }
                    })
                    .await
                })
            }),
            install_update: Rc::new(|entry_id| {
                Box::pin(async move {
                    let services = services()?;
                    let updates = services.updates.as_ref().ok_or_else(unavailable)?;
                    change(async move {
                        caskade_app::commands::updates::game_update_install(
                            updates,
                            &services.launch,
                            &entry_id,
                        )
                        .await
                        .map(|job| job.id)
                    })
                    .await
                })
            }),
            actions: Rc::new(|game_id| {
                Box::pin(async move {
                    let Some(services) = crate::shell::services() else {
                        return Ok(Vec::new());
                    };
                    change(async move {
                        caskade_app::commands::actions::game_actions(&services.store, &game_id)
                            .await
                    })
                    .await
                })
            }),
            save_action_settings: Rc::new(|game_id, entry_id, settings| {
                Box::pin(async move {
                    let services = services()?;
                    change(async move {
                        caskade_app::commands::actions::game_action_settings_save(
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
            separate: Rc::new(|entry_id| {
                Box::pin(async move {
                    let services = services()?;
                    change(async move {
                        caskade_app::commands::igdb::game_separate_entry(
                            &services.metadata,
                            &entry_id,
                        )
                        .await
                        .map_err(ErrorReport::from)
                    })
                    .await
                })
            }),
        }
    }
}

/// The page's data: a test's, else the app's.
pub fn use_page_data() -> PageData {
    let provided = try_use_context::<PageData>();
    use_hook(move || provided.unwrap_or_else(PageData::of_app))
}

/// Loads `load` (one of [`PageData`]'s answers) and again whenever an app
/// event makes `key` stale, as `crate::data::use_query` does for the
/// library's reads.
pub fn use_answer<T: 'static>(
    key: caskade_ui_data::Key,
    load: impl FnMut() -> Answer<T> + 'static,
) -> crate::data::Query<T> {
    let mut resource = use_resource(load);
    let events = use_context::<crate::data::DataSource>().events.clone();
    use_hook(move || {
        let mut events = events.subscribe();
        spawn(async move {
            use tokio::sync::broadcast::error::RecvError;
            loop {
                match events.recv().await {
                    Ok(event) if caskade_ui_data::Changed::from_event(&event).affects(&key) => {
                        resource.restart()
                    }
                    Ok(_) => {}
                    Err(RecvError::Lagged(_)) => resource.restart(),
                    Err(RecvError::Closed) => break,
                }
            }
        });
    });
    resource
}
