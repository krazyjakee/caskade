//! Valve binary KeyValues and Steam appinfo format 0x28/0x29.
//! Written from format documentation, not launcher implementation code.
//! References: Valve Source SDK KeyValues.h; xPaw/SteamAppInfo format README.
use super::{Limits, ParseError, Result, cursor::Cursor};
use serde::{Deserialize, Serialize};

/// Object pairs remain ordered and retain duplicate names.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Value {
    Object(Vec<(String, Value)>),
    String(String),
    Int(i32),
    Float(f32),
    Pointer(u32),
    WideString(String),
    Color([u8; 4]),
    Uint64(u64),
    Int64(i64),
}
impl Value {
    pub fn get(&self, name: &str) -> Option<&Value> {
        match self {
            Self::Object(pairs) => pairs
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        if let Self::String(value) = self {
            Some(value)
        } else {
            None
        }
    }
}
struct Budget {
    nodes: usize,
    strings: usize,
}
fn object(
    c: &mut Cursor<'_>,
    table: Option<&[String]>,
    depth: usize,
    budget: &mut Budget,
) -> Result<Value> {
    c.limits
        .check(depth, c.limits.max_depth.min(128), "nesting depth")?;
    let mut pairs = Vec::new();
    loop {
        let kind = c.u8()?;
        if kind == 8 {
            break;
        }
        budget.nodes = budget.nodes.saturating_add(1);
        c.limits.check(budget.nodes, c.limits.max_nodes, "nodes")?;
        let key = match table {
            Some(table) => table
                .get(c.u32()? as usize)
                .ok_or_else(|| ParseError::Invalid("string table index".into()))?
                .clone(),
            None => c.cstr()?,
        };
        budget.strings = budget.strings.saturating_add(key.len());
        c.limits
            .check(budget.strings, c.limits.max_input_bytes, "expanded strings")?;
        let value = match kind {
            0 => object(c, table, depth + 1, budget)?,
            1 => Value::String(c.cstr()?),
            2 => Value::Int(c.u32()? as i32),
            3 => {
                let v = f32::from_bits(c.u32()?);
                if !v.is_finite() {
                    return Err(ParseError::Invalid("non-finite float".into()));
                }
                Value::Float(v)
            }
            4 => Value::Pointer(c.u32()?),
            5 => {
                let count = c.u16()? as usize;
                Value::WideString(c.utf16(count)?.trim_end_matches('\0').to_owned())
            }
            6 => Value::Color(c.take(4)?.try_into().expect("four bytes")),
            7 => Value::Uint64(c.u64()?),
            10 => Value::Int64(c.u64()? as i64),
            _ => return Err(ParseError::Unsupported(format!("binary VDF type {kind}"))),
        };
        if let Value::String(text) | Value::WideString(text) = &value {
            budget.strings = budget.strings.saturating_add(text.len());
        }
        c.limits
            .check(budget.strings, c.limits.max_input_bytes, "expanded strings")?;
        pairs.push((key, value));
    }
    Ok(Value::Object(pairs))
}
/// Read a complete shortcuts.vdf-style binary document, including its root end marker.
pub fn parse(bytes: &[u8], limits: &Limits) -> Result<Value> {
    limits.input(bytes)?;
    let mut c = Cursor::new(bytes, limits);
    let value = object(
        &mut c,
        None,
        0,
        &mut Budget {
            nodes: 0,
            strings: 0,
        },
    )?;
    if c.pos != bytes.len() {
        return Err(ParseError::Invalid("trailing binary VDF bytes".into()));
    }
    Ok(value)
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AppInfo {
    pub version: u8,
    pub universe: u32,
    pub apps: Vec<App>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct App {
    pub app_id: u32,
    pub info_state: u32,
    pub last_updated: u32,
    pub access_token: u64,
    pub text_sha1: [u8; 20],
    pub change_number: u32,
    pub binary_sha1: [u8; 20],
    pub data: Value,
}
/// Parse appinfo's entry spans and, for v29, its absolute-offset key string table.
pub fn parse_appinfo(bytes: &[u8], limits: &Limits) -> Result<AppInfo> {
    parse_appinfo_inner(bytes, limits, None)
}
/// Decode only installed app ids from a large cache. Unwanted entry spans are
/// validated and skipped without allocating their trees or consuming KV nodes.
/// Input/string-table limits still apply to the entire cache; callers should
/// set max_input_bytes for their cache budget (for example 256 MiB). The node
/// budget applies per selected app, with an aggregate ceiling of sixteen such
/// budgets to keep a hostile selection from multiplying allocation indefinitely.
pub fn parse_appinfo_selected(
    bytes: &[u8],
    limits: &Limits,
    wanted: &std::collections::BTreeSet<u32>,
) -> Result<AppInfo> {
    parse_appinfo_inner(bytes, limits, Some(wanted))
}
fn parse_appinfo_inner(
    bytes: &[u8],
    limits: &Limits,
    wanted: Option<&std::collections::BTreeSet<u32>>,
) -> Result<AppInfo> {
    limits.input(bytes)?;
    let mut c = Cursor::new(bytes, limits);
    let magic = c.u32()?;
    if !matches!(magic, 0x0756_4428 | 0x0756_4429) {
        return Err(ParseError::Unsupported(format!("appinfo magic {magic:#x}")));
    }
    let version = magic as u8;
    let universe = c.u32()?;
    let table_offset = if version == 0x29 {
        usize::try_from(c.u64()?).map_err(|_| ParseError::Invalid("string table offset".into()))?
    } else {
        bytes.len()
    };
    if table_offset < c.pos || table_offset > bytes.len() {
        return Err(ParseError::Invalid("string table offset".into()));
    }
    let table = if version == 0x29 {
        let mut tc = Cursor::new(&bytes[table_offset..], limits);
        let count = tc.u32()? as usize;
        limits.check(count, limits.max_nodes, "string table entries")?;
        let mut table = Vec::new();
        for _ in 0..count {
            table.push(tc.cstr()?);
        }
        if tc.pos != tc.data.len() {
            return Err(ParseError::Invalid("trailing string table bytes".into()));
        }
        Some(table)
    } else {
        None
    };
    c.data = &bytes[..table_offset];
    let mut apps = Vec::new();
    let mut selected_nodes: usize = 0;
    let mut budget = Budget {
        nodes: 0,
        strings: 0,
    };
    loop {
        let app_id = c.u32()?;
        if app_id == 0 {
            break;
        }
        let size = c.u32()? as usize;
        let span = c.take(size)?;
        if size < 61 {
            return Err(ParseError::Invalid(
                "appinfo entry shorter than header".into(),
            ));
        }
        if wanted.is_some_and(|ids| !ids.contains(&app_id)) {
            continue;
        }
        limits.check(
            apps.len().saturating_add(1),
            limits.max_nodes,
            "app entries",
        )?;
        if wanted.is_some() {
            budget.nodes = 0;
        }
        let mut entry = Cursor::new(span, limits);
        let info_state = entry.u32()?;
        let last_updated = entry.u32()?;
        let access_token = entry.u64()?;
        let text_sha1 = entry.take(20)?.try_into().expect("twenty bytes");
        let change_number = entry.u32()?;
        let binary_sha1 = entry.take(20)?.try_into().expect("twenty bytes");
        let data = object(&mut entry, table.as_deref(), 0, &mut budget)?;
        if wanted.is_some() {
            selected_nodes = selected_nodes.saturating_add(budget.nodes);
            limits.check(
                selected_nodes,
                limits.max_nodes.saturating_mul(16),
                "selected app aggregate nodes",
            )?;
        }
        if entry.pos != size {
            return Err(ParseError::Invalid("appinfo entry size mismatch".into()));
        }
        apps.push(App {
            app_id,
            info_state,
            last_updated,
            access_token,
            text_sha1,
            change_number,
            binary_sha1,
            data,
        });
    }
    if c.pos != table_offset {
        return Err(ParseError::Invalid("appinfo terminator offset".into()));
    }
    Ok(AppInfo {
        version,
        universe,
        apps,
    })
}
