//! Named placeholders in translated text: `{name}`, filled in after the
//! lookup, so a translation can put them in any order. `{{` and `}}` are
//! literal braces.

use std::collections::BTreeSet;
use std::fmt::{Display, Write as _};

/// `template` with each `{name}` replaced by its argument. A placeholder
/// without an argument stays as it is.
pub fn format(template: &str, args: &[(&str, &dyn Display)]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(at) = rest.find(['{', '}']) {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        if tail.starts_with("{{") || tail.starts_with("}}") {
            out.push_str(&tail[..1]);
            rest = &tail[2..];
            continue;
        }
        if let Some(name) = placeholder(tail)
            && let Some((_, value)) = args.iter().find(|(arg, _)| *arg == name)
        {
            let _ = write!(out, "{value}");
            rest = &tail[name.len() + 2..];
            continue;
        }
        out.push_str(&tail[..1]);
        rest = &tail[1..];
    }
    out.push_str(rest);
    out
}

/// The placeholder names in `template`, for checking that a translation
/// keeps the source's.
pub fn placeholders(template: &str) -> BTreeSet<&str> {
    let mut names = BTreeSet::new();
    let mut rest = template;
    while let Some(at) = rest.find(['{', '}']) {
        let tail = &rest[at..];
        if tail.starts_with("{{") || tail.starts_with("}}") {
            rest = &tail[2..];
            continue;
        }
        if let Some(name) = placeholder(tail) {
            names.insert(name);
            rest = &tail[name.len() + 2..];
            continue;
        }
        rest = &tail[1..];
    }
    names
}

/// The name in a `{name}` at the start of `text`.
fn placeholder(text: &str) -> Option<&str> {
    let inner = text.strip_prefix('{')?;
    let end = inner.find('}')?;
    let name = &inner[..end];
    (!name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_named_placeholders_in_any_order() {
        let game = "Celeste";
        assert_eq!(
            format("{n} spel i {game}", &[("game", &game), ("n", &3)]),
            "3 spel i Celeste"
        );
    }

    #[test]
    fn keeps_escaped_braces_and_unknown_placeholders() {
        assert_eq!(format("{{n}} is {n}", &[("n", &1)]), "{n} is 1");
        assert_eq!(format("{missing} {", &[("n", &1)]), "{missing} {");
        assert_eq!(format("}{not a name}", &[]), "}{not a name}");
    }

    #[test]
    fn lists_placeholders() {
        assert_eq!(
            placeholders("{n} of {game}, {{literal}} {n}")
                .into_iter()
                .collect::<Vec<_>>(),
            ["game", "n"]
        );
    }
}
