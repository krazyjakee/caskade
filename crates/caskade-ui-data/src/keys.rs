//! What a view reads ([`Key`]) and what an app event makes stale
//! ([`Changed`]): a view refetches only when an event changes what it shows.

use caskade_app::events::{Event, LibraryChange};

/// What a view reads.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Key {
    /// Library pages, searches and counts, whatever their filter, sort or
    /// query: a change to any game may move it in or out of one.
    Games,
    /// One game's details.
    Game(String),
    /// The library's totals, its platforms and its title letters.
    Summary,
    /// Play sessions: one game's (`Some`), or the most recent ones (`None`).
    Sessions(Option<String>),
    /// Collections and the games in them.
    Collections,
    /// Install, download and other jobs (Activity).
    Jobs,
    /// The notification feed and its unread count.
    Notifications,
    /// Paired devices and how they're reached.
    Devices,
    /// Whether a new Caskade is available.
    AppUpdate,
    /// Background tasks (scans, metadata) and the app's version.
    Background,
    /// A settings section's value, by namespace (`core.appearance`).
    Settings(String),
    /// The installed themes and the one applied (Appearance).
    Themes,
    /// Library discovery's snapshot: launchers, scans, results to review.
    Discovery,
    /// Store accounts, the stores that can be connected and sign-ins.
    Stores,
    /// The installed plugins and their status.
    Plugins,
}

/// What an app event made stale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Changed {
    /// Nothing a view reads.
    Nothing,
    /// Everything: another account signed in, or events were missed.
    Everything,
    /// These keys; [`Key::Game`] is matched by id, and `all_games` stands
    /// for every game's details (after a scan).
    Keys { keys: Vec<Key>, all_games: bool },
}

impl Changed {
    /// What `event` makes stale.
    pub fn from_event(event: &Event) -> Self {
        match event {
            Event::LibraryChanged(changed) => {
                let mut keys = vec![Key::Games, Key::Summary, Key::Collections];
                let all_games = match (changed.change, &changed.game_id) {
                    // A scan may have touched any game.
                    (LibraryChange::Scanned, _) | (_, None) => true,
                    (_, Some(id)) => {
                        keys.push(Key::Game(id.clone()));
                        false
                    }
                };
                Self::Keys { keys, all_games }
            }
            // A session's start and end change the game's last played and
            // playtime, the recent sort and the totals.
            Event::GameStarted(started) => Self::session(&started.0.game_id),
            Event::GameExited(exited) => Self::session(&exited.0.game_id),
            // The library is the signed-in account's: favourites, hidden
            // games, collections and age limits all follow it.
            Event::AccountChanged(_) => Self::Everything,
            // The details refresh's card and the install requests waiting
            // for the user sit with the jobs (#101).
            Event::JobChanged(_) | Event::GamesRefreshChanged(_) | Event::InstallRequested(_) => {
                Self::only(Key::Jobs)
            }
            Event::NotificationsChanged(_) => Self::only(Key::Notifications),
            Event::DevicesChanged(_) => Self::only(Key::Devices),
            Event::AppUpdateChanged(_) => Self::only(Key::AppUpdate),
            Event::BackgroundChanged(_) => Self::only(Key::Background),
            Event::SettingsChanged(changed) => Self::only(Key::Settings(changed.namespace.clone())),
            Event::DiscoveryChanged(_) => Self::only(Key::Discovery),
            Event::StoreAccountsChanged(_) => Self::only(Key::Stores),
            // A dev theme recompiled, or a plugin bringing themes came or
            // went.
            Event::ThemeReloaded(_) => Self::only(Key::Themes),
            // Themes come with plugins too.
            Event::PluginsChanged(_) | Event::PluginDisabled(_) | Event::PluginReloaded(_) => {
                Self::Keys {
                    keys: vec![Key::Plugins, Key::Themes],
                    all_games: false,
                }
            }
            _ => Self::Nothing,
        }
    }

    fn only(key: Key) -> Self {
        Self::Keys {
            keys: vec![key],
            all_games: false,
        }
    }

    fn session(game_id: &str) -> Self {
        Self::Keys {
            keys: vec![
                Key::Games,
                Key::Summary,
                Key::Game(game_id.to_owned()),
                Key::Sessions(Some(game_id.to_owned())),
                Key::Sessions(None),
            ],
            all_games: false,
        }
    }

