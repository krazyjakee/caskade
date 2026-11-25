//! Nothing is shared without an explicit table/key AND field declaration.
//! These are portable payloads, not SQL rows ready to insert on another
//! machine: the change exchange must map identities and honor `receive`.

mod project;
mod rules;

pub use project::*;
pub use rules::*;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::change::Entity;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum Category {
    Library,
    Artwork,
    AchievementLinks,
    PlaySessions,
    Saves,
    Accounts,
    Appearance,
    RommServers,
    LaunchActions,
    GameScripts,
    PreferredEmulators,
    Addons,
    EmulatorConfig,
}

impl Category {
    pub const ALL: [Self; 13] = [
        Self::Library,
        Self::Artwork,
        Self::AchievementLinks,
        Self::PlaySessions,
        Self::Saves,
        Self::Accounts,
        Self::Appearance,
        Self::RommServers,
        Self::LaunchActions,
        Self::GameScripts,
        Self::PreferredEmulators,
        Self::Addons,
        Self::EmulatorConfig,
    ];

    pub const fn disposition(self) -> Disposition {
        match self {
            Self::LaunchActions
            | Self::GameScripts
            | Self::PreferredEmulators
            | Self::Addons
            | Self::EmulatorConfig => Disposition::Optional,
            Self::Library
            | Self::Artwork
            | Self::AchievementLinks
            | Self::PlaySessions
            | Self::Saves
            | Self::Accounts
            | Self::Appearance
            | Self::RommServers => Disposition::Synced,
        }
    }

    pub const fn default_state(self) -> CategoryState {
        match self.disposition() {
            Disposition::Synced => CategoryState::Sync,
            _ => CategoryState::ThisDeviceOnly,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    Synced,
    Optional,
    Local,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum CategoryState {
    #[schemars(title = "Sync")]
    Sync,
    #[schemars(title = "This device only")]
    ThisDeviceOnly,
}

/// `core.sync.categories`: each device decides what it sends AND applies.
/// Missing categories use policy defaults; an optional category never turns
/// itself on when a new version adds it. These choices themselves stay local.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SyncSettings {
    #[schemars(title = "What syncs")]
    /// Choose Sync or This device only for each category.
    pub categories: BTreeMap<Category, CategoryState>,
    #[schemars(title = "Never use relays")]
    /// Connect to other devices only directly. A device only a relay could reach shows as offline.
    #[serde(default)]
    pub never_relay: bool,
    #[schemars(title = "Listening port", range(min = 1, max = 65535))]
    /// A fixed UDP port for firewall rules; empty lets the system choose.
    #[serde(default)]
    pub port: Option<u16>,
    #[schemars(title = "Downloads from my devices")]
    /// Get a download from another of your devices on this network when it has the same file, before the internet. Game files never go through a relay.
    #[serde(default = "peer_downloads")]
    pub peer_downloads: bool,
    #[schemars(title = "Settings kept on this device")]
    /// Settings sections that stay on this device while their category syncs.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sections: BTreeMap<String, CategoryState>,
    #[schemars(title = "Accept copies from my devices")]
    /// A game another of your devices sends here is copied into the default storage without asking.
    #[serde(default)]
    pub accept_copies: bool,
    #[schemars(title = "Play requests from my devices")]
    /// Your other devices can start a game here, as Play here would.
    #[serde(default = "allowed")]
    pub remote_play: bool,
}

fn allowed() -> bool {
    true
}

fn peer_downloads() -> bool {
    true
}

/// Settings sections that can be This device only on their own (#244),
/// with the entity that carries each and its category.
pub const SECTIONS: [(&str, Entity, Category); 3] = [
    ("core.appearance", Entity::Appearance, Category::Appearance),
    ("core.accounts", Entity::AccountSettings, Category::Accounts),
    ("core.romm", Entity::RommServer, Category::RommServers),
];

impl Default for SyncSettings {
    fn default() -> Self {
        Self {
            categories: Category::ALL
                .into_iter()
                .map(|category| (category, category.default_state()))
                .collect(),
            never_relay: false,
            port: None,
            peer_downloads: true,
            sections: BTreeMap::new(),
            accept_copies: false,
            remote_play: true,
        }
    }
}

impl SyncSettings {
    /// The settings section these choices live in.
    pub const NAMESPACE: &str = "core.sync";

    /// The saved choices; the defaults without `settings` or when they're
    /// unreadable.
    pub async fn read(settings: Option<&crate::settings::SettingsService>) -> Self {
        match settings {
            Some(settings) => settings.read_or_default(Self::NAMESPACE).await,
            None => Self::default(),
        }
    }

    pub fn state(&self, category: Category) -> CategoryState {
        self.categories
            .get(&category)
            .copied()
            .unwrap_or_else(|| category.default_state())
    }

    /// Both directions use the same gate; transport implementation is #228/#230.
    pub fn allows(&self, category: Category) -> bool {
        self.state(category) == CategoryState::Sync
    }

    /// A settings section's own choice (#244): Sync unless it was made This
    /// device only; an unknown section always syncs with its category.
    pub fn section(&self, namespace: &str) -> CategoryState {
        match self.sections.get(namespace) {
            Some(CategoryState::ThisDeviceOnly)
                if SECTIONS.iter().any(|(name, _, _)| *name == namespace) =>
            {
                CategoryState::ThisDeviceOnly
            }
            _ => CategoryState::Sync,
        }
    }

    /// Whether `entity`'s changes are sent and taken in: its category syncs
    /// and, for a settings section, the section isn't This device only.
    pub fn allows_entity(&self, entity: Entity) -> bool {
        self.allows(entity.category())
            && SECTIONS
                .iter()
                .filter(|(_, carrier, _)| *carrier == entity)
                .all(|(name, _, _)| self.section(name) == CategoryState::Sync)
    }

    /// Whether all of `category` syncs: on, with none of its sections kept
    /// here. A section turned back on turns its category "on" again here,
    /// so it's sent to each device in full.
    pub fn allows_whole(&self, category: Category) -> bool {
        self.allows(category)
            && SECTIONS
                .iter()
                .filter(|(_, _, owner)| *owner == category)
                .all(|(name, _, _)| self.section(name) == CategoryState::Sync)
    }
}
