use caskade_discovery::{HostEnv, registry::*};
use std::{fs, sync::Arc};

const TEXT: &str = r#"WINE REGISTRY Version 2
#arch=win64
[Software\\Caskade\\Empty] 1700000000
#time=1da000000000000
[Software\\Caskade\\Typed] 1700000000
@="default"
"Text"="C:\\Games\\\x6e38\x620f \xd83c\xdfae"
"Expand"=str(2):"%USERPROFILE%\\Games"
"Multi"=str(7):"one\0two\0\0"
"Count"=dword:ffffffff
"Big"=hex(b):08,07,06,05,04,03,02,01
"Blob"=hex:00,ff,\
  01,02
[Software\\Wow6432Node\\Caskade\\Typed]
"Text"="32 bit"
"Expand"=hex(2):25,00,41,00,25,00,00,00
"Multi"=hex(7):41,00,00,00,42,00,00,00,00,00
[Software\\Caskade\\\x6e38\x620f]
"\x540d"="unicode key"
"broken"=hex:zz
"unsupported"=hex(99):00
"invalid utf16"=hex(1):00,d8
"bad dword"=hex(4):00
"after"="still readable"
"#;

fn key(view: RegistryView, path: &str) -> RegistryKey {
    RegistryKey::new(RegistryHive::LocalMachine, view, path)
}
fn values(registry: &dyn Registry, view: RegistryView) -> Vec<RegistryEntry> {
    registry
        .values(&key(view, r"software\CASKADE\typed"))
        .unwrap()
}

#[test]
fn wine_types_views_empty_keys_unicode_and_subkeys() {
    let registry = WineRegistry::parse(
        TEXT,
        RegistryHive::LocalMachine,
        RegistryView::Native,
        Limits::default(),
    )
    .unwrap();
    assert_eq!(
        registry
            .value(
                &key(RegistryView::Registry64, r"SOFTWARE\CASKADE\TYPED"),
                "TEXT"
            )
            .unwrap(),
        Some(RegistryValue::String("C:\\Games\\游戏 🎮".into()))
    );
    let values = values(&registry, RegistryView::Registry64);
    assert_eq!(values.len(), 7);
    for (name, value) in [
        ("", RegistryValue::String("default".into())),
        (
            "Expand",
            RegistryValue::ExpandString(r"%USERPROFILE%\Games".into()),
        ),
        (
            "Multi",
            RegistryValue::MultiString(vec!["one".into(), "two".into()]),
        ),
        ("Count", RegistryValue::Dword(u32::MAX)),
        ("Big", RegistryValue::Qword(0x0102030405060708)),
        ("Blob", RegistryValue::Binary(vec![0, 255, 1, 2])),
    ] {
        assert_eq!(
            registry
                .value(
                    &key(RegistryView::Registry64, r"Software\Caskade\Typed"),
                    name
                )
                .unwrap(),
            Some(value)
        );
    }
    let redirected = key(
        RegistryView::Registry64,
        r"Software\Wow6432Node\Caskade\Typed",
    );
    assert_eq!(
        registry.values(&redirected).unwrap(),
        values_for_32(&registry)
    );
    assert_eq!(
        registry.value(&redirected, "Expand").unwrap(),
        Some(RegistryValue::ExpandString("%A%".into()))
    );
    assert_eq!(
        registry.value(&redirected, "Multi").unwrap(),
        Some(RegistryValue::MultiString(vec!["A".into(), "B".into()]))
    );
    assert!(
        registry
            .key_exists(&key(RegistryView::Registry64, r"Software\Caskade\Empty"))
            .unwrap()
    );
    assert_eq!(
        registry
            .subkeys(&key(RegistryView::Registry64, r"Software\Caskade"))
            .unwrap(),
        ["Empty", "Typed", "游戏"]
    );
    let unicode = key(RegistryView::Registry64, r"Software\Caskade\游戏");
    assert_eq!(
        registry.value(&unicode, "名").unwrap(),
        Some(RegistryValue::String("unicode key".into()))
    );
    assert_eq!(
        registry.value(&unicode, "after").unwrap(),
        Some(RegistryValue::String("still readable".into()))
    );
    assert!(registry.malformed_records >= 4);
    assert!(
        registry
            .open_key(
                RegistryHive::LocalMachine,
                RegistryView::Registry64,
                "absent"
            )
            .unwrap()
            .is_none()
    );
}
fn values_for_32(registry: &dyn Registry) -> Vec<RegistryEntry> {
    values(registry, RegistryView::Registry32)
}

