//! Bounded GDK/AppX metadata, interpreted without loading package code.
//! Schema reference: Microsoft's MicrosoftGame.config and AppX identity docs.
use quick_xml::{Reader, XmlVersion, events::Event};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Identity {
    pub name: String,
    pub publisher: String,
}
impl Identity {
    pub fn family(&self) -> io::Result<String> {
        if !(3..=50).contains(&self.name.len())
            || !self
                .name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-'))
            || self.publisher.is_empty()
            || self.publisher.encode_utf16().count() > 8192
            || self.publisher.chars().any(char::is_control)
        {
            return Err(invalid("invalid package identity"));
        }
        // PublisherId: the first 64 SHA-256 bits of the UTF-16LE publisher,
        // encoded as thirteen Crockford base32 digits (one trailing zero bit).
        let bytes: Vec<_> = self
            .publisher
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        let digest = Sha256::digest(bytes);
        let value = (u64::from_be_bytes(digest[..8].try_into().expect("SHA-256 has eight bytes"))
            as u128)
            << 1;
        let alphabet = b"0123456789abcdefghjkmnpqrstvwxyz";
        let suffix: String = (0..13)
            .map(|i| alphabet[((value >> (60 - i * 5)) & 31) as usize] as char)
            .collect();
        Ok(format!("{}_{suffix}", self.name))
    }
}
#[derive(Debug, Default)]
pub(super) struct Executable {
    pub path: String,
    pub id: String,
    pub pc: bool,
    pub dev: bool,
}
#[derive(Debug, Default)]
pub(super) struct GameConfig {
    pub identity: Identity,
    pub title: String,
    pub store_id: String,
    pub logo: String,
    pub executables: Vec<Executable>,
}
#[derive(Debug, Default)]
pub(super) struct Application {
    pub id: String,
    pub executable: String,
    pub title: String,
}
#[derive(Debug, Default)]
pub(super) struct Appx {
    pub identity: Identity,
    pub applications: Vec<Application>,
}
pub(super) fn invalid(message: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}
fn identity(attrs: &BTreeMap<String, String>) -> Identity {
    Identity {
        name: get(attrs, "Name"),
        publisher: get(attrs, "Publisher"),
    }
}
fn get(attrs: &BTreeMap<String, String>, key: &str) -> String {
    attrs.get(key).cloned().unwrap_or_default()
}
pub(super) fn game(bytes: &[u8]) -> io::Result<GameConfig> {
    let mut out = GameConfig::default();
    let mut identity_seen = false;
    xml(bytes, "Game", |path, attrs, text| {
        match path {
            [root, tag] if root == "Game" && tag == "Identity" => {
                if identity_seen {
                    return Err(invalid("duplicate game identity"));
                }
                identity_seen = true;
                out.identity = identity(attrs);
            }
            [_, tag] if tag == "ShellVisuals" => {
                out.title = get(attrs, "DefaultDisplayName");
                out.logo = get(attrs, "Square150x150Logo");
            }
            [_, list, tag] if list == "ExecutableList" && tag == "Executable" => {
                if out.executables.len() >= 128 {
                    return Err(invalid("executable limit"));
                }
                let family = get(attrs, "TargetDeviceFamily");
                out.executables.push(Executable {
                    path: get(attrs, "Name"),
                    id: get(attrs, "Id"),
                    pc: family.is_empty() || family.eq_ignore_ascii_case("PC"),
                    dev: get(attrs, "IsDevOnly").eq_ignore_ascii_case("true"),
                });
            }
            [_, tag] if tag == "StoreId" => out.store_id = text.trim().to_owned(),
            _ => (),
        }
        Ok(())
    })?;
    out.identity.family()?;
    Ok(out)
}
pub(super) fn appx(bytes: &[u8]) -> io::Result<Appx> {
    let mut out = Appx::default();
    let mut identity_seen = false;
    let mut pending_title = String::new();
    xml(bytes, "Package", |path, attrs, _| {
        match path {
            [_, tag] if tag == "Identity" => {
                if identity_seen {
                    return Err(invalid("duplicate AppX identity"));
                }
                identity_seen = true;
                out.identity = identity(attrs);
            }
            [_, list, tag] if list == "Applications" && tag == "Application" => {
                if out.applications.len() >= 128 {
                    return Err(invalid("application limit"));
                }
                out.applications.push(Application {
                    id: get(attrs, "Id"),
                    executable: get(attrs, "Executable"),
                    title: std::mem::take(&mut pending_title),
                });
            }
            [_, list, app, tag]
                if list == "Applications" && app == "Application" && tag == "VisualElements" =>
            {
                pending_title = get(attrs, "DisplayName");
            }
            _ => (),
        }
        Ok(())
    })?;
    out.identity.family()?;
    Ok(out)
}

