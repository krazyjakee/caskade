//! Byte-level spellings of a path and boundary-safe searching.
use encoding_rs::WINDOWS_1252;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Encoding {
    Utf8,
    Utf16Le,
    /// Windows-1252, the usual system ANSI code page for Western locales.
    Ansi,
}

/// One searchable spelling: exact bytes, and whether ASCII case is ignored.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Needle {
    pub bytes: Vec<u8>,
    pub encoding: Encoding,
    pub ignore_case: bool,
}

/// Textual forms of `path` a program may have written: as given, with `\`
/// and `/` separators, `\\`-escaped, as a `file://` URL, and Wine's `Z:\`
/// form for absolute Unix paths. Windows-style forms compare without ASCII
/// case. `extra` adds caller-known spellings such as 8.3 short names.
pub fn needles(path: &str, extra: &[String]) -> Vec<Needle> {
    let mut forms: BTreeSet<(String, bool)> = BTreeSet::new();
    let trimmed = path.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        return vec![];
    }
    // Every textual form of one spelling of the path.
    let mut add = |spelling: &str| {
        let unix = spelling.starts_with('/');
        let windows = if unix {
            format!("Z:{}", spelling.replace('/', "\\"))
        } else {
            spelling.replace('/', "\\")
        };
        forms.insert((spelling.to_owned(), !unix));
        forms.insert((windows.clone(), true));
        forms.insert((windows.replace('\\', "/"), true));
        forms.insert((windows.replace('\\', "\\\\"), true));
        let url = windows.replace('\\', "/").replace(' ', "%20");
        forms.insert((format!("file:///{url}"), true));
        if unix {
            forms.insert((format!("file://{}", spelling.replace(' ', "%20")), false));
        }
    };
    add(trimmed);
    // Canonical Windows paths are verbatim; installers write plain ones, in
    // every form.
    let plain = crate::relocate::plain_windows(trimmed);
    if plain != trimmed {
        add(&plain);
    }
    for form in extra {
        let form = form.trim_end_matches(['/', '\\']);
        if !form.is_empty() {
            forms.insert((form.to_owned(), true));
            forms.insert((form.replace('\\', "\\\\"), true));
        }
    }
    let mut needles = BTreeSet::new();
    for (form, ignore_case) in forms {
        needles.insert(Needle {
            bytes: form.as_bytes().to_vec(),
            encoding: Encoding::Utf8,
            ignore_case,
        });
        needles.insert(Needle {
            bytes: form.encode_utf16().flat_map(u16::to_le_bytes).collect(),
            encoding: Encoding::Utf16Le,
            ignore_case,
        });
        let (ansi, _, unmappable) = WINDOWS_1252.encode(&form);
        if !unmappable && !form.is_ascii() {
            needles.insert(Needle {
                bytes: ansi.into_owned(),
                encoding: Encoding::Ansi,
                ignore_case,
            });
        }
    }
    needles.into_iter().collect()
}

/// The first boundary-safe occurrence of `needle` in `haystack`: the match
/// must not continue a path component, so `…\Foo` never matches `…\Foobar`.
pub fn find(haystack: &[u8], needle: &Needle) -> Option<usize> {
    let width = if needle.encoding == Encoding::Utf16Le {
        2
    } else {
        1
    };
    let length = needle.bytes.len();
    if length == 0 || haystack.len() < length {
        return None;
    }
    let first = needle.bytes[0];
    (0..=haystack.len() - length)
        .filter(|start| same(haystack[*start], first, needle.ignore_case))
        .find(|start| {
            let candidate = &haystack[*start..*start + length];
            let equal = if width == 2 {
                // Compare whole UTF-16 code units: ASCII case folds only when
                // both units are ASCII, so CJK units with 0x41/0x61 high bytes
                // stay distinct.
                candidate
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .zip(needle.bytes.as_chunks::<2>().0)
                    .all(|(a, b)| {
                        if needle.ignore_case && a[1] == 0 && b[1] == 0 {
                            a[0].eq_ignore_ascii_case(&b[0])
                        } else {
                            a == b
                        }
                    })
            } else {
                candidate
                    .iter()
                    .zip(&needle.bytes)
                    .all(|(a, b)| same(*a, *b, needle.ignore_case))
            };
            equal && boundary(&haystack[*start + length..], width)
        })
}

