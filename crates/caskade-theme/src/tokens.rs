//! `tokens.json`: DTCG overrides of declared tokens, compiled to `--cs-*`
//! declarations. Values follow the same rules as `packages/tokens`.
use crate::Problem;
use crate::contract::contract;
use serde_json::Value;

const FILE: &str = "tokens.json";

pub(crate) fn compile(text: &str) -> Result<Vec<(String, String)>, Vec<Problem>> {
    let tree: Value =
        serde_json::from_str(text).map_err(|error| vec![Problem::json(FILE, &error)])?;
    let mut out = Vec::new();
    let mut problems = Vec::new();
    walk(&tree, &mut Vec::new(), None, &mut out, &mut problems);
    if problems.is_empty() {
        Ok(out)
    } else {
        Err(problems)
    }
}

fn token_problem(path: &str, message: impl std::fmt::Display) -> Problem {
    Problem::new(
        FILE,
        format!(
            "{}: {message}",
            if path.is_empty() { "(root)" } else { path }
        ),
    )
}

fn valid_segment(key: &str) -> bool {
    key == "$root"
        || (key.starts_with(|c: char| c.is_ascii_lowercase())
            && key.split('-').all(|part| {
                !part.is_empty()
                    && part
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            }))
}

fn walk(
    node: &Value,
    path: &mut Vec<String>,
    inherited: Option<&str>,
    out: &mut Vec<(String, String)>,
    problems: &mut Vec<Problem>,
) {
    let dotted = path
        .iter()
        .filter(|part| *part != "$root")
        .cloned()
        .collect::<Vec<_>>()
        .join(".");
    let Value::Object(map) = node else {
        problems.push(token_problem(&dotted, "expected a group or token"));
        return;
    };
    let declared = map.get("$type").and_then(Value::as_str).or(inherited);
    if let Some(value) = map.get("$value") {
        let Some(token) = contract().tokens.get(&dotted) else {
            problems.push(token_problem(
                &dotted,
                "is not a token of the theming contract",
            ));
            return;
        };
        if let Some(kind) = declared.filter(|kind| *kind != token.kind) {
            problems.push(token_problem(
                &dotted,
                format!("is a {}, not a {kind}", token.kind),
            ));
            return;
        }
        match css_value(&token.kind, value) {
            Ok(css) => out.push((token.name.clone(), css)),
            Err(message) => problems.push(token_problem(&dotted, message)),
        }
        return;
    }
    for (key, child) in map {
        if key.starts_with('$') && key != "$root" {
            continue;
        }
        path.push(key.clone());
        if valid_segment(key) {
            walk(child, path, declared, out, problems);
        } else {
            problems.push(token_problem(&path.join("."), "invalid name"));
        }
        path.pop();
    }
}

/// A value as CSS: an alias becomes `var()`, a literal is checked by type.
fn css_value(kind: &str, value: &Value) -> Result<String, String> {
    if let Some(alias) = value
        .as_str()
        .and_then(|text| text.strip_prefix('{'))
        .and_then(|text| text.strip_suffix('}'))
    {
        let target = alias.strip_suffix(".$root").unwrap_or(alias);
        let token = contract()
            .tokens
            .get(target)
            .ok_or_else(|| format!("unknown alias {{{alias}}}"))?;
        if token.kind != kind {
            return Err(format!(
                "alias {{{alias}}} is a {}, not a {kind}",
                token.kind
            ));
        }
        return Ok(format!("var({})", token.name));
    }
    literal(kind, value).ok_or_else(|| {
        let expected = match kind {
            "color" => "a hex colour",
            "dimension" => "a dimension in px, rem, em, %, vh, vw",
            "duration" => "a duration in ms, s",
            "number" => "a number",
            "cubicBezier" => "four numbers with x in 0..1",
            "fontFamily" => "a font family or list",
            _ => "a CSS keyword",
        };
        format!("expected {expected}, got {value}")
    })
}

fn number(value: f64) -> String {
    // JavaScript's String(n) for the numbers tokens use.
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

fn measured(value: &Value, units: &[&str]) -> Option<String> {
    if let Some(text) = value.as_str() {
        if text == "0" {
            return Some(text.to_owned());
        }
        let digits = text.trim_start_matches('-');
        let split = digits
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(digits.len());
        let (amount, unit) = digits.split_at(split);
        let numeric = !amount.is_empty()
            && amount.matches('.').count() <= 1
            && !amount.ends_with('.')
            && amount.chars().any(|c| c.is_ascii_digit());
        return (numeric && units.contains(&unit)).then(|| text.to_owned());
    }
    let amount = value.get("value")?.as_f64()?;
    let unit = value.get("unit")?.as_str()?;
    units
        .contains(&unit)
        .then(|| format!("{}{unit}", number(amount)))
}

fn hex(text: &str) -> Option<String> {
    let digits = text.strip_prefix('#')?;
    ([3, 4, 6, 8].contains(&digits.len()) && digits.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| text.to_ascii_lowercase())
}

fn literal(kind: &str, value: &Value) -> Option<String> {
    match kind {
        "color" => value
            .as_str()
            .or_else(|| value.get("hex").and_then(Value::as_str))
            .and_then(hex),
        "dimension" => measured(value, &["px", "rem", "em", "%", "vh", "vw"]),
        "duration" => measured(value, &["ms", "s"]),
        "number" => value.as_f64().filter(|n| n.is_finite()).map(number),
        "cubicBezier" => {
            let points: Vec<f64> = value
                .as_array()?
                .iter()
                .map(Value::as_f64)
                .collect::<Option<_>>()?;
            (points.len() == 4
                && points.iter().all(|n| n.is_finite())
                && [points[0], points[2]]
                    .iter()
                    .all(|x| (0.0..=1.0).contains(x)))
            .then(|| {
                let parts: Vec<String> = points.into_iter().map(number).collect();
                format!("cubic-bezier({})", parts.join(", "))
            })
        }
        "fontFamily" => {
            let names: Vec<&str> = match value {
                Value::String(name) => vec![name.as_str()],
                Value::Array(names) => names.iter().map(Value::as_str).collect::<Option<_>>()?,
                _ => return None,
            };
            if names.is_empty() {
                return None;
            }
            let quoted: Option<Vec<String>> = names
                .into_iter()
                .map(|name| {
                    let safe = !name.is_empty()
                        && !name.contains(['"', '\'', '\\', ';', '{', '}', '<', '>']);
                    safe.then(|| {
                        if name.chars().all(|c| c.is_ascii_lowercase() || c == '-') {
                            name.to_owned()
                        } else {
                            format!("\"{name}\"")
                        }
                    })
                })
                .collect();
            Some(quoted?.join(", "))
        }
        "keyword" => value
            .as_str()
            .filter(|text| {
                !text.is_empty()
                    && text
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || " ./%-".contains(c))
            })
            .map(str::to_owned),
        _ => None,
    }
}
