use super::{Result, ScanError, extension, path_text};
use std::path::{Path, PathBuf};
/// Parse a playlist without IO. References stay confined to the scan folder.
/// CUE grammar/sanitisation is shared with the installer's multipart resolver.
pub fn playlist_references(path: &Path, text: &str) -> Result<Vec<PathBuf>> {
    let parent = path.parent().unwrap_or(Path::new(""));
    let names: Vec<String> = match extension(path).as_str() {
        "cue" => caskade_installer::input::parse_cue_references(text)
            .map_err(|e| ScanError::Invalid(e.to_string()))?
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
        "m3u" | "m3u8" => text
            .trim_start_matches('\u{feff}')
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty() && !s.starts_with('#'))
            .map(|s| s.trim_matches('"').to_string())
            .collect(),
        "gdi" => {
            let mut lines = text.lines().filter(|l| !l.trim().is_empty());
            let count = lines
                .next()
                .and_then(|l| l.trim().parse::<usize>().ok())
                .ok_or_else(|| ScanError::Invalid(crate::tr!("invalid GDI track count")))?;
            if count > 10_000 {
                return Err(ScanError::Limit("GDI tracks"));
            }
            let mut names = Vec::new();
            for line in lines {
                let fields = words(line)?;
                if fields.len() != 6 || fields[0].parse::<u32>().is_err() {
                    return Err(ScanError::Invalid(crate::tr!("invalid GDI track record")));
                }
                names.push(fields[4].clone());
            }
            if names.len() != count {
                return Err(ScanError::Invalid(crate::tr!("GDI track count mismatch")));
            }
            names
        }
        _ => return Ok(vec![]),
    };
    if names.is_empty() {
        return Err(ScanError::Invalid(crate::tr!("empty playlist")));
    }
    let mut paths = Vec::new();
    for name in names {
        let name = name.replace('\\', "/");
        // Lexically normalise parent references inside the selected scanner,
        // rejecting absolute/drive paths before joining (including on Linux).
        if name.starts_with('/') || name.contains(':') || name.contains('\0') {
            return Err(ScanError::Invalid(crate::tr!(
                "absolute playlist reference"
            )));
        }
        let mut parts: Vec<String> = parent
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        for part in name.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    if parts.pop().is_none() {
                        return Err(ScanError::Invalid(crate::tr!(
                            "playlist reference escapes scanner"
                        )));
                    }
                }
                p => parts.push(p.into()),
            }
        }
        let path = PathBuf::from(path_text(Path::new(&parts.join("/")))?);
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    Ok(paths)
}
fn words(line: &str) -> Result<Vec<String>> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut quote = false;
    for c in line.chars() {
        match c {
            '"' => quote = !quote,
            c if c.is_whitespace() && !quote => {
                if !current.is_empty() {
                    result.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if quote {
        return Err(ScanError::Invalid(crate::tr!("unterminated GDI filename")));
    }
    if !current.is_empty() {
        result.push(current);
    }
    Ok(result)
}