fn same(a: u8, b: u8, ignore_case: bool) -> bool {
    if ignore_case {
        a.eq_ignore_ascii_case(&b)
    } else {
        a == b
    }
}

/// End of input, a separator or a delimiter. Letters, digits, `_ - . ~` and
/// non-ASCII characters would continue a file name, so they are not
/// boundaries. Spaces count as boundaries: a false positive only makes the
/// verdict more cautious.
fn boundary(rest: &[u8], width: usize) -> bool {
    let Some(unit) = rest.get(..width) else {
        return true;
    };
    let (low, high) = (unit[0], if width == 2 { unit[1] } else { 0 });
    if high != 0 || low >= 0x80 {
        return false;
    }
    !(low.is_ascii_alphanumeric() || matches!(low, b'_' | b'-' | b'.' | b'~'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(haystack: &[u8], path: &str) -> bool {
        needles(path, &[])
            .iter()
            .any(|needle| find(haystack, needle).is_some())
    }

    #[test]
    fn forms_cover_separators_escapes_urls_wine_and_encodings() {
        let path = r"C:\Games\Indie Game";
        assert!(hit(br"InstallPath=C:\Games\Indie Game", path));
        assert!(hit(br#"{"dir": "C:\\Games\\Indie Game\\data"}"#, path));
        assert!(hit(b"path=c:/games/indie game/save", path));
        assert!(hit(b"<a href=\"file:///C:/Games/Indie%20Game/x\">", path));
        let utf16: Vec<u8> = "x=C:\\GAMES\\Indie Game\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert!(hit(&utf16, path));
        assert!(hit(br"Z:\home\me\Games\Foo\bin", "/home/me/Games/Foo"));
        assert!(hit(b"/home/me/Games/Foo/bin", "/home/me/Games/Foo"));
        assert!(hit(br"InstallPath=C:\Games\Foo", r"\\?\C:\Games\Foo"));
        // Unix paths are case-sensitive in their native form.
        assert!(!hit(b"/HOME/me/Games/Foo/bin", "/home/me/Games/Foo"));
        let cafe = "C:\\Jeux\\Café";
        let (ansi, _, _) = WINDOWS_1252.encode(cafe);
        assert!(hit(&ansi, cafe));
    }

    #[test]
    fn utf16_units_fold_case_only_when_ascii() {
        let units =
            |text: &str| -> Vec<u8> { text.encode_utf16().flat_map(u16::to_le_bytes).collect() };
        // U+4100 and U+6100 differ only by a 0x20 bit in the high byte.
        let needle = Needle {
            bytes: units("C:\\Games\\\u{4100}"),
            encoding: Encoding::Utf16Le,
            ignore_case: true,
        };
        assert!(find(&units("C:\\Games\\\u{6100}"), &needle).is_none());
        assert!(find(&units("c:\\GAMES\\\u{4100}"), &needle).is_some());
    }

    #[test]
    fn matches_respect_component_boundaries() {
        let path = r"C:\Games\Foo";
        assert!(!hit(br"C:\Games\Foobar\x", path));
        assert!(!hit(br"C:\Games\Foo-2", path));
        assert!(!hit(br"C:\Games\Foo.bak", path));
        assert!(hit(br"C:\Games\Foo", path));
        assert!(hit(br#""C:\Games\Foo""#, path));
        assert!(hit(br"C:\Games\Foo;D:\x", path));
        let utf16: Vec<u8> = "C:\\Games\\Foobar"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert!(!hit(&utf16, path));
    }

    proptest::proptest! {
        #![proptest_config(proptest::test_runner::Config::with_cases(256))]
        #[test]
        fn a_longer_sibling_component_never_matches(
            base in "[A-Za-z][A-Za-z0-9 ]{0,12}",
            suffix in "[A-Za-z0-9_.~-]{1,8}",
            tail in "(\\\\[a-z]{1,4}){0,2}",
        ) {
            let path = format!("C:\\Games\\{base}");
            let text = format!("x=C:\\Games\\{base}{suffix}{tail}\n");
            proptest::prop_assert!(!hit(text.as_bytes(), &path));
            let exact = format!("x=C:\\Games\\{base}{tail}\n");
            proptest::prop_assert!(hit(exact.as_bytes(), &path));
        }
    }
}
