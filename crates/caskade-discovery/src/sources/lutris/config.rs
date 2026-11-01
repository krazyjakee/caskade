//! Original typed reader of Lutris YAML format; no Lutris code is copied.
use super::super::{is_file, plain_name, read};
use crate::{HostEnv, SourceError, env::KnownFolder};
use serde::Deserialize;
use std::path::{Path, PathBuf};
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct Config {
    pub game: Game,
    pub wine: Wine,
}
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct Game {
    pub exe: Option<String>,
    pub prefix: Option<String>,
    pub arch: Option<String>,
    pub args: Arguments,
    pub working_dir: Option<String>,
}
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct Wine {
    pub version: Option<String>,
}
#[derive(Debug, Default, Deserialize)]
#[serde(untagged)]
pub(super) enum Arguments {
    Text(String),
    List(Vec<String>),
    #[default]
    Empty,
}
impl Arguments {
    pub fn argv(&self) -> Result<Vec<String>, String> {
        let args = match self {
            Self::Empty => vec![],
            Self::List(args) => args.clone(),
            Self::Text(text) => split(text)?,
        };
        if args.len() > 256
            || args
                .iter()
                .any(|arg| arg.len() > 16_384 || arg.contains('\0'))
        {
            return Err("argument budget exceeded".into());
        }
        Ok(args)
    }
}
// A quoting-aware argv reader only. No expansion, substitution or shell execution.
fn split(text: &str) -> Result<Vec<String>, String> {
    let mut words = vec![];
    let mut word = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut active = false;
    for ch in text.chars() {
        if escaped {
            word.push(ch);
            escaped = false;
            active = true;
            continue;
        }
        if ch == '\\' && quote != Some('\'') {
            escaped = true;
            active = true;
            continue;
        }
        if Some(ch) == quote {
            quote = None;
            continue;
        }
        if quote.is_none() && matches!(ch, '\'' | '"') {
            quote = Some(ch);
            active = true;
            continue;
        }
        if quote.is_none() && ch.is_whitespace() {
            if active {
                words.push(std::mem::take(&mut word));
                active = false;
            }
        } else {
            word.push(ch);
            active = true;
        }
    }
    if escaped || quote.is_some() {
        return Err("unterminated argument quoting".into());
    }
    if active {
        words.push(word);
    }
    Ok(words)
}
pub(super) fn load(
    env: &HostEnv,
    data: &Path,
    flatpak: bool,
    key: &str,
) -> Result<Option<(PathBuf, Config)>, SourceError> {
    if !plain_name(key) {
        return Ok(None);
    }
    let mut roots = vec![];
    if flatpak {
        roots.push(env.home().join(".var/app/net.lutris.Lutris/config/lutris"));
    } else if let Some(config) = env.known_folder(KnownFolder::ConfigHome) {
        roots.push(config.join("lutris"));
    }
    roots.push(data.to_owned());
    for root in roots {
        for extension in ["yml", "yaml"] {
            let path = root.join("games").join(format!("{key}.{extension}"));
            if !is_file(env, &path) {
                continue;
            }
            let bytes = read(env, &path, 1 << 20).map_err(|e| SourceError::io(&path, e))?;
            let config =
                serde_yaml_ng::from_slice(&bytes).map_err(|e| SourceError::parse(&path, e))?;
            return Ok(Some((path, config)));
        }
    }
    Ok(None)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn argv_quotes_empty_arguments_and_metacharacters_remain_data() {
        assert_eq!(
            split("--name 'Fixture Game' \"\" '$(touch no)' a\\ b").unwrap(),
            ["--name", "Fixture Game", "", "$(touch no)", "a b"]
        );
        assert!(split("'unfinished").is_err());
        assert!(Arguments::List(vec!["a".into(); 257]).argv().is_err());
    }
}
