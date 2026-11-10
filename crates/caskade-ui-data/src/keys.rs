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
            _ => Self::Nothing,
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
    fn other_events_change_nothing_a_view_reads() {
        let changed = Changed::from_event(&Event::SettingsChanged(
            caskade_app::events::SettingsChanged {
                namespace: "core.appearance".into(),
                version: 1,
            },
        ));
        assert_eq!(changed, Changed::Nothing);
    }
}
