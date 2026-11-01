//! Bounded Wine registry reader shared by installation and discovery.
//! Original implementation; Wine's registry format is used as a data format
//! reference only. This module never opens files or touches the host registry.
use crate::scan;
use std::io;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub input_bytes: usize,
    pub line_bytes: usize,
    pub value_bytes: usize,
    pub output_bytes: usize,
    pub keys: usize,
    pub values: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            input_bytes: 64 * 1024 * 1024,
            line_bytes: 4 * 1024 * 1024,
            value_bytes: 1024 * 1024,
            output_bytes: 128 * 1024 * 1024,
            keys: 100_000,
            values: 1_000_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Architecture {
    Win32,
    Win64,
}

#[derive(Debug, Default)]
pub struct Document {
    /// Includes empty keys, in file order. Repeated sections are retained.
    pub keys: Vec<String>,
    pub values: Vec<scan::RegistryValue>,
    pub architecture: Option<Architecture>,
    /// Malformed individual records are skipped, without allocating warnings.
    pub malformed_records: usize,
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Parse Wine version 2 text. Also accepts regedit version 5 records (as
/// written by [`super::reg_file`]) and headerless sections.
/// `#time` and section timestamps are metadata and never interpreted as values.
/// A hard limit returns an error rather than a misleading partial document.
pub fn parse(text: &str, hive: &str, limits: Limits) -> io::Result<Document> {
    if text.len() > limits.input_bytes {
        return Err(invalid("registry input byte limit exceeded"));
    }
    let regedit = text
        .trim_start_matches('\u{feff}')
        .starts_with("Windows Registry Editor");
    let mut document = Document::default();
    let mut current: Option<String> = None;
    let mut total = 0usize;
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let mut line = line.trim().to_owned();
        if line.len() > limits.line_bytes {
            return Err(invalid("registry line byte limit exceeded"));
        }
        // Wine/regedit hex continuations contain no quoted strings.
        while line.ends_with('\\')
            && !line.starts_with('[')
            && line
                .split_once('=')
                .is_some_and(|(_, value)| value.trim_start().starts_with("hex"))
        {
            line.pop();
            let Some(next) = lines.next() else { break };
            let next = next.trim();
            if next.len() > limits.line_bytes.saturating_sub(line.len()) {
                return Err(invalid("registry line byte limit exceeded"));
            }
            line.push_str(next);
        }
        if let Some(arch) = line.strip_prefix("#arch=") {
            document.architecture = match arch.trim() {
                "win32" => Some(Architecture::Win32),
                "win64" => Some(Architecture::Win64),
                _ => {
                    document.malformed_records += 1;
                    None
                }
            };
            continue;
        }
        if let Some(rest) = line.strip_prefix('[') {
            current = rest.rsplit_once(']').and_then(|(name, _)| {
                // Deleted keys are not live keys in an imported file.
                (!name.starts_with('-')).then_some(name).and_then(|name| {
                    if regedit {
                        Some(name.to_owned())
                    } else {
                        decode(name)
                    }
                })
            });
            if let Some(key) = &current {
                if key.contains('\0') || document.keys.len() >= limits.keys {
                    return Err(invalid("registry key limit or invalid key"));
                }
                total = add_output(total, key.len(), limits)?;
                document.keys.push(key.clone());
            } else {
                document.malformed_records += 1;
            }
            continue;
        }
        if line.is_empty() || line.starts_with(['#', ';']) || !line.contains('=') {
            continue;
        }
        let Some(key) = &current else { continue };
        let record = record(&line, limits)?;
        let Some((name, value_type, data)) = record else {
            document.malformed_records += 1;
            continue;
        };
        if document.values.len() >= limits.values {
            return Err(invalid("registry value count limit exceeded"));
        }
        total = add_output(
            total,
            key.len() + name.len() + data.len() + hive.len(),
            limits,
        )?;
        document.values.push(scan::RegistryValue {
            hive: hive.into(),
            key: key.clone(),
            name,
            value_type: value_type.into(),
            data,
        });
    }
    Ok(document)
}

fn add_output(total: usize, amount: usize, limits: Limits) -> io::Result<usize> {
    let total = total
        .checked_add(amount)
        .ok_or_else(|| invalid("registry output overflow"))?;
    if total > limits.output_bytes {
        return Err(invalid("registry output byte limit exceeded"));
    }
    Ok(total)
}

type Record = (String, &'static str, Vec<u8>);
fn record(line: &str, limits: Limits) -> io::Result<Option<Record>> {
    let (name, rest) = if let Some(rest) = line.strip_prefix('@') {
        (String::new(), rest)
    } else {
        let Some((name, rest)) = quoted(line) else {
            return Ok(None);
        };
        (name, rest)
    };
    let Some(rest) = rest.trim_start().strip_prefix('=') else {
        return Ok(None);
    };
    let rest = rest.trim();
    let (kind, data) = if rest.starts_with('"') || rest.starts_with("str(") {
        let (kind, text) = if let Some(text) = rest.strip_prefix("str(2):") {
            ("REG_EXPAND_SZ", text)
        } else if let Some(text) = rest.strip_prefix("str(7):") {
            ("REG_MULTI_SZ", text)
        } else if let Some(text) = rest.strip_prefix("str(1):") {
            ("REG_SZ", text)
        } else {
            ("REG_SZ", rest)
        };
        let Some((value, suffix)) = quoted(text) else {
            return Ok(None);
        };
        if !suffix.trim().is_empty() {
            return Ok(None);
        }
        (
            kind,
            value
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
    } else if let Some(word) = rest.strip_prefix("dword:") {
        let Ok(word) = u32::from_str_radix(word.trim(), 16) else {
            return Ok(None);
        };
        ("REG_DWORD", word.to_le_bytes().to_vec())
    } else if let Some((kind, bytes)) = rest
        .split_once(':')
        .filter(|(kind, _)| kind.starts_with("hex"))
    {
        let kind = match kind {
            "hex" | "hex(3)" => "REG_BINARY",
            "hex(1)" => "REG_SZ",
            "hex(2)" => "REG_EXPAND_SZ",
            "hex(7)" => "REG_MULTI_SZ",
            "hex(4)" => "REG_DWORD",
            "hex(b)" => "REG_QWORD",
            _ => "REG_NONE",
        };
        let mut data = vec![];
        for byte in bytes
            .split(',')
            .map(str::trim)
            .filter(|byte| !byte.is_empty())
        {
            if data.len() >= limits.value_bytes {
                return Err(invalid("registry value byte limit exceeded"));
            }
            if byte.len() != 2 {
                return Ok(None);
            }
            let Ok(byte) = u8::from_str_radix(byte, 16) else {
                return Ok(None);
            };
            data.push(byte);
        }
        (kind, data)
    } else {
        return Ok(None);
    };
    if name.contains('\0') {
        return Ok(None);
    }
    if data.len() > limits.value_bytes {
        return Err(invalid("registry value byte limit exceeded"));
    }
    Ok(Some((name, kind, data)))
}

fn quoted(text: &str) -> Option<(String, &str)> {
    let text = text.strip_prefix('"')?;
    let mut escaped = false;
    for (index, ch) in text.char_indices() {
        if ch == '"' && !escaped {
            return Some((decode(&text[..index])?, &text[index + 1..]));
        }
        escaped = ch == '\\' && !escaped;
    }
    None
}

/// Decode into UTF-16 units first so paired Wine `\x` surrogate escapes work.
fn decode(text: &str) -> Option<String> {
    let mut units = vec![];
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            units.extend(ch.encode_utf16(&mut [0; 2]).iter().copied());
            continue;
        }
        let escaped = match chars.next()? {
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            '0' => '\0',
            'a' => '\u{7}',
            'b' => '\u{8}',
            'f' => '\u{c}',
            'v' => '\u{b}',
            'x' => {
                let mut value = 0u16;
                let mut count = 0;
                while count < 4 && chars.peek().is_some_and(char::is_ascii_hexdigit) {
                    value = value * 16 + chars.next()?.to_digit(16)? as u16;
                    count += 1;
                }
                if count == 0 {
                    return None;
                }
                units.push(value);
                continue;
            }
            other => other,
        };
        units.extend(escaped.encode_utf16(&mut [0; 2]).iter().copied());
    }
    String::from_utf16(&units).ok()
}
