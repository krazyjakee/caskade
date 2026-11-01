//! Shared reader regressions: discovery and installer scan raw data agree.
use caskade_installer::{
    RegistryEntry, RegistryHive, RegistryValue,
    runtime::registry::{
        self,
        read::{self, Limits},
    },
};

#[test]
fn regedit_writer_roundtrips_through_the_shared_reader() {
    let values = vec![
        RegistryValue::String("游戏 🎮".into()),
        RegistryValue::ExpandString(r"%USERPROFILE%\Games".into()),
        RegistryValue::MultiString(vec!["a".into(), "b".into()]),
        RegistryValue::Dword(u32::MAX),
        RegistryValue::Qword(u64::MAX),
        RegistryValue::Binary(vec![0, 255]),
    ];
    let records: Vec<_> = values
        .into_iter()
        .enumerate()
        .map(|(i, value)| RegistryEntry {
            hive: RegistryHive::LocalMachine,
            key: r"Software\Fixture".into(),
            name: format!("value{i}"),
            value,
        })
        .collect();
    let text = registry::reg_file(&records);
    let document = read::parse(&text, "HKLM", Limits::default()).unwrap();
    assert_eq!(document.values.len(), 6);
    assert_eq!(document.keys, [r"HKEY_LOCAL_MACHINE\Software\Fixture"]);
    assert_eq!(
        document
            .values
            .iter()
            .map(|v| v.value_type.as_str())
            .collect::<Vec<_>>(),
        [
            "REG_SZ",
            "REG_EXPAND_SZ",
            "REG_MULTI_SZ",
            "REG_DWORD",
            "REG_QWORD",
            "REG_BINARY"
        ]
    );
    assert_eq!(
        document.values[0].data,
        "游戏 🎮"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        registry::parse_prefix_reg(&text, "HKLM").unwrap(),
        document.values
    );
}

#[test]
fn raw_reader_preserves_string_bytes_and_surrogate_escapes() {
    let text = r#"WINE REGISTRY Version 2
#arch=win64
[Software\\\x6e38\x620f] 1700000000
#time=1da1747c66d0000
"name"="\xd83c\xdfae"
"expand"=str(2):"%A%"
"multi"=str(7):"a\0b\0"
"#;
    let document = read::parse(text, "HKCU", Limits::default()).unwrap();
    assert_eq!(document.keys, [r"Software\游戏"]);
    assert_eq!(
        document.values[0].data,
        "🎮"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    );
    assert_eq!(document.values[1].value_type, "REG_EXPAND_SZ");
    assert_eq!(
        document.values[2].data,
        "a\0b\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    );
    assert_eq!(document.architecture, Some(read::Architecture::Win64));
}

#[test]
fn continuation_expansion_and_repeated_keys_stop_at_limits() {
    let text = "WINE REGISTRY Version 2\n[K]\n\"v\"=hex:01,\\\n 02,\\\n 03\n";
    assert_eq!(
        read::parse(text, "HKCU", Limits::default()).unwrap().values[0].data,
        [1, 2, 3]
    );
    assert!(
        read::parse(
            text,
            "HKCU",
            Limits {
                line_bytes: 18,
                ..Limits::default()
            }
        )
        .is_err()
    );
    assert!(
        read::parse(
            "[K]\n[K]\n",
            "HKCU",
            Limits {
                keys: 1,
                ..Limits::default()
            }
        )
        .is_err()
    );
    // An invalid Unicode scalar or bad record never hides later valid values.
    let bad = "[K]\n\"bad\"=\"\\xd800\"\n\"good\"=dword:2a\n";
    let document = read::parse(bad, "HKCU", Limits::default()).unwrap();
    assert_eq!(document.malformed_records, 1);
    assert_eq!(document.values[0].data, 42u32.to_le_bytes());
}
