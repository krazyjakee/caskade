use crate::{Step, Strategy};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Match {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(regex(pattern = "^[a-fA-F0-9]{64}$"))]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1, max = 256))]
    pub gog_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1, max = 256))]
    pub app_id: Option<String>,
}
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct PresetOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1, max = 128))]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_shortcuts: Option<bool>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    #[schemars(range(min = 1, max = 1))]
    pub schema_version: u32,
    #[schemars(regex(pattern = "^[a-z0-9][a-z0-9._-]{0,127}$"))]
    pub id: String,
    #[serde(default)]
    #[schemars(length(max = 2048))]
    pub description: String,
    #[serde(rename = "match")]
    pub matches: Match,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strategy: Option<Strategy>,
    #[serde(default)]
    #[schemars(length(max = 256))]
    pub add_steps: Vec<Step>,
    #[serde(default)]
    pub preset_args: PresetOverrides,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disable_alias: Option<bool>,
}
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct InputIdentity {
    pub sha256: Option<String>,
    pub gog_id: Option<String>,
    pub app_id: Option<String>,
}
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ResolvedRecipe {
    pub recipe_ids: Vec<String>,
    pub strategy: Option<Strategy>,
    pub add_steps: Vec<Step>,
    pub preset_args: PresetOverrides,
    /// Consumed by install/launch alias orchestration, never by the executor.
    pub disable_alias: bool,
}
impl Match {
    pub fn accepts(&self, identity: &InputIdentity) -> bool {
        self.sha256.as_ref().is_none_or(|hash| {
            identity
                .sha256
                .as_ref()
                .is_some_and(|value| hash.eq_ignore_ascii_case(value))
        }) && self
            .gog_id
            .as_ref()
            .is_none_or(|id| identity.gog_id.as_ref() == Some(id))
            && self.app_id.as_ref().is_none_or(|id| {
                identity
                    .app_id
                    .as_ref()
                    .is_some_and(|value| id.eq_ignore_ascii_case(value))
            })
    }
}
