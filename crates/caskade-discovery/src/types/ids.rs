use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{borrow::Cow, fmt, str::FromStr};
use thiserror::Error;

/// Identifies a source: `"steam"`, `"heroic.gog"`, `"windows.uninstall"`, …
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema, Type,
)]
#[serde(transparent)]
pub struct SourceId(Cow<'static, str>);

impl SourceId {
    pub const fn new(id: &'static str) -> Self {
        Self(Cow::Borrowed(id))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for SourceId {
    fn from(id: String) -> Self {
        Self(Cow::Owned(id))
    }
}

impl fmt::Display for SourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A store or launcher identifier for a game. `Display` and `FromStr` give a
/// stable string form (`steam:620`, `gog:1207658924`,
/// `epic:<namespace>:<catalog item>:<app name>`, `xbox:<pfn>`, …) for
/// `GameEntry.external_id`. Within a component, `%` and `:` are written as
/// `%25` and `%3A`; numbers have no sign or leading zeros.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema, Type,
)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ExternalId {
    Steam {
        app_id: u32,
    },
    /// A non-Steam shortcut's computed app id.
    SteamShortcut {
        app_id: u32,
    },
    Gog {
        #[serde(with = "decimal")]
        #[specta(type = String)]
        #[schemars(with = "String")]
        product_id: u64,
    },
    Epic {
        namespace: String,
        catalog_item: String,
        app_name: String,
    },
    Itch {
        #[serde(with = "decimal")]
        #[specta(type = String)]
        #[schemars(with = "String")]
        game_id: u64,
    },
    Amazon {
        id: String,
    },
    Ubisoft {
        #[serde(with = "decimal")]
        #[specta(type = String)]
        #[schemars(with = "String")]
        id: u64,
    },
    BattleNet {
        code: String,
    },
    Ea {
        offer: String,
    },
    Xbox {
        pfn: String,
    },
    /// A Microsoft Store product id (e.g. `9NBLGGH4R315`).
    MicrosoftStore {
        store_id: String,
    },
    Riot {
        product: String,
    },
    Rockstar {
        title: String,
    },
    Lutris {
        slug: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("invalid external id {input:?}: {reason}")]
pub struct ExternalIdError {
    pub input: String,
    pub reason: &'static str,
}

impl ExternalId {
    /// The string-form prefix, e.g. `steam` or `battle_net`.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Steam { .. } => "steam",
            Self::SteamShortcut { .. } => "steam_shortcut",
            Self::Gog { .. } => "gog",
            Self::Epic { .. } => "epic",
            Self::Itch { .. } => "itch",
            Self::Amazon { .. } => "amazon",
            Self::Ubisoft { .. } => "ubisoft",
            Self::BattleNet { .. } => "battle_net",
            Self::Ea { .. } => "ea",
            Self::Xbox { .. } => "xbox",
            Self::MicrosoftStore { .. } => "microsoft_store",
            Self::Riot { .. } => "riot",
            Self::Rockstar { .. } => "rockstar",
            Self::Lutris { .. } => "lutris",
        }
    }
}

impl fmt::Display for ExternalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.kind())?;
        let mut part = |text: &str| -> fmt::Result {
            f.write_str(":")?;
            for char in text.chars() {
                match char {
                    '%' => f.write_str("%25")?,
                    ':' => f.write_str("%3A")?,
                    char => fmt::Write::write_char(f, char)?,
                }
            }
            Ok(())
        };
        match self {
            Self::Steam { app_id } | Self::SteamShortcut { app_id } => part(&app_id.to_string()),
            Self::Gog { product_id: id } | Self::Itch { game_id: id } | Self::Ubisoft { id } => {
                part(&id.to_string())
            }
            Self::Epic {
                namespace,
                catalog_item,
                app_name,
            } => {
                part(namespace)?;
                part(catalog_item)?;
                part(app_name)
            }
            Self::Amazon { id: text }
            | Self::BattleNet { code: text }
            | Self::Ea { offer: text }
            | Self::Xbox { pfn: text }
            | Self::MicrosoftStore { store_id: text }
            | Self::Riot { product: text }
            | Self::Rockstar { title: text }
            | Self::Lutris { slug: text } => part(text),
        }
    }
}

impl FromStr for ExternalId {
    type Err = ExternalIdError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let error = |reason| ExternalIdError {
            input: input.into(),
            reason,
        };
        let mut parts = input.split(':');
        let kind = parts.next().unwrap_or_default();
        let parts = parts
            .map(unescape)
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| error("invalid escape"))?;
        let one = || match parts.as_slice() {
            [part] => Ok(part.clone()),
            _ => Err(error("expected one component")),
        };
        let number = || {
            let part = one()?;
            let canonical = !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (part == "0" || !part.starts_with('0'));
            if canonical {
                part.parse::<u64>()
                    .map_err(|_| error("number out of range"))
            } else {
                Err(error("expected a number"))
            }
        };
        let app_id = || u32::try_from(number()?).map_err(|_| error("number out of range"));
        Ok(match kind {
            "steam" => Self::Steam { app_id: app_id()? },
            "steam_shortcut" => Self::SteamShortcut { app_id: app_id()? },
            "gog" => Self::Gog {
                product_id: number()?,
            },
            "epic" => match parts.as_slice() {
                [namespace, catalog_item, app_name] => Self::Epic {
                    namespace: namespace.clone(),
                    catalog_item: catalog_item.clone(),
                    app_name: app_name.clone(),
                },
                _ => return Err(error("expected three components")),
            },
            "itch" => Self::Itch { game_id: number()? },
            "amazon" => Self::Amazon { id: one()? },
            "ubisoft" => Self::Ubisoft { id: number()? },
            "battle_net" => Self::BattleNet { code: one()? },
            "ea" => Self::Ea { offer: one()? },
            "xbox" => Self::Xbox { pfn: one()? },
            "microsoft_store" => Self::MicrosoftStore { store_id: one()? },
            "riot" => Self::Riot { product: one()? },
            "rockstar" => Self::Rockstar { title: one()? },
            "lutris" => Self::Lutris { slug: one()? },
            _ => return Err(error("unknown kind")),
        })
    }
}

/// Reverses the component escaping; only the canonical `%25` and `%3A` are
/// accepted, so every string form has exactly one spelling.
fn unescape(part: &str) -> Option<String> {
    let mut out = String::with_capacity(part.len());
    let mut rest = part;
    while let Some(index) = rest.find('%') {
        out.push_str(&rest[..index]);
        let escape = rest.get(index..index + 3)?;
        out.push(match escape {
            "%25" => '%',
            "%3A" => ':',
            _ => return None,
        });
        rest = &rest[index + 3..];
    }
    out.push_str(rest);
    Some(out)
}

/// 64-bit store ids cross JSON (and the UI's JavaScript numbers) as decimal
/// strings, so no id ever loses precision. Numbers are still accepted.
mod decimal {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(value)
    }

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Either {
        Text(String),
        Number(u64),
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
        match Either::deserialize(deserializer)? {
            Either::Number(value) => Ok(value),
            Either::Text(text) => text.parse().map_err(D::Error::custom),
        }
    }
}
