//! Read-only Shell Link reader, written from Microsoft's MS-SHLLINK specification.
//! Unknown ExtraData blocks are skipped by their validated spans. Known-folder
//! GUIDs and environment paths are returned as data, never resolved on this host.
use super::{Limits, ParseError, Parsed, Result, cursor::Cursor};
use serde::{Deserialize, Serialize};

const CLSID: [u8; 16] = [1, 20, 2, 0, 0, 0, 0, 0, 192, 0, 0, 0, 0, 0, 0, 70];
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shortcut {
    pub target: Option<String>,
    pub relative_path: Option<String>,
    pub arguments: Vec<String>,
    pub raw_arguments: Option<String>,
    pub working_dir: Option<String>,
    pub icon: Option<String>,
    pub icon_index: i32,
    pub description: Option<String>,
    pub known_folders: Vec<KnownFolder>,
    pub environment_target: Option<String>,
    pub id_list: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KnownFolder {
    pub guid: String,
    pub id_list_offset: u32,
}
fn u32_at(data: &[u8], offset: usize, limits: &Limits) -> Result<u32> {
    let mut c = Cursor::new(data, limits);
    c.pos = offset;
    c.u32()
}
fn string_at(
    data: &[u8],
    offset: usize,
    wide: bool,
    minimum: usize,
    limits: &Limits,
    encoding: &'static encoding_rs::Encoding,
) -> Result<String> {
    if offset < minimum || offset >= data.len() {
        return Err(ParseError::Invalid("link string offset".into()));
    }
    let mut c = Cursor::new(data, limits);
    c.pos = offset;
    if wide {
        let mut words = Vec::new();
        loop {
            let word = c.u16()?;
            if word == 0 {
                break;
            }
            limits.check(
                words.len().saturating_add(1).saturating_mul(2),
                limits.max_string_bytes,
                "string bytes",
            )?;
            words.push(word);
        }
        String::from_utf16(&words).map_err(|_| ParseError::Encoding)
    } else {
        let tail = &data[offset..];
        let n = tail
            .iter()
            .take(limits.max_string_bytes.saturating_add(1))
            .position(|x| *x == 0)
            .ok_or(ParseError::Truncated(offset))?;
        ansi(&tail[..n], encoding)
    }
}
fn ansi(bytes: &[u8], encoding: &'static encoding_rs::Encoding) -> Result<String> {
    encoding
        .decode_without_bom_handling_and_without_replacement(bytes)
        .map(|s| s.into_owned())
        .ok_or(ParseError::Encoding)
}
fn counted(
    c: &mut Cursor<'_>,
    wide: bool,
    encoding: &'static encoding_rs::Encoding,
) -> Result<String> {
    let n = c.u16()? as usize;
    c.limits
        .check(n, c.limits.max_string_bytes, "string bytes")?;
    if wide {
        c.utf16(n)
    } else {
        ansi(c.take(n)?, encoding)
    }
}
fn join(base: &str, suffix: &str) -> String {
    if suffix.is_empty() || base.ends_with(suffix) {
        base.to_owned()
    } else {
        format!(
            "{}\\{}",
            base.trim_end_matches('\\'),
            suffix.trim_start_matches('\\')
        )
    }
}
fn link_info(
    data: &[u8],
    limits: &Limits,
    encoding: &'static encoding_rs::Encoding,
) -> Result<String> {
    let header = u32_at(data, 4, limits)? as usize;
    if (header != 0x1c && header < 0x24) || header > data.len() {
        return Err(ParseError::Invalid("LinkInfo header size".into()));
    }
    let flags = u32_at(data, 8, limits)?;
    let unicode_suffix = if header >= 0x24 {
        u32_at(data, 32, limits)? as usize
    } else {
        0
    };
    let suffix_off = if unicode_suffix != 0 {
        unicode_suffix
    } else {
        u32_at(data, 24, limits)? as usize
    };
    let suffix = string_at(
        data,
        suffix_off,
        unicode_suffix != 0,
        header,
        limits,
        encoding,
    )?;
    if flags & 1 != 0 {
        let volume_off = u32_at(data, 12, limits)? as usize;
        if volume_off < header {
            return Err(ParseError::Invalid("VolumeID offset".into()));
        }
        let volume_size = u32_at(data, volume_off, limits)? as usize;
        if volume_size < 16
            || volume_off
                .checked_add(volume_size)
                .is_none_or(|end| end > data.len())
        {
            return Err(ParseError::Invalid("VolumeID span".into()));
        }
        let unicode_base = if header >= 0x24 {
            u32_at(data, 28, limits)? as usize
        } else {
            0
        };
        let off = if unicode_base != 0 {
            unicode_base
        } else {
            u32_at(data, 16, limits)? as usize
        };
        let base = string_at(data, off, unicode_base != 0, header, limits, encoding)?;
        Ok(join(&base, &suffix))
    } else if flags & 2 != 0 {
        let off = u32_at(data, 20, limits)? as usize;
        if off < header {
            return Err(ParseError::Invalid("network offset".into()));
        }
        let size = u32_at(data, off, limits)? as usize;
        let end = off
            .checked_add(size)
            .ok_or_else(|| ParseError::Invalid("network span".into()))?;
        let network = data.get(off..end).ok_or(ParseError::Truncated(off))?;
        if size < 20 {
            return Err(ParseError::Invalid("network size".into()));
        }
        let name_off = u32_at(network, 8, limits)? as usize;
        let (name_off, wide, min) = if name_off > 20 {
            (u32_at(network, 20, limits)? as usize, true, 28)
        } else {
            (name_off, false, 20)
        };
        Ok(join(
            &string_at(network, name_off, wide, min, limits, encoding)?,
            &suffix,
        ))
    } else {
        Err(ParseError::Invalid("LinkInfo has no target".into()))
    }
}
/// Parse arguments using Windows backslash-before-quote rules, without shell expansion.
pub fn split_arguments(text: &str) -> Result<Vec<String>> {
    if text.contains('\0') {
        return Err(ParseError::Invalid("NUL in arguments".into()));
    }
    let mut args = Vec::new();
    let mut chars = text.chars().peekable();
    while chars.peek().is_some() {
        while chars.peek().is_some_and(|c| matches!(c, ' ' | '\t')) {
            chars.next();
        }
        if chars.peek().is_none() {
            break;
        }
        let mut out = String::new();
        let mut quoted = false;
        loop {
            let mut slashes = 0;
            while chars.peek() == Some(&'\\') {
                chars.next();
                slashes += 1;
            }
            if chars.peek() == Some(&'"') {
                for _ in 0..slashes / 2 {
                    out.push('\\');
                }
                chars.next();
                if slashes % 2 == 1 {
                    out.push('"');
                } else if quoted && chars.peek() == Some(&'"') {
                    out.push('"');
                    chars.next();
                } else {
                    quoted = !quoted;
                }
            } else {
                for _ in 0..slashes {
                    out.push('\\');
                }
                match chars.peek() {
                    None => break,
                    Some(' ' | '\t') if !quoted => break,
                    _ => out.push(chars.next().expect("peeked")),
                }
            }
        }
        // Windows accepts an unmatched final quote as extending to end-of-line.
        args.push(out);
    }
    Ok(args)
}
pub fn parse(bytes: &[u8], limits: &Limits) -> Result<Parsed<Shortcut>> {
    parse_with_encoding(bytes, limits, encoding_rs::WINDOWS_1252)
}
/// ANSI Shell Links carry no code-page identifier. The source may inject its
/// known Windows code page; the default reader uses Windows-1252.
pub fn parse_with_encoding(
    bytes: &[u8],
    limits: &Limits,
    encoding: &'static encoding_rs::Encoding,
) -> Result<Parsed<Shortcut>> {
    limits.input(bytes)?;
    let mut c = Cursor::new(bytes, limits);
    if c.u32()? != 76 || c.take(16)? != CLSID {
        return Err(ParseError::Invalid("Shell Link header".into()));
    }
    let flags = c.u32()?;
    c.take(32)?;
    let icon_index = c.u32()? as i32;
    c.take(16)?;
    let mut out = Shortcut {
        target: None,
        relative_path: None,
        arguments: Vec::new(),
        raw_arguments: None,
        working_dir: None,
        icon: None,
        icon_index,
        description: None,
        known_folders: Vec::new(),
        environment_target: None,
        id_list: Vec::new(),
    };
    if flags & 1 != 0 {
        let n = c.u16()? as usize;
        let raw = c.take(n)?;
        let mut ids = Cursor::new(raw, limits);
        let mut count: usize = 0;
        loop {
            let size = ids.u16()? as usize;
            if size == 0 {
                break;
            }
            if size < 2 {
                return Err(ParseError::Invalid("ItemID size".into()));
            }
            count += 1;
            limits.check(count, limits.max_nodes, "ItemIDs")?;
            ids.take(size - 2)?;
        }
        if ids.pos != n {
            return Err(ParseError::Invalid("IDList terminator".into()));
        }
        out.id_list = raw.to_vec();
    }
    if flags & 2 != 0 {
        let start = c.pos;
        let size = c.u32()? as usize;
        if size < 0x1c {
            return Err(ParseError::Invalid("LinkInfo size".into()));
        }
        c.pos = start;
        out.target = Some(link_info(c.take(size)?, limits, encoding)?);
    }
    let wide = flags & 128 != 0;
    for (flag, field) in [
        (4, &mut out.description),
        (8, &mut out.relative_path),
        (16, &mut out.working_dir),
        (32, &mut out.raw_arguments),
        (64, &mut out.icon),
    ] {
        if flags & flag != 0 {
            *field = Some(counted(&mut c, wide, encoding)?);
        }
    }
    let mut warnings = Vec::new();
    let mut blocks: usize = 0;
    loop {
        let start = c.pos;
        let size = c.u32()? as usize;
        if size == 0 {
            break;
        }
        if size < 8 {
            return Err(ParseError::Invalid("ExtraData size".into()));
        }
        blocks += 1;
        limits.check(blocks, limits.max_nodes, "ExtraData blocks")?;
        c.pos = start;
        let block = c.take(size)?;
        match u32_at(block, 4, limits)? {
            0xa000_000b => {
                if size != 28 {
                    return Err(ParseError::Invalid("KnownFolderDataBlock size".into()));
                }
                let g = &block[8..24];
                let guid = format!(
                    "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
                    u32::from_le_bytes(g[..4].try_into().expect("four")),
                    u16::from_le_bytes(g[4..6].try_into().expect("two")),
                    u16::from_le_bytes(g[6..8].try_into().expect("two")),
                    g[8],
                    g[9],
                    g[10],
                    g[11],
                    g[12],
                    g[13],
                    g[14],
                    g[15]
                );
                let id_list_offset = u32_at(block, 24, limits)?;
                if id_list_offset as usize >= out.id_list.len() {
                    return Err(ParseError::Invalid("KnownFolder IDList offset".into()));
                }
                out.known_folders.push(KnownFolder {
                    guid,
                    id_list_offset,
                });
            }
            sig @ (0xa000_0001 | 0xa000_0007) => {
                if size != 788 {
                    return Err(ParseError::Invalid("environment block size".into()));
                }
                let s = if block[268..270] != [0, 0] {
                    string_at(block, 268, true, 8, limits, encoding)?
                } else {
                    string_at(&block[..268], 8, false, 8, limits, encoding)?
                };
                if sig == 0xa000_0001 {
                    out.environment_target = Some(s);
                } else {
                    out.icon = Some(s);
                }
            }
            signature => warnings.push(format!("ignored ExtraData block {signature:#x}")),
        }
    }
    if c.pos != bytes.len() {
        return Err(ParseError::Invalid("trailing Shell Link bytes".into()));
    }
    if flags & (1 << 25) != 0 && out.environment_target.is_some() {
        out.target.clone_from(&out.environment_target);
    }
    if out.target.is_none() {
        out.target = out
            .environment_target
            .clone()
            .or_else(|| out.relative_path.clone());
    }
    if let Some(raw) = &out.raw_arguments {
        out.arguments = split_arguments(raw)?;
    }
    Ok(Parsed {
        value: out,
        warnings,
    })
}