    /// Whether a view reading `key` must refetch.
    pub fn affects(&self, key: &Key) -> bool {
        match self {
            Self::Nothing => false,
            Self::Everything => true,
            Self::Keys { keys, all_games } => {
                (*all_games && matches!(key, Key::Game(_))) || keys.contains(key)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use caskade_app::events::{AccountChanged, GameExited, GameStarted, LibraryChanged};
    use caskade_core::bus::GameSession;

    fn library(change: LibraryChange, game: Option<&str>) -> Event {
        Event::LibraryChanged(LibraryChanged {
            change,
            game_id: game.map(str::to_owned),
        })
    }

    fn session(game: &str) -> GameSession {
        GameSession {
            game_id: game.into(),
            entry_id: format!("{game}-entry"),
            session_id: "s1".into(),
            started_at_ms: 0,
            ended_at_ms: None,
            duration_ms: 0,
        }
    }

    #[test]
    fn a_game_update_makes_only_that_game_and_the_lists_stale() {
        let changed = Changed::from_event(&library(LibraryChange::Updated, Some("a")));
        assert!(changed.affects(&Key::Game("a".into())));
        assert!(!changed.affects(&Key::Game("b".into())), "another game");
        assert!(changed.affects(&Key::Games));
        assert!(changed.affects(&Key::Summary));
        assert!(changed.affects(&Key::Collections));
        assert!(!changed.affects(&Key::Sessions(None)), "no session changed");
    }

    #[test]
    fn a_scan_makes_every_game_stale() {
        let changed = Changed::from_event(&library(LibraryChange::Scanned, None));
        assert!(changed.affects(&Key::Game("a".into())));
        assert!(changed.affects(&Key::Game("b".into())));
        assert!(changed.affects(&Key::Games));
        assert!(!changed.affects(&Key::Sessions(Some("a".into()))));
    }

    #[test]
    fn a_session_makes_its_game_and_the_recent_sessions_stale() {
        for event in [
            Event::GameStarted(GameStarted(session("a"))),
            Event::GameExited(GameExited(session("a"))),
        ] {
            let changed = Changed::from_event(&event);
            assert!(changed.affects(&Key::Game("a".into())));
            assert!(changed.affects(&Key::Sessions(Some("a".into()))));
            assert!(changed.affects(&Key::Sessions(None)));
            assert!(!changed.affects(&Key::Sessions(Some("b".into()))));
            assert!(!changed.affects(&Key::Game("b".into())));
            assert!(!changed.affects(&Key::Collections));
        }
    }

    #[test]
    fn another_account_makes_everything_stale() {
        let changed = Changed::from_event(&Event::AccountChanged(AccountChanged {
            active_id: Some("x".into()),
        }));
        assert!(changed.affects(&Key::Game("a".into())));
        assert!(changed.affects(&Key::Collections));
    }

    #[test]
    fn status_events_make_only_their_status_stale() {
        for (event, key) in [
            (
                Event::NotificationsChanged(
                    caskade_app::commands::notifications::NotificationsChanged { unread: 2 },
                ),
                Key::Notifications,
            ),
            (
                Event::BackgroundChanged(
                    caskade_app::events::BackgroundChanged(Default::default()),
                ),
                Key::Background,
            ),
        ] {
            let changed = Changed::from_event(&event);
            assert!(changed.affects(&key), "{key:?}");
            for other in [Key::Games, Key::Jobs, Key::Devices, Key::AppUpdate] {
                assert!(!changed.affects(&other), "{key:?} made {other:?} stale");
            }
        }
    }

    /// A settings change makes its own section stale, and nothing else.
    #[test]
    fn a_settings_change_makes_only_its_section_stale() {
        let changed = Changed::from_event(&Event::SettingsChanged(
            caskade_app::events::SettingsChanged {
                namespace: "core.appearance".into(),
                version: 1,
            },
        ));
        assert!(changed.affects(&Key::Settings("core.appearance".into())));
        for other in [
            Key::Themes,
            Key::Settings("core.general".into()),
            Key::Games,
            Key::Summary,
            Key::Jobs,
        ] {
            assert!(!changed.affects(&other), "{other:?}");
        }
    }

    /// A dev theme recompiling, or plugins changing, makes the theme list
    /// stale.
    #[test]
    fn themes_follow_reloads_and_plugins() {
        for event in [
            Event::ThemeReloaded(caskade_app::commands::themes::ThemeReloaded {
                theme: "dev.mine".into(),
                error: None,
            }),
            Event::PluginsChanged(caskade_app::events::PluginsChanged {
                plugin: "acme".into(),
            }),
        ] {
            let changed = Changed::from_event(&event);
            assert!(changed.affects(&Key::Themes));
            assert!(!changed.affects(&Key::Games));
        }
    }
}
