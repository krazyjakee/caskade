//! Only the discovery-live profile opts into real, temporary HKCU state.
#![cfg(windows)]
use caskade_discovery::registry::*;
use std::time::{SystemTime, UNIX_EPOCH};
use winreg::{RegKey, RegValue, enums::*};

#[test]
#[ignore = "real Windows HKCU fixture parity; discovery-live profile"]
fn live_hkcu_matches_the_same_source_read_from_wine_fixture() {
    assert!(
        std::env::var_os("CASKADE_TEST_NATIVE").is_some(),
        "set CASKADE_TEST_NATIVE=1 for real registry tests"
    );
    let path = format!(
        r"Software\CaskadeDiscoveryFixture\{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    struct Cleanup(String);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let parent = RegKey::predef(HKEY_CURRENT_USER);
            for view in [KEY_WOW64_32KEY, KEY_WOW64_64KEY] {
                if let Ok(key) = parent.open_subkey_with_flags(&self.0, KEY_READ | KEY_WRITE | view)
                {
                    let _ = key.delete_subkey_all("");
                }
            }
            let _ = parent.delete_subkey_all(&self.0);
        }
    }
    let _cleanup = Cleanup(path.clone());
    let native = NativeRegistry::default();
    for view in [RegistryView::Registry32, RegistryView::Registry64] {
        let flag = if view == RegistryView::Registry32 {
            KEY_WOW64_32KEY
        } else {
            KEY_WOW64_64KEY
        };
        let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey_with_flags(&path, KEY_READ | KEY_WRITE | flag)
            .unwrap();
        let records = [
            (
                "",
                REG_SZ,
                "default\0"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect(),
            ),
            (
                "text",
                REG_SZ,
                "游戏 🎮\0"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect(),
            ),
            (
                "expand",
                REG_EXPAND_SZ,
                "%USERPROFILE%\\Games\0"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect(),
            ),
            (
                "multi",
                REG_MULTI_SZ,
                "one\0two\0\0"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect(),
            ),
            ("dword", REG_DWORD, u32::MAX.to_le_bytes().to_vec()),
            ("qword", REG_QWORD, u64::MAX.to_le_bytes().to_vec()),
            ("binary", REG_BINARY, vec![0, 1, 255]),
        ];
        let mut text = format!(
            "WINE REGISTRY Version 2\n[{}]\n",
            path.replace('\\', "\\\\")
        );
        for (name, kind, bytes) in records {
            key.set_raw_value(
                name,
                &RegValue {
                    bytes: bytes.clone().into(),
                    vtype: kind.clone(),
                },
            )
            .unwrap();
            text.push_str(&format!(
                "\"{name}\"=hex({:x}):{}\n",
                kind as u32,
                bytes
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        let fixture =
            WineRegistry::parse(&text, RegistryHive::CurrentUser, view, Limits::default()).unwrap();
        let key = RegistryKey::new(RegistryHive::CurrentUser, view, &path);
        // This is the same read-only source against both implementations.
        fn source(registry: &dyn Registry, key: &RegistryKey) -> Vec<RegistryEntry> {
            registry.values(key).unwrap()
        }
        assert_eq!(source(&native, &key), source(&fixture, &key));
        assert!(native.key_exists(&key).unwrap());
        assert!(
            native
                .open_key(RegistryHive::CurrentUser, view, &path)
                .unwrap()
                .is_some()
        );
    }
}
