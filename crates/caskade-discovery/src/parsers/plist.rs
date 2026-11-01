//! XML and binary plists with graph/event budgets before recursive decoding.
use super::{Limits, ParseError, Result};
use std::io::Cursor;
/// Uses the plist crate's public value so downstream metadata keeps native types.
pub fn parse(bytes: &[u8], limits: &Limits) -> Result<::plist::Value> {
    limits.input(bytes)?;
    if bytes.starts_with(b"bplist00") {
        super::plist_binary::validate(bytes, limits)?;
    } else {
        let mut reader = quick_xml::Reader::from_reader(bytes);
        let mut depth: usize = 0;
        let mut nodes: usize = 0;
        loop {
            nodes += 1;
            limits.check(nodes, limits.max_nodes, "plist events")?;
            match reader
                .read_event()
                .map_err(|e| ParseError::Invalid(e.to_string()))?
            {
                quick_xml::events::Event::Start(_) => {
                    depth += 1;
                    limits.check(depth, limits.max_depth.min(128), "nesting depth")?;
                }
                quick_xml::events::Event::End(_) => {
                    depth = depth
                        .checked_sub(1)
                        .ok_or_else(|| ParseError::Invalid("unbalanced plist".into()))?;
                }
                quick_xml::events::Event::Text(text) => {
                    limits.check(text.len(), limits.max_string_bytes, "string bytes")?;
                }
                quick_xml::events::Event::CData(text) => {
                    limits.check(text.len(), limits.max_string_bytes, "string bytes")?;
                }
                quick_xml::events::Event::Eof => break,
                _ => (),
            }
        }
        if depth != 0 {
            return Err(ParseError::Invalid("unbalanced plist".into()));
        }
    }
    let value = ::plist::Value::from_reader(Cursor::new(bytes))
        .map_err(|e| ParseError::Invalid(e.to_string()))?;
    // The graph guard bounds binary expansion; also enforce scalar byte limits
    // and a total byte budget without recursively walking the resulting value.
    let mut stack = vec![&value];
    let mut expanded: usize = 0;
    let mut nodes: usize = 0;
    while let Some(v) = stack.pop() {
        nodes += 1;
        limits.check(nodes, limits.max_nodes, "plist values")?;
        match v {
            ::plist::Value::Array(values) => {
                limits.check(
                    stack.len().saturating_add(values.len()),
                    limits.max_nodes,
                    "plist values",
                )?;
                stack.extend(values);
            }
            ::plist::Value::Dictionary(values) => {
                limits.check(
                    stack.len().saturating_add(values.len()),
                    limits.max_nodes,
                    "plist values",
                )?;
                for (key, value) in values {
                    limits.check(key.len(), limits.max_string_bytes, "string bytes")?;
                    expanded = expanded.saturating_add(key.len());
                    stack.push(value);
                }
            }
            ::plist::Value::String(text) => {
                limits.check(text.len(), limits.max_string_bytes, "string bytes")?;
                expanded = expanded.saturating_add(text.len());
            }
            ::plist::Value::Data(bytes) => {
                expanded = expanded.saturating_add(bytes.len());
            }
            _ => (),
        }
        limits.check(expanded, limits.max_input_bytes, "expanded plist bytes")?;
    }
    Ok(value)
}
