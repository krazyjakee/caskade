//! Text KeyValues via keyvalues-parser. Include directives are reported, never read.
use super::{Limits, ParseError, Parsed, Result, binary_vdf::Value};

/// Preflight nesting before the dependency's recursive grammar runs.
fn guard(text: &str, limits: &Limits) -> Result<()> {
    let mut chars = text.chars().peekable();
    let mut depth: usize = 0;
    let mut tokens: usize = 0;
    while let Some(ch) = chars.next() {
        if ch == '/' && chars.peek() == Some(&'/') {
            for c in chars.by_ref() {
                if c == '\n' {
                    break;
                }
            }
            continue;
        }
        if ch == '"' {
            tokens += 1;
            let mut size = 0;
            loop {
                let ch = chars
                    .next()
                    .ok_or_else(|| ParseError::Invalid("unterminated VDF string".into()))?;
                if ch == '"' {
                    break;
                }
                size += ch.len_utf8();
                if ch == '\\' {
                    size += chars
                        .next()
                        .ok_or_else(|| ParseError::Invalid("VDF escape".into()))?
                        .len_utf8();
                }
                limits.check(size, limits.max_string_bytes, "string bytes")?;
            }
        } else if ch == '{' {
            depth += 1;
            limits.check(depth, limits.max_depth.min(128), "nesting depth")?;
        } else if ch == '}' {
            depth = depth
                .checked_sub(1)
                .ok_or_else(|| ParseError::Invalid("unbalanced VDF".into()))?;
        } else if !ch.is_whitespace() {
            tokens += 1;
            let mut size = ch.len_utf8();
            while chars
                .peek()
                .is_some_and(|c| !c.is_whitespace() && !matches!(c, '{' | '}' | '"'))
            {
                size += chars.next().expect("peeked").len_utf8();
            }
            limits.check(size, limits.max_string_bytes, "string bytes")?;
        }
        limits.check(tokens, limits.max_nodes.saturating_mul(2), "tokens")?;
    }
    if depth != 0 {
        return Err(ParseError::Invalid("unbalanced VDF".into()));
    }
    Ok(())
}
fn convert(value: keyvalues_parser::Value<'_>) -> Value {
    match value {
        keyvalues_parser::Value::Str(text) => Value::String(text.into_owned()),
        keyvalues_parser::Value::Obj(obj) => Value::Object(
            obj.0
                .into_iter()
                .flat_map(|(k, vals)| vals.into_iter().map(move |v| (k.to_string(), convert(v))))
                .collect(),
        ),
    }
}
pub fn parse(bytes: &[u8], limits: &Limits) -> Result<Parsed<Value>> {
    let text = limits.text(bytes)?;
    guard(text, limits)?;
    let v = keyvalues_parser::parse(text).map_err(|e| ParseError::Invalid(e.to_string()))?;
    let warnings = v
        .bases
        .iter()
        .map(|_| "ignored VDF #base directive".to_owned())
        .collect();
    Ok(Parsed {
        value: Value::Object(vec![(v.key.into_owned(), convert(v.value))]),
        warnings,
    })
}
