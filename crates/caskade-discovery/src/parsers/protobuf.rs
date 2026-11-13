//! Handwritten prost messages for the discovery subset of Battle.net product.db.
//! Wire field numbers are format facts checked against overtools/TACTLib's
//! proto_database descriptor. No generated or launcher implementation code is ported.
use super::{Limits, ParseError, Result};
use prost::Message;
use serde::{Deserialize, Serialize};
#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct ProductDb {
    #[prost(message, repeated, tag = "1")]
    pub products: Vec<ProductInstall>,
}
#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct ProductInstall {
    #[prost(string, tag = "1")]
    pub uid: String,
    #[prost(string, tag = "2")]
    pub product_code: String,
    #[prost(message, optional, tag = "3")]
    pub settings: Option<UserSettings>,
    #[prost(message, optional, tag = "4")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_product_state: Option<CachedProductState>,
}
#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct CachedProductState {
    #[prost(message, optional, tag = "1")]
    pub base_product_state: Option<BaseProductState>,
}
#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct BaseProductState {
    #[prost(bool, tag = "1")]
    pub installed: bool,
    #[prost(bool, tag = "2")]
    pub playable: bool,
}
#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct UserSettings {
    #[prost(string, tag = "1")]
    pub install_path: String,
    #[prost(string, tag = "2")]
    pub play_region: String,
    #[prost(string, tag = "6")]
    pub text_language: String,
    #[prost(string, tag = "7")]
    pub speech_language: String,
}
fn varint(bytes: &[u8], pos: &mut usize) -> Result<u64> {
    let mut value = 0;
    for shift in (0..70).step_by(7) {
        let b = *bytes.get(*pos).ok_or(ParseError::Truncated(*pos))?;
        *pos += 1;
        if shift == 63 && b > 1 {
            return Err(ParseError::Invalid("protobuf varint overflow".into()));
        }
        value |= u64::from(b & 127) << shift;
        if b & 128 == 0 {
            return Ok(value);
        }
    }
    Err(ParseError::Invalid("protobuf varint".into()))
}
/// Walks a message before prost allocates it, within `limits`. `children`
/// names the message kind of a length-delimited field of a `kind` message,
/// or `None` for plain bytes; the top-level message is kind 0.
pub(crate) fn preflight(
    bytes: &[u8],
    kind: u8,
    depth: usize,
    count: &mut usize,
    limits: &Limits,
    children: fn(u8, u64) -> Option<u8>,
) -> Result<()> {
    limits.check(depth, limits.max_depth.min(128), "nesting depth")?;
    let mut pos: usize = 0;
    while pos < bytes.len() {
        *count += 1;
        limits.check(*count, limits.max_nodes, "protobuf fields")?;
        let key = varint(bytes, &mut pos)?;
        let field = key >> 3;
        if field == 0 || field > 0x1fff_ffff {
            return Err(ParseError::Invalid("protobuf field number".into()));
        }
        let n = match key & 7 {
            0 => {
                varint(bytes, &mut pos)?;
                continue;
            }
            1 => 8,
            5 => 4,
            2 => usize::try_from(varint(bytes, &mut pos)?)
                .map_err(|_| ParseError::Invalid("protobuf length".into()))?,
            _ => return Err(ParseError::Unsupported("protobuf groups/wire type".into())),
        };
        let end = pos.checked_add(n).ok_or(ParseError::Truncated(pos))?;
        let data = bytes.get(pos..end).ok_or(ParseError::Truncated(pos))?;
        if key & 7 == 2 {
            if let Some(child_kind) = children(kind, field) {
                preflight(data, child_kind, depth + 1, count, limits, children)?;
            } else {
                limits.check(n, limits.max_string_bytes, "protobuf field bytes")?;
            }
        }
        pos = end;
    }
    Ok(())
}
pub fn parse_product_db(bytes: &[u8], limits: &Limits) -> Result<ProductDb> {
    limits.input(bytes)?;
    preflight(bytes, 0, 0, &mut 0, limits, |kind, field| {
        match (kind, field) {
            (0, 1) => Some(1), // ProductDb -> ProductInstall
            (1, 3) => Some(2), // ProductInstall -> UserSettings
            (1, 4) => Some(3), // ProductInstall -> CachedProductState
            (3, 1) => Some(4), // CachedProductState -> BaseProductState
            _ => None,
        }
    })?;
    ProductDb::decode(bytes).map_err(|e| ParseError::Invalid(e.to_string()))
}
