use caskade_discovery::parsers::{self, Limits, ParseError};
use proptest::prelude::*;
const LIMITS: Limits = Limits {
    max_input_bytes: 1024 * 1024,
    max_string_bytes: 64 * 1024,
    max_nodes: 4096,
    max_depth: 32,
};
fn sample(name: &str) -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/formats")
            .join(name),
    )
    .unwrap()
}

#[test]
fn committed_binary_vdf_goldens() {
    let shortcuts = parsers::binary_vdf::parse(&sample("shortcuts.vdf"), &LIMITS).unwrap();
    insta::assert_json_snapshot!("shortcuts", shortcuts);
    for (file, version) in [("appinfo-v28.vdf", 0x28), ("appinfo-v29.vdf", 0x29)] {
        let info = parsers::binary_vdf::parse_appinfo(&sample(file), &LIMITS).unwrap();
        assert_eq!(info.version, version);
        assert_eq!(info.apps[0].app_id, 620);
        insta::assert_json_snapshot!(file, info);
    }
}
#[test]
fn committed_shell_link_golden() {
    let link = parsers::lnk::parse(&sample("unicode.lnk"), &LIMITS).unwrap();
    assert_eq!(
        link.value.target.as_deref(),
        Some(r"C:\Games\Fixture Café\game.exe")
    );
    assert_eq!(link.value.arguments, ["--name", "Fixture Café", "--flag"]);
    assert_eq!(
        link.value.known_folders[0].guid,
        "905e63b6-c1bf-494e-b29c-65b732d3d21a"
    );
    insta::assert_json_snapshot!("shell_link", link);
}
#[test]
fn committed_text_formats_goldens() {
    let desktop = parsers::desktop::parse(&sample("fixture.desktop"), &LIMITS).unwrap();
    assert_eq!(
        desktop.localized("Name", "fr_FR.UTF-8"),
        Some("Casse-tête fictif")
    );
    assert_eq!(
        desktop.argv("fr", Some("/fixture.desktop")).unwrap(),
        [
            "env",
            "FIXTURE_MODE=1",
            "/home/fixture/Games/Fixture Puzzle/game",
            "--name",
            "Fixture Puzzle"
        ]
    );
    insta::assert_json_snapshot!("desktop", desktop);
    insta::assert_json_snapshot!(
        "url",
        parsers::url::parse(&sample("fixture.url"), &LIMITS).unwrap()
    );
    insta::assert_json_snapshot!(
        "plist",
        parsers::plist::parse(&sample("Info.plist"), &LIMITS).unwrap()
    );
    let vdf = parsers::text_vdf::parse(
        include_bytes!("fixtures/src/linux/appmanifest.acf"),
        &LIMITS,
    )
    .unwrap();
    insta::assert_json_snapshot!("text_vdf", vdf);
}
#[test]
fn committed_protobuf_golden_and_roundtrip() {
    use prost::Message;
    let db = parsers::protobuf::parse_product_db(&sample("product.db"), &LIMITS).unwrap();
    assert_eq!(
        db.products[0].settings.as_ref().unwrap().install_path,
        r"C:\Games\Fixture Strategy"
    );
    assert_eq!(
        parsers::protobuf::parse_product_db(&db.encode_to_vec(), &LIMITS).unwrap(),
        db
    );
    insta::assert_json_snapshot!("product_db", db);
}
#[test]
fn windows_arguments_are_data() {
    assert_eq!(
        parsers::lnk::split_arguments(r#"--name "a b" "" a\"b "$HOME;rm -rf /""#).unwrap(),
        ["--name", "a b", "", "a\"b", "$HOME;rm -rf /"]
    );
    assert!(parsers::lnk::split_arguments("a\0b").is_err());
}
#[test]
fn desktop_field_codes_preserve_argument_boundaries() {
    let entry = parsers::desktop::parse(
        br#"[Desktop Entry]
Name=Game %k
Name[fr_CA@formal]=Jeu
Icon=an icon.png
Exec="/a path/game" %c %k %i %U %% --literal=$HOME
TryExec=/a path/game
"#,
        &LIMITS,
    )
    .unwrap();
    assert_eq!(
        entry
            .argv("fr_CA.UTF-8@formal", Some("/a file.desktop"))
            .unwrap(),
        [
            "/a path/game",
            "Jeu",
            "/a file.desktop",
            "--icon",
            "an icon.png",
            "%",
            "--literal=$HOME"
        ]
    );
    assert_eq!(entry.try_exec(), Some("/a path/game"));
    for exec in ["game %Q", "game %", "game --file=%F", "game \"unclosed"] {
        let input = format!("[Desktop Entry]\nExec={exec}\n");
        assert!(
            parsers::desktop::parse(input.as_bytes(), &LIMITS)
                .unwrap()
                .argv("", None)
                .is_err()
        );
    }
}
#[test]
fn url_utf16_and_duplicate_keys() {
    let text = "[InternetShortcut]\r\nURL=wrong\r\nURL=steam://run/620\r\n";
    let bytes = [
        vec![255, 254],
        text.encode_utf16().flat_map(u16::to_le_bytes).collect(),
    ]
    .concat();
    assert_eq!(
        parsers::url::parse(&bytes, &LIMITS).unwrap().url,
        "steam://run/620"
    );
    assert!(parsers::url::parse(b"[InternetShortcut]\nIconIndex=bad", &LIMITS).is_err());
}
#[test]
fn vdf_includes_are_never_followed_and_depth_is_bounded() {
    let parsed = parsers::text_vdf::parse(
        b"#base \"/definitely/no/such/path\"\nRoot { key value key other }",
        &LIMITS,
    )
    .unwrap();
    assert_eq!(parsed.warnings.len(), 1);
    let text = format!("Root {{ {} {} }}", "x {".repeat(1000), "}".repeat(1000));
    assert!(matches!(
        parsers::text_vdf::parse(text.as_bytes(), &LIMITS),
        Err(ParseError::Limit { .. })
    ));
    let mut binary = vec![];
    for _ in 0..1000 {
        binary.extend_from_slice(b"\0x\0");
    }
    binary.extend(vec![8; 1001]);
    assert!(matches!(
        parsers::binary_vdf::parse(&binary, &LIMITS),
        Err(ParseError::Limit { .. })
    ));
}
#[test]
fn every_binary_fixture_prefix_fails_cleanly() {
    for file in [
        "shortcuts.vdf",
        "appinfo-v28.vdf",
        "appinfo-v29.vdf",
        "unicode.lnk",
        "product.db",
    ] {
        let bytes = sample(file);
        for n in 0..bytes.len() {
            let prefix = &bytes[..n];
            match file {
                "unicode.lnk" => {
                    assert!(parsers::lnk::parse(prefix, &LIMITS).is_err());
                }
                "shortcuts.vdf" => {
                    assert!(parsers::binary_vdf::parse(prefix, &LIMITS).is_err());
                }
                "product.db" => {
                    let _ = parsers::protobuf::parse_product_db(prefix, &LIMITS);
                }
                _ => {
                    assert!(parsers::binary_vdf::parse_appinfo(prefix, &LIMITS).is_err());
                }
            }
        }
    }
}
#[test]
fn plist_binary_roundtrip_and_cycle_rejection() {
    let original = parsers::plist::parse(&sample("Info.plist"), &LIMITS).unwrap();
    let mut binary = Vec::new();
    original.to_writer_binary(&mut binary).unwrap();
    assert_eq!(parsers::plist::parse(&binary, &LIMITS).unwrap(), original);
    // One array referencing itself. Header, object, offset table, trailer.
    let mut cycle = b"bplist00\xa1\x00\x08".to_vec();
    cycle.extend([0; 6]);
    cycle.extend([1, 1]);
    cycle.extend(1u64.to_be_bytes());
    cycle.extend(0u64.to_be_bytes());
    cycle.extend(10u64.to_be_bytes());
    assert!(parsers::plist::parse(&cycle, &LIMITS).is_err());
}
#[test]
fn ansi_shell_link_codepage_is_injected() {
    let text = r"C:\Games\テスト\game.exe";
    let (encoded, _, errors) = encoding_rs::SHIFT_JIS.encode(text);
    assert!(!errors);
    let mut link = vec![0; 76];
    link[..4].copy_from_slice(&76u32.to_le_bytes());
    link[4..20].copy_from_slice(&[1, 20, 2, 0, 0, 0, 0, 0, 192, 0, 0, 0, 0, 0, 0, 70]);
    link[20..24].copy_from_slice(&8u32.to_le_bytes());
    link.extend((encoded.len() as u16).to_le_bytes());
    link.extend_from_slice(&encoded);
    link.extend(0u32.to_le_bytes());
    let parsed = parsers::lnk::parse_with_encoding(&link, &LIMITS, encoding_rs::SHIFT_JIS).unwrap();
    assert_eq!(parsed.value.target.as_deref(), Some(text));
}
#[test]
fn malformed_link_offsets_and_counts_are_rejected() {
    let original = sample("unicode.lnk");
    for (at, value) in [(0, u32::MAX), (76, u32::MAX)] {
        let mut bytes = original.clone();
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        assert!(parsers::lnk::parse(&bytes, &LIMITS).is_err());
    }
    let mut bytes = original.clone();
    // IDList has an invalid one-byte ItemID.
    bytes[78..80].copy_from_slice(&1u16.to_le_bytes());
    assert!(parsers::lnk::parse(&bytes, &LIMITS).is_err());
}
#[test]
fn protobuf_unknown_fields_are_skipped_but_field_bombs_are_bounded() {
    let mut bytes = sample("product.db");
    bytes.extend([0x78, 0x01]);
    let parsed = parsers::protobuf::parse_product_db(&bytes, &LIMITS).unwrap();
    assert_eq!(parsed.products.len(), 1);
    let bomb = [0x0a, 0].repeat(LIMITS.max_nodes + 1);
    assert!(matches!(
        parsers::protobuf::parse_product_db(&bomb, &LIMITS),
        Err(ParseError::Limit { .. })
    ));
    assert!(
        parsers::protobuf::parse_product_db(&[0x0a, 0xff, 0xff, 0xff, 0xff, 0x7f], &LIMITS)
            .is_err()
    );
}
#[test]
fn binary_plist_shared_large_strings_cannot_expand_before_guard() {
    // One array with 100 references to one 100-byte string: small file, large tree.
    let mut data = b"bplist00".to_vec();
    data.extend([0xaf, 0x10, 100]);
    data.extend([1u8; 100]);
    let string_offset = data.len();
    data.extend([0x5f, 0x10, 100]);
    data.extend([b'x'; 100]);
    let table_offset = data.len();
    data.extend([8, string_offset as u8]);
    data.extend([0; 6]);
    data.extend([1, 1]);
    data.extend(2u64.to_be_bytes());
    data.extend(0u64.to_be_bytes());
    data.extend((table_offset as u64).to_be_bytes());
    let limits = Limits {
        max_input_bytes: 1024,
        ..LIMITS
    };
    assert!(matches!(
        parsers::plist::parse(&data, &limits),
        Err(ParseError::Limit { .. })
    ));
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn arbitrary_bytes_are_clean_errors_or_bounded_values(bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
        let limits = Limits { max_input_bytes: 4096, max_string_bytes: 2048, max_nodes: 256, max_depth: 16 };
        let _ = parsers::binary_vdf::parse(&bytes, &limits);
        let _ = parsers::binary_vdf::parse_appinfo(&bytes, &limits);
        let _ = parsers::lnk::parse(&bytes, &limits);
        let _ = parsers::desktop::parse(&bytes, &limits);
        let _ = parsers::text_vdf::parse(&bytes, &limits);
        let _ = parsers::url::parse(&bytes, &limits);
        let _ = parsers::plist::parse(&bytes, &limits);
        let _ = parsers::protobuf::parse_product_db(&bytes, &limits);
    }
}

#[test]
fn selective_appinfo_skips_unwanted_trees_and_resets_per_app_budget() {
    use std::collections::BTreeSet;
    let bytes = sample("appinfo-v29.vdf");
    let limits = Limits {
        max_nodes: 5,
        ..LIMITS
    };
    assert!(parsers::binary_vdf::parse_appinfo(&bytes, &limits).is_err());
    let both = BTreeSet::from([620, 1493710]);
    assert_eq!(
        parsers::binary_vdf::parse_appinfo_selected(&bytes, &limits, &both)
            .unwrap()
            .apps
            .len(),
        2
    );
    let one = BTreeSet::from([1493710]);
    let mut corrupted = bytes.clone();
    // 16-byte v29 header, appid+size, 60-byte fixed app record.
    corrupted[16 + 8 + 60] = 255;
    assert!(parsers::binary_vdf::parse_appinfo(&corrupted, &LIMITS).is_err());
    let selected = parsers::binary_vdf::parse_appinfo_selected(&corrupted, &limits, &one).unwrap();
    assert_eq!(selected.apps.len(), 1);
    assert_eq!(selected.apps[0].app_id, 1493710);
    assert!(
        parsers::binary_vdf::parse_appinfo_selected(&bytes[..bytes.len() - 1], &limits, &one)
            .is_err()
    );
    assert!(
        parsers::binary_vdf::parse_appinfo_selected(&bytes, &limits, &BTreeSet::new())
            .unwrap()
            .apps
            .is_empty()
    );
}

#[test]
fn product_cached_install_state_roundtrips_and_obeys_depth_budgets() {
    use parsers::protobuf::{BaseProductState, CachedProductState, ProductDb, ProductInstall};
    use prost::Message;
    for installed in [true, false] {
        let db = ProductDb {
            products: vec![ProductInstall {
                uid: "fixture".into(),
                product_code: "wow".into(),
                cached_product_state: Some(CachedProductState {
                    base_product_state: Some(BaseProductState {
                        installed,
                        playable: true,
                    }),
                }),
                ..Default::default()
            }],
        };
        let bytes = db.encode_to_vec();
        assert_eq!(
            parsers::protobuf::parse_product_db(&bytes, &LIMITS).unwrap(),
            db
        );
        let shallow = Limits {
            max_depth: 2,
            ..LIMITS
        };
        assert!(matches!(
            parsers::protobuf::parse_product_db(&bytes, &shallow),
            Err(ParseError::Limit { .. })
        ));
    }
}
