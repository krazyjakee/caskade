//! #62 T1: independent generated spellings and byte-preserving rewrites.
use caskade_installer::{
    relocate::{replace_all, spelling_pairs},
    scan::{find, needles},
};
use proptest::prelude::*;

fn encode(text: &str, encoding: u8) -> Vec<u8> {
    match encoding {
        0 => text.as_bytes().to_vec(),
        1 => text.encode_utf16().flat_map(u16::to_le_bytes).collect(),
        2 => {
            let (bytes, _, unmappable) = encoding_rs::WINDOWS_1252.encode(text);
            assert!(!unmappable);
            bytes.into_owned()
        }
        _ => unreachable!(),
    }
}

// Construct the examples independently of needles/spelling_pairs, so a form
// omitted by both implementations still fails the contract.
fn windows_forms(path: &str) -> Vec<String> {
    vec![
        path.into(),
        path.replace('\\', "/"),
        path.replace('\\', "\\\\"),
        format!("file:///{}", path.replace('\\', "/").replace(' ', "%20")),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(192))]

    #[test]
    fn generated_variants_are_found_in_every_supported_encoding(
        name in "[A-Za-z][A-Za-z0-9 ]{0,16}[A-Za-z]",
        encoding in 0u8..3,
        lowercase in any::<bool>(),
        verbatim in any::<bool>(),
    ) {
        let original = format!("C:\\Jeux Café\\{name}");
        let short = format!("C:\\JEUXCA~1\\{}~1", name.replace(' ', ""));
        let mut forms = windows_forms(&original);
        forms.extend([short.clone(), short.replace('\\', "\\\\")]);
        for mut form in forms {
            if lowercase {
                form.make_ascii_lowercase();
            }
            let haystack = encode(&format!("prefix=\"{form}\\data\"\r\n"), encoding);
            let scanner_path = if verbatim { format!(r"\\?\{original}") } else { original.clone() };
            prop_assert!(needles(&scanner_path, std::slice::from_ref(&short))
                .iter().any(|needle| find(&haystack, needle).is_some()),
                "missing form {form:?}, encoding {encoding}");
        }
    }

    #[test]
    fn rewriting_changes_only_complete_path_occurrences(
        name in "[A-Za-z][A-Za-z0-9 ]{0,16}[A-Za-z]",
        sibling in "[A-Za-z0-9_.~-]{1,8}",
        encoding in 0u8..3,
        form in 0usize..4,
        verbatim in any::<bool>(),
    ) {
        let old = format!("C:\\Jeux Café\\{name}");
        let new = format!("D:\\Autres Jeux\\{name}");
        let old_form = &windows_forms(&old)[form];
        let new_form = &windows_forms(&new)[form];
        let before = format!("é préfixe\r\n\"{old_form}\\save\"\n{old_form}{sibling}\\keep\r\n{old_form};fin é");
        let expected = format!("é préfixe\r\n\"{new_form}\\save\"\n{old_form}{sibling}\\keep\r\n{new_form};fin é");
        let pairs = if verbatim {
            spelling_pairs(&format!(r"\\?\{old}"), &format!(r"\\?\{new}"))
        } else { spelling_pairs(&old, &new) };
        let actual = replace_all(&encode(&before, encoding), &pairs);
        let encoded_expected = encode(&expected, encoding);
        prop_assert_eq!(actual.as_deref(), Some(encoded_expected.as_slice()));
        prop_assert_eq!(replace_all(&encode(&expected, encoding), &pairs), None);
    }

    #[test]
    fn unix_native_url_and_wine_forms_follow_the_same_move(
        name in "[A-Za-z][A-Za-z0-9 ]{0,12}[A-Za-z]",
        encoding in 0u8..2,
    ) {
        let old = format!("/mnt/old/{name}");
        let new = format!("/mnt/new/{name}");
        let mut forms = vec![(old.clone(), new.clone()), (
            format!("file://{}", old.replace(' ', "%20")),
            format!("file://{}", new.replace(' ', "%20")),
        )];
        forms.extend(windows_forms(&format!("Z:{}", old.replace('/', "\\")))
            .into_iter().zip(windows_forms(&format!("Z:{}", new.replace('/', "\\")))));
        for (from, to) in forms {
            let bytes = encode(&format!("path=\"{from}/data\"\r\n"), encoding);
            prop_assert!(needles(&old, &[]).iter().any(|needle| find(&bytes, needle).is_some()));
            prop_assert_eq!(replace_all(&bytes, &spelling_pairs(&old, &new)),
                Some(encode(&format!("path=\"{to}/data\"\r\n"), encoding)));
        }
    }
}

#[test]
fn unicode_siblings_are_never_partially_rewritten() {
    let pairs = spelling_pairs(r"C:\Games\Foo", r"D:\Games\Foo");
    for suffix in ["bar", "-backup", ".old", "_2", "~1", "é", "遊戲"] {
        for encoding in 0..2 {
            let bytes = encode(&format!(r"C:\Games\Foo{suffix}\data"), encoding);
            assert_eq!(replace_all(&bytes, &pairs), None, "{suffix}");
        }
    }
}

#[test]
fn utf16_unicode_path_units_are_not_ascii_case_folded_byte_by_byte() {
    // U+4100 and U+6100 are different CJK characters; their UTF-16 high
    // bytes happen to be ASCII A/a. Windows ASCII case matching must not
    // turn one directory into the other.
    let old = "C:\\Games\\\u{4100}";
    let other = encode("C:\\Games\\\u{6100}\\data", 1);
    assert!(
        !needles(old, &[])
            .iter()
            .any(|needle| find(&other, needle).is_some())
    );
    assert_eq!(
        replace_all(&other, &spelling_pairs(old, r"D:\Games\Moved")),
        None
    );
}
