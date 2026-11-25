//! The page's own data, scripted (`PageData`): updates, launch actions,
//! separating, IGDB, saves, achievements, copies and ports.

use crate::game::data::{PageData, UpdateChange};
use caskade_core::launch::ReleaseActions;
use caskade_core::model::EntryActionSettings;
use caskade_core::updates::GameUpdate;
use std::rc::Rc;
use std::sync::Mutex;

mod igdb;
mod panels;
mod releases;

/// What the page's data answers, and what it was asked.
#[derive(Default)]
pub(crate) struct PageScript {
    pub(crate) ports: Option<caskade_core::ports::GamePorts>,
    pub(crate) ports_installed: Vec<String>,
    pub(crate) achievement_sets: Vec<caskade_achievements::achievements::AchievementSet>,
    pub(crate) achievement_refreshes: usize,
    /// Achievements' pictures by achievement id: their URLs.
    pub(crate) achievement_icons: Vec<(String, String)>,
    /// The achievements whose pictures were asked for.
    pub(crate) icons_asked: Vec<String>,
    pub(crate) saves: Option<caskade_core::saves::versions::SaveHistory>,
    pub(crate) save_paths: Vec<String>,
    pub(crate) restored: Vec<(String, String)>,
    pub(crate) save_chosen: Vec<(String, String)>,
    pub(crate) revealed: Vec<(String, u32)>,
    pub(crate) igdb: Option<caskade_core::metadata::GameIgdb>,
    pub(crate) igdb_changes: Vec<crate::game::data::IgdbChange>,
    pub(crate) igdb_results: Vec<caskade_core::igdb::IgdbGame>,
    pub(crate) searches: Vec<String>,
    pub(crate) ratings: Vec<caskade_core::igdb::IgdbAgeRating>,
    pub(crate) videos_played: Vec<(String, String)>,
    pub(crate) emulations: Vec<caskade_core::emulation::GameEmulation>,
    pub(crate) emulator_set: Vec<(String, caskade_core::emulation::EmulatorTarget)>,
    pub(crate) owned: Vec<caskade_core::store_accounts::OwnedRelease>,
    pub(crate) owned_installs: Vec<String>,
    pub(crate) playtime: Option<caskade_core::db::PlaytimeBreakdown>,
    pub(crate) copies: Vec<caskade_core::sync::copies::PeerCopy>,
    pub(crate) copied: Vec<String>,
    pub(crate) played_on: Vec<String>,
    pub(crate) updates: Vec<GameUpdate>,
    pub(crate) actions: Vec<ReleaseActions>,
    pub(crate) changes: Vec<(String, UpdateChange)>,
    pub(crate) installs: Vec<String>,
    pub(crate) saved: Vec<(String, EntryActionSettings)>,
    pub(crate) separated: Vec<String>,
}

static PAGE: Mutex<Option<PageScript>> = Mutex::new(None);

pub(crate) fn page<T>(f: impl FnOnce(&mut PageScript) -> T) -> T {
    let mut script = caskade_core::lock(&PAGE);
    f(script.get_or_insert_with(PageScript::default))
}

pub(crate) struct PageScripted;

pub(crate) fn page_script(script: PageScript) -> PageScripted {
    *caskade_core::lock(&PAGE) = Some(script);
    PageScripted
}

impl Drop for PageScripted {
    fn drop(&mut self) {
        *caskade_core::lock(&PAGE) = None;
    }
}

fn answer<T: 'static>(value: T) -> crate::game::launch::Answer<T> {
    Box::pin(async move { Ok(value) })
}

