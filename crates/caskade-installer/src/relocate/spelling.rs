//! Path spellings of a moved game and their byte-level replacement.
use crate::scan::{Encoding, Needle, find};

/// Spelling pairs for a host path change: as given, `\`/`/`, `\\`-escaped,
/// `file://` and Wine `Z:\` forms. Windows forms match without ASCII case.
pub fn spelling_pairs(old: &str, new: &str) -> Vec<(String, String)> {
    let forms = |path: &str| -> Vec<String> {
        let path = path.trim_end_matches(['/', '\\']);
        let windows = if path.starts_with('/') {
            format!("Z:{}", path.replace('/', "\\"))
        } else {
            path.replace('/', "\\")
        };
        vec![
            path.to_owned(),
            windows.clone(),
            windows.replace('\\', "/"),
            windows.replace('\\', "\\\\"),
            format!("file:///{}", windows.replace('\\', "/").replace(' ', "%20")),
            if path.starts_with('/') {
                format!("file://{}", path.replace(' ', "%20"))
            } else {
                format!("file:///{}", windows.replace('\\', "/").replace(' ', "%20"))
            },
        ]
    };
    let mut pairs: Vec<_> = forms(old).into_iter().zip(forms(new)).collect();
    // Canonical Windows paths are verbatim (\\?\C:\..., \\?\UNC\...), while
    // installers record ordinary ones: pair the plain spellings as well.
    let (plain_old, plain_new) = (plain_windows(old), plain_windows(new));
    if plain_old != old || plain_new != new {
        pairs.extend(forms(&plain_old).into_iter().zip(forms(&plain_new)));
    }
    pairs.sort();
    pairs.dedup();
    // Longest first, so an escaped form is not half-replaced by a shorter one.
    pairs.sort_by_key(|(old, _)| std::cmp::Reverse(old.len()));
    pairs
}

/// Strip a Windows verbatim prefix: `\\?\C:\x` → `C:\x`,
/// `\\?\UNC\server\share` → `\\server\share`; other paths are unchanged.
pub fn plain_windows(path: &str) -> String {
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = path.strip_prefix(r"\\?\") {
        rest.to_owned()
    } else {
        path.to_owned()
    }
}

/// Boundary-safe replacement of every spelling in UTF-8, UTF-16LE and
/// Windows-1252; `None` when nothing matched.
pub fn replace_all(bytes: &[u8], spellings: &[(String, String)]) -> Option<Vec<u8>> {
    let mut current = bytes.to_vec();
    let mut changed = false;
    for (old, new) in spellings {
        let ignore_case = old.contains('\\') || old.starts_with("file:") || old.contains(":/");
        let encodings: [(Encoding, Vec<u8>, Vec<u8>); 3] = [
            (
                Encoding::Utf8,
                old.as_bytes().to_vec(),
                new.as_bytes().to_vec(),
            ),
            (
                Encoding::Utf16Le,
                old.encode_utf16().flat_map(u16::to_le_bytes).collect(),
                new.encode_utf16().flat_map(u16::to_le_bytes).collect(),
            ),
            (
                Encoding::Ansi,
                encoding_rs::WINDOWS_1252.encode(old).0.into_owned(),
                encoding_rs::WINDOWS_1252.encode(new).0.into_owned(),
            ),
        ];
        for (encoding, from, to) in encodings {
            if encoding == Encoding::Ansi && old.is_ascii() {
                continue;
            }
            let needle = Needle {
                bytes: from.clone(),
                encoding,
                ignore_case,
            };
            let mut output = Vec::with_capacity(current.len());
            let mut rest = current.as_slice();
            let mut hit = false;
            while let Some(index) = find(rest, &needle) {
                output.extend_from_slice(&rest[..index]);
                output.extend_from_slice(&to);
                rest = &rest[index + from.len()..];
                hit = true;
            }
            if hit {
                output.extend_from_slice(rest);
                current = output;
                changed = true;
            }
        }
    }
    changed.then_some(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbatim_windows_paths_also_rewrite_their_plain_spellings() {
        assert_eq!(plain_windows(r"\\?\C:\Games\X"), r"C:\Games\X");
        assert_eq!(plain_windows(r"\\?\UNC\nas\games\X"), r"\\nas\games\X");
        assert_eq!(plain_windows("/home/x"), "/home/x");
        let pairs = spelling_pairs(r"\\?\C:\Games\X", r"\\?\D:\Games\X");
        let text = "InstallPath=C:\\Games\\X\r\nSave=c:\\games\\x\\save\r\n";
        assert_eq!(
            String::from_utf8(replace_all(text.as_bytes(), &pairs).unwrap()).unwrap(),
            "InstallPath=D:\\Games\\X\r\nSave=D:\\Games\\X\\save\r\n"
        );
    }
}
