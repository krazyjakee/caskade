//! A bounded subset of AppStream metainfo: id, localized name and categories.
//! No DTD, external entity, include, network access or recursive deserialization.
use quick_xml::{Reader, events::Event};
use std::io;
#[derive(Debug, Default)]
pub(super) struct MetaInfo {
    pub id: String,
    pub title: Option<String>,
    pub categories: Vec<String>,
}
fn invalid(message: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}
pub(super) fn parse(bytes: &[u8], locale: &str) -> io::Result<MetaInfo> {
    if bytes.len() > 1024 * 1024 {
        return Err(invalid("metainfo byte limit exceeded"));
    }
    std::str::from_utf8(bytes).map_err(invalid)?;
    let mut reader = Reader::from_reader(bytes);
    let mut stack: Vec<String> = vec![];
    let mut info = MetaInfo::default();
    let mut events = 0;
    let mut text = String::new();
    let locale = locale.split('.').next().unwrap_or(locale).replace('_', "-");
    let language = locale.split('-').next().unwrap_or(&locale);
    let mut name_rank = 0;
    let mut selected_rank = 0;
    let mut root_seen = false;
    loop {
        events += 1;
        if events > 20_000 {
            return Err(invalid("metainfo event limit exceeded"));
        }
        match reader.read_event().map_err(invalid)? {
            Event::Start(start) => {
                if stack.len() >= 32 {
                    return Err(invalid("metainfo nesting limit exceeded"));
                }
                let tag = start.name().as_ref().to_owned();
                if stack.is_empty() {
                    if root_seen || !matches!(tag.as_str(), "component" | "application") {
                        return Err(invalid("expected one metainfo component"));
                    }
                    root_seen = true;
                }
                if stack.len() == 1 && tag == "name" {
                    name_rank = 1;
                    for attribute in start.attributes() {
                        let attribute = attribute.map_err(invalid)?;
                        if attribute.key.as_ref() == "xml:lang" {
                            let value = attribute.value.replace('_', "-");
                            name_rank = if value == locale {
                                3
                            } else if value == language {
                                2
                            } else {
                                0
                            };
                        }
                    }
                }
                stack.push(tag);
                text.clear();
            }
            Event::Text(value) => text.push_str(value.as_ref()),
            Event::CData(value) => text.push_str(value.as_ref()),
            Event::GeneralRef(value) => {
                let name = value.as_ref();
                let value = match name {
                    "amp" => '&',
                    "lt" => '<',
                    "gt" => '>',
                    "quot" => '"',
                    "apos" => '\'',
                    name => {
                        let code = if let Some(hex) = name.strip_prefix("#x") {
                            u32::from_str_radix(hex, 16).ok()
                        } else if let Some(dec) = name.strip_prefix('#') {
                            dec.parse().ok()
                        } else {
                            None
                        };
                        code.and_then(char::from_u32)
                            .ok_or_else(|| invalid("unknown metainfo entity"))?
                    }
                };
                text.push(value);
            }
            Event::End(_) => {
                let tag = stack.pop().ok_or_else(|| invalid("unbalanced metainfo"))?;
                if stack.len() == 1 {
                    if tag == "id" {
                        info.id = text.trim().to_owned();
                    } else if tag == "name" && name_rank > selected_rank && !text.trim().is_empty()
                    {
                        info.title = Some(text.trim().to_owned());
                        selected_rank = name_rank;
                    }
                } else if tag == "category"
                    && stack.len() == 2
                    && stack.last().is_some_and(|s| s == "categories")
                {
                    if info.categories.len() >= 256 {
                        return Err(invalid("metainfo category limit exceeded"));
                    }
                    info.categories.push(text.trim().to_owned());
                }
                text.clear();
            }
            Event::DocType(_) => return Err(invalid("metainfo DTD is not supported")),
            Event::Eof => break,
            _ => (),
        }
        if text.len() > 16_384 {
            return Err(invalid("metainfo text limit exceeded"));
        }
    }
    if !root_seen || !stack.is_empty() {
        return Err(invalid("incomplete metainfo"));
    }
    Ok(info)
}