/// Decode only XML encodings used by Windows package metadata. No replacement
/// characters, DTDs, includes, or recursive deserialization.
fn decode(bytes: &[u8]) -> io::Result<String> {
    if bytes.len() > 1024 * 1024 {
        return Err(invalid("XML byte limit"));
    }
    let (data, little) = if let Some(b) = bytes.strip_prefix(&[0xff, 0xfe]) {
        (b, true)
    } else if let Some(b) = bytes.strip_prefix(&[0xfe, 0xff]) {
        (b, false)
    } else if bytes.starts_with(&[b'<', 0]) {
        (bytes, true)
    } else if bytes.starts_with(&[0, b'<']) {
        (bytes, false)
    } else {
        return std::str::from_utf8(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes))
            .map(str::to_owned)
            .map_err(invalid);
    };
    if data.len() % 2 != 0 {
        return Err(invalid("odd UTF-16 XML length"));
    }
    let units: Vec<_> = data
        .as_chunks::<2>()
        .0
        .iter()
        .map(|v| {
            if little {
                u16::from_le_bytes([v[0], v[1]])
            } else {
                u16::from_be_bytes([v[0], v[1]])
            }
        })
        .collect();
    String::from_utf16(&units).map_err(invalid)
}
struct Element {
    tag: String,
    attrs: BTreeMap<String, String>,
    text: String,
}
fn xml(
    bytes: &[u8],
    root: &str,
    mut visit: impl FnMut(&[String], &BTreeMap<String, String>, &str) -> io::Result<()>,
) -> io::Result<()> {
    let decoded = decode(bytes)?;
    let mut reader = Reader::from_str(&decoded);
    reader.config_mut().expand_empty_elements = true;
    let mut stack: Vec<Element> = vec![];
    let mut root_seen = false;
    for _ in 0..20_000 {
        match reader.read_event().map_err(invalid)? {
            Event::Start(start) | Event::Empty(start) => {
                // Reader expands empty elements, so this branch is Start only.
                if stack.len() >= 32 {
                    return Err(invalid("XML depth limit"));
                }
                let tag = start
                    .name()
                    .as_ref()
                    .rsplit(':')
                    .next()
                    .unwrap_or_default()
                    .to_owned();
                if stack.is_empty() {
                    if root_seen || tag != root {
                        return Err(invalid("unexpected XML root"));
                    }
                    root_seen = true;
                }
                let mut attrs = BTreeMap::new();
                for attribute in start.attributes() {
                    let attr = attribute.map_err(invalid)?;
                    if attrs.len() >= 128 || attr.value.len() > 16_384 {
                        return Err(invalid("XML attribute limit"));
                    }
                    let key = attr.key.as_ref().to_owned();
                    let value = attr
                        .normalized_value(XmlVersion::Explicit1_0)
                        .map_err(invalid)?
                        .into_owned();
                    if attrs.insert(key, value).is_some() {
                        return Err(invalid("duplicate XML attribute"));
                    }
                }
                stack.push(Element {
                    tag,
                    attrs,
                    text: String::new(),
                });
            }
            Event::Text(text) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(text.as_ref());
                } else if !text.as_ref().trim().is_empty() {
                    return Err(invalid("text outside XML root"));
                }
            }
            Event::CData(text) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(text.as_ref());
                }
            }
            Event::GeneralRef(value) => {
                let reference = format!("&{};", value.as_ref());
                let value = quick_xml::escape::unescape(&reference).map_err(invalid)?;
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&value);
                }
            }
            Event::End(_) => {
                let path: Vec<_> = stack.iter().map(|e| e.tag.clone()).collect();
                let element = stack.pop().ok_or_else(|| invalid("unbalanced XML"))?;
                visit(&path, &element.attrs, &element.text)?;
            }
            Event::DocType(_) => return Err(invalid("XML DTD is forbidden")),
            Event::Eof => {
                return if root_seen && stack.is_empty() {
                    Ok(())
                } else {
                    Err(invalid("incomplete XML"))
                };
            }
            _ => (),
        }
        if stack.last().is_some_and(|e| e.text.len() > 16_384) {
            return Err(invalid("XML text limit"));
        }
    }
    Err(invalid("XML event limit"))
}
