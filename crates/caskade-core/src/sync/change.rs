//! What a change is: an entity, a key, a field and a value (#228).
//!
//! Entities are the kinds of synced data, each a portable form of what
//! `policy` declares synced. A value of `None` is a tombstone. The field `""`
//! is a row's existence (`true` while it exists), so deleting a row is one
//! tombstone and is final: a field written to a deleted row is ignored.
//!
//! Keys use the ids of the device that made the change. Collections,
//! accounts and play sessions have ids that are the same everywhere; games
//! don't (each device gives its games its own), so keys and values naming a
//! game are mapped to the game here before a change applies (#229).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::clock::{Hlc, Stamp};
use super::policy::Category;

/// The field that holds whether a row exists.
pub const EXISTS: &str = "";

/// Separates the parts of a composite key: a collection and a game, say.
pub const KEY_SEPARATOR: char = '|';

/// A kind of synced data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Entity {
    /// A game's identity, favourite, hidden flag and user values. Key: game.
    Game,
    /// The user kept the game apart from automatic merging. Key: game.
    KeptApart,
    /// A match set by hand: an IGDB id, or `"none"` for "not on IGDB".
    /// Key: game.
    IgdbMatch,
    /// A removed artwork kind. Key: game|kind.
    ArtworkChoice,
    /// Key: collection id.
    Collection,
    /// A game in a manual collection. Key: collection|game.
    Member,
    /// A finished play session, one immutable record. Key: session id.
    Session,
    /// Key: account id.
    Account,
    /// A permanent age exception. Key: account|game.
    Approval,
    /// A RomM server's shared definition (`core.romm`), never its password
    /// or token: each device keeps its own in its keychain. Key: server id.
    RommServer,
}

impl Entity {
    pub const ALL: [Self; 10] = [
        Self::Game,
        Self::KeptApart,
        Self::IgdbMatch,
        Self::ArtworkChoice,
        Self::Collection,
        Self::Member,
        Self::Session,
        Self::Account,
        Self::Approval,
        Self::RommServer,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Game => "game",
            Self::KeptApart => "keptApart",
            Self::IgdbMatch => "igdbMatch",
            Self::ArtworkChoice => "artworkChoice",
            Self::Collection => "collection",
            Self::Member => "member",
            Self::Session => "session",
            Self::Account => "account",
            Self::Approval => "approval",
            Self::RommServer => "rommServer",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|entity| entity.as_str() == text)
    }

    /// The category whose choice (Sync or This device only) governs it.
    pub fn category(self) -> Category {
        match self {
            Self::Game | Self::KeptApart | Self::IgdbMatch | Self::Collection | Self::Member => {
                Category::Library
            }
            Self::ArtworkChoice => Category::Artwork,
            Self::Session => Category::PlaySessions,
            Self::Account | Self::Approval => Category::Accounts,
            Self::RommServer => Category::RommServers,
        }
    }

    /// Which parts of its key are games, which differ per device.
    pub fn game_parts(self) -> &'static [usize] {
        match self {
            Self::Game | Self::KeptApart | Self::IgdbMatch | Self::ArtworkChoice => &[0],
            Self::Member | Self::Approval => &[1],
            Self::Collection | Self::Session | Self::Account | Self::RommServer => &[],
        }
    }

    /// Whether rows exist as such ([`EXISTS`]), so a tombstone deletes one.
    pub fn has_rows(self) -> bool {
        !matches!(self, Self::Game | Self::IgdbMatch)
    }
}

/// One change to one field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Change {
    pub origin: String,
    pub clock: Hlc,
    pub entity: Entity,
    /// In the origin device's ids.
    pub key: String,
    pub field: String,
    /// `None` for a tombstone.
    pub value: Option<Value>,
}

impl Change {
    pub fn stamp(&self) -> Stamp {
        Stamp {
            clock: self.clock,
            origin: self.origin.clone(),
        }
    }
}

/// The parts of a composite key.
pub fn key_parts(key: &str) -> Vec<&str> {
    key.split(KEY_SEPARATOR).collect()
}

/// A composite key from its parts.
pub fn join_key(parts: &[&str]) -> String {
    parts.join(&KEY_SEPARATOR.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entities_round_trip_their_names() {
        for entity in Entity::ALL {
            assert_eq!(Entity::parse(entity.as_str()), Some(entity));
        }
    }

    #[test]
    fn every_entity_belongs_to_a_category_the_policy_syncs() {
        for entity in Entity::ALL {
            assert_ne!(
                entity.category().disposition(),
                super::super::policy::Disposition::Local,
                "{entity:?}"
            );
        }
    }
}
