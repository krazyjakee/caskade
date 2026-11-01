use super::{Recipe, RecipeError, Result};
use crate::{Program, SourcePath, Step, safety::sanitize_path};
use std::{path::Path, sync::LazyLock};
pub const MAX_RECIPE_BYTES: usize = 256 * 1024;
pub fn json_schema() -> serde_json::Value {
    let mut schema = serde_json::to_value(schemars::schema_for!(Recipe))
        .expect("derived recipe schema serializes");
    schema["$defs"]["Match"]["anyOf"] = serde_json::json!([
        {"required": ["sha256"]}, {"required": ["gog_id"]}, {"required": ["app_id"]}
    ]);
    schema
}
static VALIDATOR: LazyLock<std::result::Result<jsonschema::Validator, String>> =
    LazyLock::new(|| jsonschema::validator_for(&json_schema()).map_err(|error| error.to_string()));
fn invalid(message: &str) -> RecipeError {
    RecipeError::Invalid(message.into())
}
fn relative(path: &Path, allow_root: bool) -> Result<()> {
    if allow_root && matches!(path.to_str(), Some("." | "")) {
        return Ok(());
    }
    let text = path.to_str().ok_or_else(|| invalid("path encoding"))?;
    let path = sanitize_path(text).map_err(|error| invalid(&error.to_string()))?;
    if path.components().count() > 128 {
        return Err(RecipeError::Limit);
    }
    Ok(())
}
fn source(source: &SourcePath) -> Result<()> {
    match source {
        SourcePath::Artifact { artifact, path } => {
            if artifact.0.is_empty() || artifact.0.len() > 256 {
                return Err(invalid("source artifact ID"));
            }
            relative(path, true)
        }
        SourcePath::Game(path) => relative(path, true),
    }
}
fn program(program: &Program) -> Result<()> {
    match program {
        Program::Source(path) => source(path),
        Program::Tool(tool) if !tool.is_empty() && !tool.contains('\0') => Ok(()),
        _ => Err(invalid("program")),
    }
}
fn args(args: &[String]) -> Result<()> {
    if args.len() > 256 || args.iter().any(|s| s.len() > 16 * 1024 || s.contains('\0')) {
        return Err(invalid("argument vector limits or NUL"));
    }
    Ok(())
}
pub(super) fn validate_steps(steps: &[Step]) -> Result<()> {
    if steps.len() > 256 {
        return Err(RecipeError::Limit);
    }
    let mut bytes = 0_usize;
    for step in steps {
        match step {
            Step::WriteFile { path, contents } => {
                relative(path, false)?;
                bytes = bytes
                    .checked_add(contents.len())
                    .ok_or(RecipeError::Limit)?;
            }
            Step::Extract {
                from,
                select,
                strip,
                to,
            } => {
                if from.0.is_empty() || from.0.len() > 256 || select.len() > 256 {
                    return Err(invalid("extract arguments"));
                }
                for glob in select {
                    if glob.len() > 4096 {
                        return Err(RecipeError::Limit);
                    }
                    globset::Glob::new(glob).map_err(|_| invalid("extract glob"))?;
                }
                if let Some(path) = strip {
                    relative(path, true)?;
                }
                relative(to, true)?;
            }
            Step::Copy { from, to } | Step::Move { from, to } => {
                source(from)?;
                relative(to, true)?;
            }
            Step::SetExecutable { path } => relative(path, false)?,
            Step::CopyRange { from, path, .. } => {
                if from.0.is_empty() || from.0.len() > 256 {
                    return Err(invalid("copy range source"));
                }
                relative(path, false)?;
            }
            Step::Symlink { path, target } => {
                relative(path, false)?;
                let text = target.to_str().ok_or_else(|| invalid("link encoding"))?;
                if text.is_empty()
                    || text.len() > 4096
                    || text.contains(['\0', ':'])
                    || text.starts_with(['/', '\\'])
                {
                    return Err(invalid("link target"));
                }
                let mut depth = path.components().count() - 1;
                for part in text.split(['/', '\\']) {
                    if part == ".." {
                        depth = depth
                            .checked_sub(1)
                            .ok_or_else(|| invalid("link escapes game root"))?;
                    } else if !matches!(part, "" | ".") {
                        depth += 1;
                    }
                }
            }
            Step::Run {
                program: command,
                args: argv,
                env,
                timeout_secs,
                expect,
                ..
            } => {
                program(command)?;
                args(argv)?;
                if *timeout_secs == 0 || *timeout_secs > 86400 {
                    return Err(invalid("run timeout"));
                }
                if env.len() > 256
                    || env.iter().any(|(key, value)| {
                        key.is_empty() || key.contains(['\0', '=']) || value.contains('\0')
                    })
                {
                    return Err(invalid("environment"));
                }
                for path in expect {
                    relative(path, false)?;
                }
            }
            Step::RunInteractive {
                program: command,
                args: argv,
                env,
                target_dir,
                ..
            } => {
                program(command)?;
                args(argv)?;
                relative(target_dir, true)?;
                if env.len() > 256
                    || env.iter().any(|(key, value)| {
                        key.is_empty() || key.contains(['\0', '=']) || value.contains('\0')
                    })
                {
                    return Err(invalid("environment"));
                }
            }
            Step::Mount { mount_point, .. } | Step::Unmount { mount_point } => {
                relative(mount_point, false)?
            }
            Step::RegistryImport { runtime, .. } => {
                if *runtime == crate::RuntimeSpec::Native {
                    return Err(invalid("registry import requires an isolated prefix"));
                }
            }
            Step::Delegate {
                tool,
                args: argv,
                uninstall,
                only_if,
            } => {
                if tool.is_empty()
                    || tool.contains('\0')
                    || uninstall.is_empty()
                    || uninstall.len() > 256
                {
                    return Err(invalid("delegate requires explicit uninstall"));
                }
                args(argv)?;
                for command in uninstall {
                    if command.tool.is_empty() || command.tool.contains('\0') {
                        return Err(invalid("uninstall tool"));
                    }
                    args(&command.args)?;
                }
                for check in only_if.iter().chain(
                    uninstall
                        .iter()
                        .filter_map(|command| command.only_if.as_ref()),
                ) {
                    if check.tool.is_empty() || check.tool.contains('\0') {
                        return Err(invalid("delegate check tool"));
                    }
                    args(&check.args)?;
                }
            }
        }
        if bytes > 1024 * 1024 {
            return Err(RecipeError::Limit);
        }
    }
    Ok(())
}
impl Recipe {
    pub fn parse(text: &str) -> Result<Self> {
        if text.len() > MAX_RECIPE_BYTES {
            return Err(RecipeError::Limit);
        }
        let value: toml::Value = toml::from_str(text)
            .map_err(|error: toml::de::Error| RecipeError::Toml(error.message().into()))?;
        let json = serde_json::to_value(value).map_err(|_| invalid("non-JSON TOML value"))?;
        let validator = VALIDATOR.as_ref().map_err(|message| invalid(message))?;
        validator
            .validate(&json)
            .map_err(|error| RecipeError::Schema {
                instance: error.instance_path().to_string(),
                schema: error.schema_path().to_string(),
            })?;
        let recipe: Self = serde_json::from_value(json).map_err(|_| invalid("recipe fields"))?;
        recipe.validate()?;
        Ok(recipe)
    }
    pub fn validate(&self) -> Result<()> {
        let json = serde_json::to_value(self).map_err(|_| invalid("recipe fields"))?;
        VALIDATOR
            .as_ref()
            .map_err(|message| invalid(message))?
            .validate(&json)
            .map_err(|error| RecipeError::Schema {
                instance: error.instance_path().to_string(),
                schema: error.schema_path().to_string(),
            })?;
        if self.schema_version != 1
            || self.id.is_empty()
            || self.id.len() > 128
            || !self
                .id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
        {
            return Err(invalid("version or recipe ID"));
        }
        if self.matches.sha256.is_none()
            && self.matches.gog_id.is_none()
            && self.matches.app_id.is_none()
        {
            return Err(invalid("at least one exact match selector is required"));
        }
        if let Some(hash) = &self.matches.sha256
            && (hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(invalid("SHA-256 selector"));
        }
        for text in [
            &self.matches.gog_id,
            &self.matches.app_id,
            &self.preset_args.language,
        ]
        .into_iter()
        .flatten()
        {
            if text.is_empty() || text.len() > 256 || text.chars().any(char::is_control) {
                return Err(invalid("selector or preset contains invalid text"));
            }
        }
        validate_steps(&self.add_steps)
    }
}
