//! Original bounded reader of Ubisoft's configurations cache. Wire field facts:
//! Playnite UplayLibrary/Models/LocalCache.cs (MIT); YAML layout:
//! ProductSchema.cs (MIT), BoilR uplay/platform.rs (Apache-2.0). No code ported.
use crate::parsers::{Limits, protobuf::preflight};
use prost::Message;
use serde::Deserialize;
use std::{collections::BTreeMap, io};
fn invalid(s: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, s.to_string())
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheTitle {
    pub id: u64,
    pub install_id: u64,
    pub title: String,
}
#[derive(Debug, Default)]
pub struct ConfigurationCache {
    pub games: Vec<CacheTitle>,
    pub warnings: Vec<String>,
}
#[derive(Clone, PartialEq, prost::Message)]
struct Cache {
    #[prost(message, repeated, tag = "1")]
    records: Vec<Record>,
}
#[derive(Clone, PartialEq, prost::Message)]
struct Record {
    #[prost(uint64, tag = "1")]
    id: u64,
    #[prost(uint64, tag = "2")]
    install_id: u64,
    #[prost(bytes = "vec", optional, tag = "3")]
    configuration: Option<Vec<u8>>,
}
const LIMITS: Limits = Limits {
    max_input_bytes: 8 << 20,
    max_string_bytes: 1 << 20,
    max_nodes: 50_000,
    max_depth: 1,
};
#[derive(Default, Deserialize)]
#[serde(default)]
struct Product {
    root: ProductRoot,
    localizations: Localizations,
    uplay_id: u64,
    install_id: u64,
}
#[derive(Default, Deserialize)]
#[serde(default)]
struct ProductRoot {
    name: String,
    is_ulc: bool,
}
#[derive(Default, Deserialize)]
#[serde(default)]
struct Localizations {
    default: BTreeMap<String, String>,
}
// Bound flow nesting and disallow aliases/anchors/custom tags before serde
// allocates expanded values. Quoted titles and comments are ordinary data.
fn yaml_preflight(text: &str) -> io::Result<()> {
    let mut quote = None;
    let mut escape = false;
    let mut comment = false;
    let mut flow = 0usize;
    let mut previous = b' ';
    let mut nodes = 0;
    for byte in text.bytes() {
        if comment {
            if byte == b'\n' {
                comment = false;
            }
            previous = byte;
            continue;
        }
        if let Some(q) = quote {
            if q == b'"' && !escape && byte == b'\\' {
                escape = true;
                continue;
            }
            if !escape && byte == q {
                quote = None;
            }
            escape = false;
            previous = byte;
            continue;
        }
        let token = previous.is_ascii_whitespace() || b"[{,:".contains(&previous);
        match byte {
            b'\'' | b'"' => quote = Some(byte),
            b'#' if token => comment = true,
            b'&' | b'*' | b'!' if token => {
                return Err(invalid("cache YAML aliases/anchors/tags forbidden"));
            }
            b'[' | b'{' => {
                flow += 1;
                if flow > 32 {
                    return Err(invalid("cache YAML flow depth"));
                }
            }
            b']' | b'}' => flow = flow.saturating_sub(1),
            b'\n' | b':' => {
                nodes += 1;
                if nodes > 20_000 {
                    return Err(invalid("cache YAML node budget"));
                }
            }
            0 => return Err(invalid("NUL in cache YAML")),
            _ => {}
        }
        previous = byte;
    }
    Ok(())
}
fn title(bytes: &[u8], id: u64, install_id: u64) -> io::Result<Option<CacheTitle>> {
    let text = std::str::from_utf8(bytes).map_err(invalid)?;
    yaml_preflight(text)?;
    let product: Product = serde_yaml_ng::from_str(text).map_err(invalid)?;
    if product.root.is_ulc {
        return Ok(None);
    }
    let value = product
        .localizations
        .default
        .get(&product.root.name)
        .unwrap_or(&product.root.name)
        .trim();
    if value.is_empty() {
        return Ok(None);
    }
    if value.len() > 4096 || value.chars().any(char::is_control) {
        return Err(invalid("invalid cache title"));
    }
    let id = if id == 0 { product.uplay_id } else { id };
    let install_id = if install_id == 0 {
        product.install_id
    } else {
        install_id
    };
    if id == 0 && install_id == 0 {
        return Err(invalid("cache title missing product id"));
    }
    Ok(Some(CacheTitle {
        id: if id == 0 { install_id } else { id },
        install_id: if install_id == 0 { id } else { install_id },
        title: value.into(),
    }))
}
/// A cache is a protobuf collection, not an account/ownership database. Only
/// installed registry records use these titles; cached owned games aren't emitted.
pub fn parse_configurations(bytes: &[u8]) -> io::Result<ConfigurationCache> {
    if bytes.len() > 8 << 20 {
        return Err(invalid("configurations exceeds 8 MiB"));
    }
    let mut result = ConfigurationCache::default();
    // A plain YAML export is useful for older cache layouts and authored fixtures.
    if bytes.starts_with(b"version:") || bytes.starts_with(b"root:") {
        if let Some(game) = title(bytes, 0, 0)? {
            result.games.push(game);
        }
        return Ok(result);
    }
    // Records are the only nested messages (kind 1).
    preflight(bytes, 0, 0, &mut 0, &LIMITS, |kind, field| {
        (kind == 0 && field == 1).then_some(1)
    })
    .map_err(invalid)?;
    let cache = Cache::decode(bytes).map_err(invalid)?;
    for record in cache.records {
        if result.games.len() + result.warnings.len() >= 10_000 {
            return Err(invalid("cache record budget"));
        }
        let (id, install_id) = (record.id, record.install_id);
        if let Some(yaml) = record.configuration {
            match title(&yaml, id, install_id) {
                Ok(Some(game)) => result.games.push(game),
                Ok(None) => {}
                Err(e) => result
                    .warnings
                    .push(format!("Ubisoft cache product {id}: {e}")),
            }
        }
    }
    result.games.sort_by_key(|g| (g.install_id, g.id));
    Ok(result)
}