#[test]
fn win32_arch_native_view_and_duplicate_case_records() {
    let text = "WINE REGISTRY Version 2\n#arch=win32\n[Software\\\\Caskade]\n\"Name\"=\"old\"\n[software\\\\CASKADE]\n\"NAME\"=\"new\"\n";
    let registry = WineRegistry::parse(
        text,
        RegistryHive::CurrentUser,
        RegistryView::Native,
        Limits::default(),
    )
    .unwrap();
    let key = RegistryKey::new(
        RegistryHive::CurrentUser,
        RegistryView::Registry32,
        r"Software\Caskade",
    );
    assert_eq!(
        registry.values(&key).unwrap(),
        vec![RegistryEntry {
            name: "NAME".into(),
            value: RegistryValue::String("new".into())
        }]
    );
    assert_eq!(
        registry
            .values(&RegistryKey {
                view: RegistryView::Native,
                ..key.clone()
            })
            .unwrap(),
        registry.values(&key).unwrap()
    );
    assert!(
        !registry
            .key_exists(&RegistryKey {
                view: RegistryView::Registry64,
                ..key
            })
            .unwrap()
    );
}

#[test]
fn fixture_registry_is_automatic_and_never_falls_back_to_live() {
    let tmp = crate::fixtures::tempdir();
    fs::create_dir(tmp.path().join("root")).unwrap();
    fs::create_dir(tmp.path().join("registry")).unwrap();
    fs::write(tmp.path().join("registry/hklm64.reg"), TEXT).unwrap();
    fs::write(
        tmp.path().join("registry/hkcu.reg"),
        "WINE REGISTRY Version 2\n[Software\\\\Fixture]\n@=\"user\"\n",
    )
    .unwrap();
    let host = HostEnv::fixture(tmp.path()).unwrap();
    assert_eq!(values(host.registry(), RegistryView::Registry64).len(), 7);
    assert_eq!(values(host.registry(), RegistryView::Registry32).len(), 3);
    assert!(
        host.registry()
            .key_exists(&RegistryKey::new(
                RegistryHive::CurrentUser,
                RegistryView::Registry64,
                r"Software\Fixture"
            ))
            .unwrap()
    );
    let mut fake = MemoryRegistry::new();
    fake.insert(
        key(RegistryView::Registry64, "Fake"),
        "Value",
        RegistryValue::Dword(42),
    );
    let host = host.with_registry(Arc::new(fake));
    assert_eq!(
        host.registry()
            .value(&key(RegistryView::Registry64, "Fake"), "value")
            .unwrap(),
        Some(RegistryValue::Dword(42))
    );
    assert!(values(host.registry(), RegistryView::Registry64).is_empty());
}

#[test]
fn prefix_loader_reads_all_hives_and_default_user() {
    let tmp = crate::fixtures::tempdir();
    for name in ["system.reg", "user.reg", "userdef.reg"] {
        fs::write(
            tmp.path().join(name),
            "WINE REGISTRY Version 2\n[Software\\\\Fixture]\n@=\"value\"\n",
        )
        .unwrap();
    }
    let registry = WineRegistry::from_prefix(tmp.path(), Limits::default()).unwrap();
    for (hive, path) in [
        (RegistryHive::LocalMachine, r"Software\Fixture"),
        (RegistryHive::CurrentUser, r"Software\Fixture"),
        (RegistryHive::Users, r".Default\Software\Fixture"),
    ] {
        assert_eq!(
            registry
                .value(&RegistryKey::new(hive, RegistryView::Registry64, path), "")
                .unwrap(),
            Some(RegistryValue::String("value".into()))
        );
    }
}

