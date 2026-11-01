use super::{Result, ScanError};
use globset::{GlobBuilder, GlobMatcher};
use regex::Regex;
/// Playnite syntax, independently implemented from the documented grammar.
pub struct Exclusions(Vec<Rule>);
enum Rule {
    Exact(String),
    Glob(GlobMatcher),
    Regex(Regex, bool),
}
impl Exclusions {
    pub fn compile(patterns: impl IntoIterator<Item = impl AsRef<str>>) -> Result<Self> {
        let mut rules = Vec::new();
        for pattern in patterns {
            if rules.len() >= 10_000 {
                return Err(ScanError::Limit("exclusion patterns"));
            }
            let p = pattern.as_ref();
            if p.len() > 4096 {
                return Err(ScanError::Limit("exclusion pattern"));
            }
            let err = |e: String| {
                ScanError::Invalid(crate::tr!(
                    "exclusion {pattern}: {error}",
                    pattern = format!("{p:?}"),
                    error = e
                ))
            };
            if let Some(expr) = p.strip_prefix(">?") {
                rules.push(Rule::Regex(
                    Regex::new(expr).map_err(|e| err(e.to_string()))?,
                    true,
                ));
            } else if let Some(expr) = p.strip_prefix('?') {
                rules.push(Rule::Regex(
                    Regex::new(expr).map_err(|e| err(e.to_string()))?,
                    false,
                ));
            } else if let Some(glob) = p.strip_prefix('>') {
                rules.push(Rule::Glob(
                    GlobBuilder::new(glob)
                        .case_insensitive(true)
                        .literal_separator(false)
                        .build()
                        .map_err(|e| err(e.to_string()))?
                        .compile_matcher(),
                ));
            } else {
                let normalized = p.replace('\\', "/");
                let path = normalized.trim_end_matches('/');
                if path.is_empty() {
                    continue;
                }
                let safe = super::path_text(std::path::Path::new(path))?;
                rules.push(Rule::Exact(safe.to_lowercase()));
            }
        }
        if rules.len() > 10_000 {
            return Err(ScanError::Limit("exclusion patterns"));
        }
        Ok(Self(rules))
    }
    pub fn matches(&self, relative: &str) -> bool {
        let lower = relative.to_lowercase();
        self.0.iter().any(|r| match r {
            Rule::Exact(p) => {
                lower == *p || lower.strip_prefix(p).is_some_and(|s| s.starts_with('/'))
            }
            Rule::Glob(g) => {
                g.is_match(relative) || relative.rsplit('/').next().is_some_and(|s| g.is_match(s))
            }
            Rule::Regex(r, any_depth) => {
                r.find(relative)
                    .is_some_and(|m| *any_depth || m.start() == 0)
                    || (*any_depth
                        && relative
                            .match_indices('/')
                            .any(|(i, _)| r.is_match(&relative[i + 1..])))
            }
        })
    }
}
