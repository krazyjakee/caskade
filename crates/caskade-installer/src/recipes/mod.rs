//! Optional, data-only recipe overrides. Load and hash at the application edge;
//! planning remains pure and every changed candidate still passes host/policy
//! checks. Recipes cannot introduce a new strategy a handler did not propose.
mod apply;
mod load;
mod types;
mod validation;
pub use load::{RecipeSet, identity_from_artifact, identity_from_path};
pub use types::{InputIdentity, Match, PresetOverrides, Recipe, ResolvedRecipe};
pub use validation::{MAX_RECIPE_BYTES, json_schema};
#[derive(Debug, thiserror::Error)]
pub enum RecipeError {
    #[error("recipe I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("recipe TOML is invalid: {0}")]
    Toml(String),
    #[error("recipe JSON Schema validation failed at {instance}: {schema}")]
    Schema { instance: String, schema: String },
    #[error("invalid recipe: {0}")]
    Invalid(String),
    #[error("recipe limits exceeded")]
    Limit,
    #[error("conflicting recipes: {0}")]
    Conflict(String),
}
pub type Result<T> = std::result::Result<T, RecipeError>;