#[test]
fn committed_fresh_wine_and_proton_prefixes_read_identical_authored_values() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/formats/registry66");
    let mut expected = None;
    for engine in ["wine", "proton"] {
        let prefix = root.join(engine);
        let before = [
            fs::read(prefix.join("system.reg")).unwrap(),
            fs::read(prefix.join("user.reg")).unwrap(),
        ];
        let registry = WineRegistry::from_prefix(&prefix, Limits::default()).unwrap();
        assert_eq!(registry.malformed_records, 0);
        for hive in [RegistryHive::LocalMachine, RegistryHive::CurrentUser] {
            let key = RegistryKey::new(
                hive,
                RegistryView::Registry32,
                r"Software\CaskadeDiscoveryFixture",
            );
            let values = registry.values(&key).unwrap();
            assert_eq!(values.len(), 6);
            assert_eq!(
                registry.value(&key, "installpath").unwrap(),
                Some(RegistryValue::String(
                    "C:\\Users\\fixture\\Games\\游戏 🎮".into()
                ))
            );
            assert_eq!(
                registry.value(&key, "expand").unwrap(),
                Some(RegistryValue::ExpandString(r"%USERPROFILE%\Games".into()))
            );
            assert_eq!(
                registry.value(&key, "multi").unwrap(),
                Some(RegistryValue::MultiString(vec!["one".into(), "two".into()]))
            );
            assert_eq!(
                registry.value(&key, "count").unwrap(),
                Some(RegistryValue::Dword(0x12345678))
            );
            assert_eq!(
                registry.value(&key, "big").unwrap(),
                Some(RegistryValue::Qword(0x0102030405060708))
            );
            assert_eq!(
                registry.value(&key, "blob").unwrap(),
                Some(RegistryValue::Binary(vec![0, 1, 255]))
            );
            assert_eq!(registry.subkeys(&key).unwrap(), ["Empty"]);
            if let Some(expected) = &expected {
                assert_eq!(&values, expected)
            } else {
                expected = Some(values)
            }
        }
        assert_eq!(
            before,
            [
                fs::read(prefix.join("system.reg")).unwrap(),
                fs::read(prefix.join("user.reg")).unwrap()
            ]
        );
    }
}

#[test]
fn registry_slashes_are_literal_key_characters_and_unicode_names_ignore_case() {
    let registry = WineRegistry::parse(
        "[Software\\\\A/B]\n\"Ä\"=\"value\"\n",
        RegistryHive::CurrentUser,
        RegistryView::Native,
        Limits::default(),
    )
    .unwrap();
    let key = RegistryKey::new(
        RegistryHive::CurrentUser,
        RegistryView::Native,
        r"Software\A/B",
    );
    assert_eq!(
        registry.value(&key, "ä").unwrap(),
        Some(RegistryValue::String("value".into()))
    );
    assert!(
        !registry
            .key_exists(&RegistryKey::new(
                RegistryHive::CurrentUser,
                RegistryView::Native,
                r"Software\A\B"
            ))
            .unwrap()
    );
}

#[test]
fn malformed_files_limits_and_fixture_symlinks_are_errors() {
    let tmp = crate::fixtures::tempdir();
    fs::create_dir(tmp.path().join("root")).unwrap();
    fs::create_dir(tmp.path().join("registry")).unwrap();
    fs::write(tmp.path().join("registry/hkcu.reg"), [0xff]).unwrap();
    assert!(HostEnv::fixture(tmp.path()).is_err());
    for limits in [
        Limits {
            input_bytes: 1,
            ..Limits::default()
        },
        Limits {
            line_bytes: 4,
            ..Limits::default()
        },
        Limits {
            value_bytes: 1,
            ..Limits::default()
        },
        Limits {
            output_bytes: 10,
            ..Limits::default()
        },
        Limits {
            keys: 1,
            ..Limits::default()
        },
        Limits {
            values: 1,
            ..Limits::default()
        },
    ] {
        assert!(
            WineRegistry::parse(
                TEXT,
                RegistryHive::LocalMachine,
                RegistryView::Native,
                limits
            )
            .is_err()
        );
    }
    #[cfg(unix)]
    {
        fs::remove_file(tmp.path().join("registry/hkcu.reg")).unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        fs::write(outside.path(), TEXT).unwrap();
        std::os::unix::fs::symlink(outside.path(), tmp.path().join("registry/hkcu.reg")).unwrap();
        assert!(HostEnv::fixture(tmp.path()).is_err());
    }
}

proptest::proptest! {
    #[test]
    fn arbitrary_registry_bytes_are_bounded(bytes in proptest::collection::vec(proptest::prelude::any::<u8>(),0..8192)) {
        let text=String::from_utf8_lossy(&bytes);
        let limits=Limits{input_bytes:16384,line_bytes:1024,value_bytes:512,output_bytes:8192,keys:32,values:64};
        let _=WineRegistry::parse(&text,RegistryHive::CurrentUser,RegistryView::Native,limits);
    }
}

#[test]
fn implicit_parent_keys_and_depth_are_bounded_before_snapshot_expansion() {
    let text = format!(
        "[{}]\n",
        std::iter::repeat_n("part", 129)
            .collect::<Vec<_>>()
            .join("\\\\")
    );
    assert!(
        WineRegistry::parse(
            &text,
            RegistryHive::CurrentUser,
            RegistryView::Native,
            Limits::default()
        )
        .is_err()
    );
    let text = "[A\\\\B\\\\C]\n";
    assert!(
        WineRegistry::parse(
            text,
            RegistryHive::CurrentUser,
            RegistryView::Native,
            Limits {
                keys: 2,
                ..Limits::default()
            }
        )
        .is_err()
    );
}
