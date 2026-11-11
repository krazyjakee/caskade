//! What a game's page reads and changes through the app's services, which
//! fixtures mode doesn't have: its installs' updates, its releases' launch
//! actions, separating a release. [`PageData`] is the running app's by
//! default; a test provides one of its own (`use_context_provider`).

use super::launch::Answer;
use crate::data::change;
use caskade_core::db::PlaytimeBreakdown;
use caskade_core::emulation::{ArgsOverride, EmulatorTarget, GameEmulation};
use caskade_core::error::ErrorReport;
use caskade_core::launch::ReleaseActions;
use caskade_core::model::EntryActionSettings;
use caskade_core::store_accounts::OwnedRelease;
use caskade_core::sync::copies::PeerCopy;
use caskade_core::updates::GameUpdate;
use dioxus_native::prelude::*;
use std::rc::Rc;

/// A change to a game's IGDB match.
#[derive(Debug, Clone, PartialEq)]
pub enum IgdbChange {
    /// This IGDB game, or none ("Not on IGDB").
    Set(Option<String>),
    /// Match it again automatically.
    Rematch,
    /// Fetch the IGDB game's details again.
    Refresh,
}

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
    /// Emulated entry `0`'s emulator, discs, BIOS and readiness.
    pub emulation: Rc<dyn Fn(String) -> Answer<GameEmulation>>,
    /// Sets entry `0`'s emulator and arguments.
    pub set_emulation: Rc<dyn Fn(String, EmulatorTarget, ArgsOverride) -> Answer<GameEmulation>>,
    /// Splits a command line into arguments, as a shell would.
    pub split_args: Rc<dyn Fn(String) -> Answer<Vec<String>>>,
    /// Game `0`'s releases its store accounts own.
    pub owned: Rc<dyn Fn(String) -> Answer<Vec<OwnedRelease>>>,
    /// Installs owned entry `0` through its store's launcher.
    pub owned_install: Rc<dyn Fn(String) -> Answer<()>>,
    /// Stops waiting for owned entry `0`'s install.
    pub owned_cancel: Rc<dyn Fn(String) -> Answer<()>>,
    /// Where game `0`'s play time comes from.
    pub playtime: Rc<dyn Fn(String) -> Answer<PlaytimeBreakdown>>,
    /// Game `0`'s copies on paired devices.
    pub copies: Rc<dyn Fn(String) -> Answer<Vec<PeerCopy>>>,
    /// Copies game `0` here from device `1`: an install job.
    pub copy_here: Rc<dyn Fn(String, String) -> Answer<()>>,
    /// Starts game `0` on device `1`, waking it first when it can.
    pub play_on: Rc<dyn Fn(String, String) -> Answer<()>>,
    /// Game `0`'s IGDB match; `None` before one was tried.
    pub igdb: Rc<dyn Fn(String) -> Answer<Option<caskade_core::metadata::GameIgdb>>>,
    /// Changes game `0`'s match: the match after (another game's id when
    /// the game merged into it).
    pub igdb_change:
        Rc<dyn Fn(String, IgdbChange) -> Answer<Option<caskade_core::metadata::GameIgdb>>>,
    /// IGDB games for a title.
    pub igdb_search: Rc<dyn Fn(String) -> Answer<Vec<caskade_core::igdb::IgdbGame>>>,
    /// Game `0`'s age ratings.
    pub age_ratings: Rc<dyn Fn(String) -> Answer<Vec<caskade_core::igdb::IgdbAgeRating>>>,
    /// Plays YouTube video `0` in the video window, titled `1`.
    pub play_video: Rc<dyn Fn(String, String) -> Result<(), String>>,
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
            emulation: Rc::new(|entry_id| {
                Box::pin(async move {
                    let services = services()?;
                    change(async move {
                        caskade_app::commands::emulation::emulation_game(
                            &services.emulation,
                            &entry_id,
                        )
                        .await
                    })
                    .await
                })
            }),
            set_emulation: Rc::new(|entry_id, target, args| {
                Box::pin(async move {
                    let services = services()?;
                    change(async move {
                        caskade_app::commands::emulation::emulation_game_set(
                            &services.emulation,
                            &entry_id,
                            target,
                            args,
                        )
                        .await
                    })
                    .await
                })
            }),
            split_args: Rc::new(|text| {
                Box::pin(async move { caskade_app::commands::emulation::args_split(&text) })
            }),
            owned: Rc::new(|game_id| {
                Box::pin(async move {
                    let Some(services) = crate::shell::services() else {
                        return Ok(Vec::new());
                    };
                    change(async move {
                        caskade_app::commands::store_accounts::game_owned_releases(
                            &services.store_accounts,
                            Some(&services.discovery_controller),
                            &game_id,
                        )
                        .await
                    })
                    .await
                })
            }),
            owned_install: Rc::new(|entry_id| {
                Box::pin(async move {
                    let services = services()?;
                    change(async move {
                        caskade_app::commands::store_accounts::owned_install(
                            &services.store_accounts,
                            Some(&services.discovery_controller),
                            &entry_id,
                        )
                        .await
                        .map(drop)
                    })
                    .await
                })
            }),
            owned_cancel: Rc::new(|entry_id| {
                Box::pin(async move {
                    let services = services()?;
                    caskade_app::commands::store_accounts::owned_install_cancel(
                        &services.store_accounts,
                        &entry_id,
                    );
                    Ok(())
                })
            }),
            playtime: Rc::new(|game_id| {
                Box::pin(async move {
                    let services = services()?;
                    change(async move {
                        caskade_app::commands::store_accounts::game_playtime(
                            &services.store,
                            &game_id,
                        )
                        .await
                    })
                    .await
                })
            }),
            copies: Rc::new(|game_id| {
                Box::pin(async move {
                    let Some(devices) = crate::shell::services()
                        .and_then(caskade_ui_data::devices::OtherDevices::of)
                    else {
                        return Ok(Vec::new());
                    };
                    change(async move { devices.copies(&game_id).await }).await
                })
            }),
            copy_here: Rc::new(|game_id, device_id| {
                Box::pin(async move {
                    let services = services()?;
                    let install = services.install.as_ref().ok_or_else(unavailable)?;
                    change(async move {
                        caskade_app::commands::copies::game_copy_here(
                            install,
                            &services.store,
                            &services.devices,
                            game_id,
                            device_id,
                            None,
                        )
                        .await
                        .map(drop)
                    })
                    .await
                })
            }),
            play_on: Rc::new(|game_id, device_id| {
                Box::pin(async move {
                    let devices = crate::shell::services()
                        .and_then(caskade_ui_data::devices::OtherDevices::of)
                        .ok_or_else(unavailable)?;
                    change(async move { devices.play_on(&game_id, &device_id).await }).await
                })
            }),
            igdb: Rc::new(|game_id| {
                Box::pin(async move {
                    let services = services()?;
                    change(async move {
                        caskade_app::commands::igdb::game_igdb(&services.metadata, &game_id)
                            .await
                            .map_err(ErrorReport::from)
                    })
                    .await
                })
            }),
            igdb_change: Rc::new(|game_id, what| {
                Box::pin(async move {
                    let services = services()?;
                    change(async move {
                        use caskade_app::commands::igdb as commands;
                        let metadata = &services.metadata;
                        match what {
                            IgdbChange::Set(id) => commands::game_igdb_set(metadata, &game_id, id)
                                .await
                                .map(Some),
                            IgdbChange::Rematch => {
                                commands::game_igdb_rematch(metadata, &game_id).await
                            }
                            IgdbChange::Refresh => {
                                commands::game_igdb_refresh(metadata, &game_id).await
                            }
                        }
                        .map_err(ErrorReport::from)
                    })
                    .await
                })
            }),
            igdb_search: Rc::new(|query| {
                Box::pin(async move {
                    let services = services()?;
                    change(async move {
                        caskade_app::commands::igdb::igdb_search(&services.igdb, &query, 12)
                            .await
                            .map_err(ErrorReport::from)
                    })
                    .await
                })
            }),
            age_ratings: Rc::new(|game_id| {
                Box::pin(async move {
                    let Some(services) = crate::shell::services() else {
                        return Ok(Vec::new());
                    };
                    change(async move {
                        caskade_app::commands::igdb::game_age_ratings(&services.store, &game_id)
                            .await
                            .map(|ratings| {
                                ratings.map(|ratings| ratings.ratings).unwrap_or_default()
                            })
                            .map_err(ErrorReport::from)
                    })
                    .await
                })
            }),
            play_video: Rc::new(|video_id, title| {
                caskade_app::commands::video::video_play(
                    crate::shell::video_window(),
                    &video_id,
                    &title,
                    caskade_app::commands::video::VideoPlacement::Anywhere,
                )
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