/// The page data every test window reads (`frame::tests`' app provides it).
pub(crate) fn fake_data() -> PageData {
    PageData {
        updates: Rc::new(|game_id| {
            answer(page(|script| {
                script
                    .updates
                    .iter()
                    .filter(|update| update.game_id == game_id)
                    .cloned()
                    .collect()
            }))
        }),
        change_update: Rc::new(|entry_id, change| {
            let update = page(|script| {
                script.changes.push((entry_id.clone(), change));
                script
                    .updates
                    .iter()
                    .find(|update| update.entry_id == entry_id)
                    .cloned()
            });
            Box::pin(async move { update.ok_or_else(|| "no such update".to_owned().into()) })
        }),
        install_update: Rc::new(|entry_id| {
            page(|script| script.installs.push(entry_id));
            answer("job-1".to_owned())
        }),
        actions: Rc::new(|_| answer(page(|script| script.actions.clone()))),
        save_action_settings: Rc::new(|_, entry_id, settings| {
            page(|script| script.saved.push((entry_id, settings.clone())));
            answer(settings)
        }),
        separate: Rc::new(|entry_id| {
            page(|script| script.separated.push(entry_id));
            answer("separated".to_owned())
        }),
        emulation: Rc::new(|entry_id| {
            answer(page(|script| {
                script
                    .emulations
                    .iter()
                    .find(|each| each.entry_id == entry_id)
                    .cloned()
                    .unwrap_or_else(|| ready_emulation(&entry_id))
            }))
        }),
        set_emulation: Rc::new(|entry_id, target, _| {
            let emulation = page(|script| {
                script.emulator_set.push((entry_id.clone(), target.clone()));
                ready_emulation(&entry_id)
            });
            answer(emulation)
        }),
        split_args: Rc::new(|text| answer(text.split_whitespace().map(str::to_owned).collect())),
        owned: Rc::new(|_| answer(page(|script| script.owned.clone()))),
        owned_install: Rc::new(|entry_id| {
            page(|script| script.owned_installs.push(entry_id));
            answer(())
        }),
        owned_cancel: Rc::new(|_| answer(())),
        playtime: Rc::new(|_| answer(page(|script| script.playtime.clone().unwrap_or_default()))),
        copies: Rc::new(|_| answer(page(|script| script.copies.clone()))),
        copy_here: Rc::new(|_, device| {
            page(|script| script.copied.push(device));
            answer(())
        }),
        play_on: Rc::new(|_, device| {
            page(|script| script.played_on.push(device));
            answer(())
        }),
        igdb: Rc::new(|_| answer(page(|script| script.igdb.clone()))),
        igdb_change: Rc::new(|_, what| {
            answer(page(|script| {
                script.igdb_changes.push(what);
                script.igdb.clone()
            }))
        }),
        igdb_search: Rc::new(|query| {
            answer(page(|script| {
                script.searches.push(query);
                script.igdb_results.clone()
            }))
        }),
        age_ratings: Rc::new(|_| answer(page(|script| script.ratings.clone()))),
        play_video: Rc::new(|video, title| {
            page(|script| script.videos_played.push((video, title)));
            Ok(())
        }),
        save_history: Rc::new(|_| answer(page(|script| script.saves.clone()))),
        save_locations: Rc::new(|_| answer(page(|script| script.save_paths.clone()))),
        save_restore: Rc::new(|entry, version| {
            page(|script| script.restored.push((entry, version)));
            answer(())
        }),
        save_choose: Rc::new(|entry, version| {
            page(|script| script.save_chosen.push((entry, version)));
            answer(())
        }),
        save_reveal: Rc::new(|entry, location| {
            page(|script| script.revealed.push((entry, location)));
            answer(())
        }),
        accounts: Rc::new(|| answer(vec![("acct-1".to_owned(), "Robin".to_owned())])),
        achievements: Rc::new(|_| answer(page(|script| script.achievement_sets.clone()))),
        achievements_refresh: Rc::new(|_| {
            answer(page(|script| {
                script.achievement_refreshes += 1;
                script.achievement_sets.clone()
            }))
        }),
        achievements_cancel: Rc::new(|_| answer(())),
        achievement_icon: Rc::new(|icon| {
            answer(page(|script| {
                script.icons_asked.push(icon.achievement_id.clone());
                script
                    .achievement_icons
                    .iter()
                    .find(|(id, _)| *id == icon.achievement_id)
                    .map(|(_, url)| url.clone())
            }))
        }),
        ports: Rc::new(|_| answer(page(|script| script.ports.clone()))),
        install_port: Rc::new(|_, project| {
            page(|script| script.ports_installed.push(project));
            answer("job-port".to_owned())
        }),
    }
}

/// An emulated release that can start, with one profile to choose.
pub(crate) fn ready_emulation(entry_id: &str) -> caskade_core::emulation::GameEmulation {
    use caskade_core::emulation::{
        ArgsOverride, CompatibleProfile, EmulatorTarget, GameEmulation, Readiness,
    };
    let profile = CompatibleProfile {
        emulator_id: "retroarch".to_owned(),
        profile_id: "mupen".to_owned(),
        emulator_name: "RetroArch".to_owned(),
        profile_name: "Mupen64Plus".to_owned(),
        available: true,
    };
    GameEmulation {
        entry_id: entry_id.to_owned(),
        platform_id: "nintendo_n64".to_owned(),
        platform: None,
        target: EmulatorTarget::PlatformDefault,
        args: ArgsOverride::Default,
        discs: Vec::new(),
        profile: Some(profile.clone()),
        compatible: vec![profile],
        bios: Vec::new(),
        readiness: Readiness::Ready,
        message: None,
    }
}
