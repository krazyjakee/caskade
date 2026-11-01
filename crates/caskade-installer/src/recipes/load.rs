use super::{InputIdentity, MAX_RECIPE_BYTES, Recipe, RecipeError, ResolvedRecipe, Result};
use crate::{ArtifactId, ArtifactNode};
use cap_std::{ambient_authority, fs::Dir};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
};
const MAX_RECIPES: usize = 512;
const MAX_DIRECTORY_BYTES: usize = 8 * 1024 * 1024;
#[derive(Debug, Clone, Default)]
pub struct RecipeSet {
    pub(super) recipes: BTreeMap<String, Recipe>,
    identities: BTreeMap<ArtifactId, InputIdentity>,
}
fn fact(node: &ArtifactNode, key: &str) -> Option<String> {
    node.facts.get(key).and_then(|value| match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        _ => None,
    })
}
pub fn identity_from_artifact(node: &ArtifactNode) -> InputIdentity {
    InputIdentity {
        sha256: None,
        gog_id: fact(node, "gog_id")
            .or_else(|| fact(node, "gog.id"))
            .or_else(|| {
                node.facts
                    .get("ids")
                    .and_then(|value| value.get("gog"))
                    .and_then(|v| v.as_str())
                    .map(str::to_owned)
            }),
        app_id: fact(node, "app_id")
            .or_else(|| fact(node, "inno_app_id"))
            .or_else(|| {
                node.facts
                    .get("inno.metadata")
                    .and_then(|value| value.get("app_id"))
                    .and_then(|v| v.as_str())
                    .map(str::to_owned)
            }),
    }
}
/// The input hash is computed only at this explicit I/O edge. Probe/plan never
/// read an input implicitly. Callers can avoid hashing when requires_hash=false.
pub fn identity_from_path(node: &ArtifactNode, path: &Path, hash: bool) -> Result<InputIdentity> {
    let mut identity = identity_from_artifact(node);
    if hash {
        let mut file = std::fs::File::open(path)?;
        if !file.metadata()?.is_file() {
            return Err(RecipeError::Invalid(
                "hash input must be a regular file".into(),
            ));
        }
        let mut digest = Sha256::new();
        std::io::copy(&mut file, &mut digest)?;
        identity.sha256 = Some(format!("{:x}", digest.finalize()));
    }
    Ok(identity)
}
impl RecipeSet {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn builtin() -> Result<Self> {
        let mut set = Self::new();
        set.insert(Recipe::parse(include_str!(
            "../../recipes/fixture-inno-extract.toml"
        ))?)?;
        Ok(set)
    }
    /// User files replace a builtin recipe with the same ID. Distinct matching
    /// IDs merge only when their scalar overrides agree; conflicts fail closed.
    pub fn load(user_directory: Option<&Path>) -> Result<Self> {
        let mut set = Self::builtin()?;
        if let Some(directory) = user_directory {
            set.load_directory(directory)?;
        }
        Ok(set)
    }
    pub fn insert(&mut self, recipe: Recipe) -> Result<Option<Recipe>> {
        recipe.validate()?;
        if self.recipes.len() >= MAX_RECIPES && !self.recipes.contains_key(&recipe.id) {
            return Err(RecipeError::Limit);
        }
        Ok(self.recipes.insert(recipe.id.clone(), recipe))
    }
    pub fn load_directory(&mut self, path: &Path) -> Result<()> {
        let root = Dir::open_ambient_dir(path, ambient_authority())?;
        let mut paths = Vec::new();
        for entry in root.entries()? {
            let entry = entry?;
            let name = PathBuf::from(entry.file_name());
            if name
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"))
            {
                if paths.len() >= MAX_RECIPES {
                    return Err(RecipeError::Limit);
                }
                if !root.symlink_metadata(&name)?.is_file()
                    || root.symlink_metadata(&name)?.is_symlink()
                {
                    return Err(RecipeError::Invalid(
                        "recipe file is not a regular file".into(),
                    ));
                }
                paths.push(name);
            }
        }
        paths.sort();
        let mut pending = self.clone();
        let mut total = 0_usize;
        let mut ids = std::collections::BTreeSet::new();
        for path in paths {
            let mut text = String::new();
            root.open(&path)?
                .take(MAX_RECIPE_BYTES as u64 + 1)
                .read_to_string(&mut text)?;
            total = total.checked_add(text.len()).ok_or(RecipeError::Limit)?;
            if total > MAX_DIRECTORY_BYTES {
                return Err(RecipeError::Limit);
            }
            let recipe = Recipe::parse(&text)?;
            if !ids.insert(recipe.id.clone()) {
                return Err(RecipeError::Conflict(
                    "duplicate IDs in user recipe directory".into(),
                ));
            }
            pending.insert(recipe)?;
        }
        *self = pending;
        Ok(())
    }
    pub fn recipes(&self) -> impl Iterator<Item = &Recipe> {
        self.recipes.values()
    }
    pub fn requires_hash(&self) -> bool {
        self.recipes
            .values()
            .any(|recipe| recipe.matches.sha256.is_some())
    }
    pub fn bind_input(&mut self, node: &ArtifactNode, path: &Path) -> Result<()> {
        self.identities.insert(
            node.id.clone(),
            identity_from_path(node, path, self.requires_hash())?,
        );
        Ok(())
    }
    pub fn resolve(&self, identity: &InputIdentity) -> Result<ResolvedRecipe> {
        super::apply::resolve(
            self.recipes
                .values()
                .filter(|recipe| recipe.matches.accepts(identity)),
        )
    }
    pub fn resolve_artifact(&self, node: &ArtifactNode) -> Result<ResolvedRecipe> {
        self.resolve(
            self.identities
                .get(&node.id)
                .unwrap_or(&identity_from_artifact(node)),
        )
    }
}
