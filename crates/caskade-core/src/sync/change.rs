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
    /// A save version (#239), one immutable record: its game, account,
    /// device, session, time, parents and the hash of its file list. Its
    /// files travel as blobs (`sync::blobs`). Key: version id.
    SaveVersion,
    /// The user's own artwork: an imported image, or an automatic one they
    /// chose, its file following as a blob (#273). Key: game|artwork id.
    Artwork,
    /// A game's shell script (`lifecycle_scripts` of game scope), arriving
    /// turned off (#273). Key: game|script id.
    GameScript,
    /// A launch action whose target can be bound on each device (a Flatpak,
    /// an emulator), by its release (`db::sync_carried`). Key:
    /// game|release key|action id.
    Action,
    /// A release's action choices: `defaultAction`, `ask`, `builtInName`,
    /// `builtInTracking`. Key: game|release key.
    ActionSettings,
    /// A hand-made achievement link of a release. Key: game|release
    /// key|provider.
    AchievementLink,
    /// What one device has installed: a plugin or a catalogue add-on, with
    /// its version and (plugins) whether it's on. An offer to the others;
    /// nothing installs or turns on by itself. Key: device|add-on id.
    Addon,
    /// One device's installed copy of a game (#232): release key, size,
    /// build OS, where it lives and whether it can be copied, never its
    /// path. Key: device|game|entry id.
    Copy,
    /// The emulator definition a platform uses (`definition`), applied only
    /// where that emulator is installed. Key: platform id.
    PreferredEmulator,
    /// A `core.appearance` field the user changed from its default (#273).
    /// Key: the namespace; field: the setting's.
    Appearance,
    /// `core.accounts`' rating board, when not the default. Key: the
    /// namespace; field: the setting's.
    AccountSettings,
}

impl Entity {
    pub const ALL: [Self; 21] = [
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
        Self::SaveVersion,
        Self::Artwork,
        Self::GameScript,
        Self::Action,
        Self::ActionSettings,
        Self::AchievementLink,
        Self::Addon,
        Self::Copy,
        Self::PreferredEmulator,
        Self::Appearance,
        Self::AccountSettings,
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
            Self::SaveVersion => "saveVersion",
            Self::Artwork => "artwork",
            Self::GameScript => "gameScript",
            Self::Action => "action",
            Self::ActionSettings => "actionSettings",
            Self::AchievementLink => "achievementLink",
            Self::Addon => "addon",
            Self::Copy => "copy",
            Self::PreferredEmulator => "preferredEmulator",
            Self::Appearance => "appearance",
            Self::AccountSettings => "accountSettings",
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
            Self::ArtworkChoice | Self::Artwork => Category::Artwork,
            Self::Session => Category::PlaySessions,
            Self::Account | Self::Approval => Category::Accounts,
            Self::RommServer => Category::RommServers,
            Self::SaveVersion => Category::Saves,
            Self::GameScript => Category::GameScripts,
            Self::Action | Self::ActionSettings => Category::LaunchActions,
            Self::AchievementLink => Category::AchievementLinks,
            Self::Addon => Category::Addons,
            Self::Copy => Category::Library,
            Self::PreferredEmulator => Category::PreferredEmulators,
            Self::Appearance => Category::Appearance,
            Self::AccountSettings => Category::Accounts,
        }
    }

    /// Which parts of its key are games, which differ per device.
    pub fn game_parts(self) -> &'static [usize] {
        match self {
            Self::Game
            | Self::KeptApart
            | Self::IgdbMatch
            | Self::ArtworkChoice
            | Self::Artwork
            | Self::GameScript
            | Self::Action
            | Self::ActionSettings
            | Self::AchievementLink => &[0],
            Self::Member | Self::Approval | Self::Copy => &[1],
            Self::Collection
            | Self::Session
            | Self::Account
            | Self::RommServer
            | Self::SaveVersion
            | Self::Addon
            | Self::PreferredEmulator
            | Self::Appearance
            | Self::AccountSettings => &[],
        }
    }

    /// Whether rows exist as such ([`EXISTS`]), so a tombstone deletes one.
    pub fn has_rows(self) -> bool {
        !matches!(
            self,
            Self::Game
                | Self::IgdbMatch
                | Self::ActionSettings
                | Self::PreferredEmulator
                | Self::Appearance
                | Self::AccountSettings
        )
    }

    /// The `policy` rules (tables, settings, resources) it carries.
    pub fn carries(self) -> &'static [&'static str] {
        match self {
            Self::Game => &["games"],
            Self::KeptApart => &["kept_apart"],
            Self::IgdbMatch => &["igdb_matches"],
            Self::ArtworkChoice => &["artwork_choices"],
            Self::Collection => &["collections"],
            Self::Member => &["collection_members"],
            Self::Session => &["play_sessions"],
            Self::Account => &["accounts"],
            Self::Approval => &["account_game_approvals"],
            Self::RommServer => &["core.romm"],
            Self::SaveVersion => &["save_versions"],
            Self::Artwork => &["artwork"],
            Self::GameScript => &["lifecycle_scripts"],
            Self::Action => &["game_actions"],
            Self::ActionSettings => &["entry_action_settings"],
            Self::AchievementLink => &["achievement_links"],
            Self::Addon => &["plugins", "addons.records", "installed_addons"],
            Self::Copy => &["installed_copies"],
            Self::PreferredEmulator => &["preferred_emulators"],
            Self::Appearance => &["core.appearance"],
            Self::AccountSettings => &["core.accounts"],
        }
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

    /// Rules carried as files beside the log rather than as an entity.
    const BLOBS: &[&str] = &["save_blobs"];

    #[test]
    fn every_synced_or_optional_rule_has_an_entity() {
        use super::super::policy::{RESOURCES, SETTINGS, TABLES};
        let carried: Vec<&str> = Entity::ALL
            .into_iter()
            .flat_map(|entity| entity.carries().iter().copied())
            .chain(BLOBS.iter().copied())
            .collect();
        for rule in TABLES.iter().chain(SETTINGS).chain(RESOURCES) {
            let Some(category) = rule.category else {
                continue;
            };
            assert!(
                carried.contains(&rule.name),
                "{} ({category:?}) syncs, and no entity carries it",
                rule.name
            );
            let entities: Vec<Entity> = Entity::ALL
                .into_iter()
                .filter(|entity| entity.carries().contains(&rule.name))
                .collect();
            for entity in entities {
                assert_eq!(entity.category(), category, "{} as {entity:?}", rule.name);
            }
        }
        // Nothing carries a rule that doesn't exist, or one kept local.
        for name in carried {
            let rule = super::super::policy::table(name)
                .or_else(|| super::super::policy::setting(name))
                .or_else(|| super::super::policy::resource(name));
            assert!(rule.is_some_and(|rule| rule.category.is_some()), "{name}");
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
