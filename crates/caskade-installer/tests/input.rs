use caskade_installer::input::*;
use std::path::{Path, PathBuf};

fn fixture(names: &[&str]) -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    for name in names {
        let path = temp.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"fixture").unwrap();
    }
    temp
}
fn basenames(paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
        .map(|p| p.file_name().unwrap().to_str().unwrap().to_owned())
        .collect()
}
fn assert_set(
    temp: &tempfile::TempDir,
    selected: &str,
    expected_kind: SetKind,
    expected_primary: &str,
    expected_files: &[&str],
) {
    match resolve_path(&temp.path().join(selected)).unwrap() {
        ResolvedInput::MultiPart {
            kind,
            primary,
            files,
        } => {
            assert_eq!(kind, expected_kind);
            assert_eq!(primary.file_name().unwrap(), expected_primary);
            assert_eq!(basenames(&files), expected_files);
        }
        other => panic!("expected multipart, got {other:?}"),
    }
}

#[test]
fn single_file_and_url_boundary() {
    let temp = fixture(&["game.zip"]);
    assert_eq!(
        resolve(&Input::Path(temp.path().join("game.zip"))).unwrap(),
        ResolvedInput::File {
            path: dunce::canonicalize(temp.path().join("game.zip")).unwrap()
        }
    );
    assert!(matches!(
        resolve(&Input::Urls(vec![])),
        Err(InputError::DownloadRequired)
    ));
}

#[test]
fn gathers_every_numbered_set_from_each_member_in_numeric_order() {
    let cases: &[(SetKind, &[&str], &str)] = &[
        (
            SetKind::Inno,
            &["setup.exe", "setup-1.bin", "setup-2.bin"],
            "setup.exe",
        ),
        (
            SetKind::Rar,
            &["game.part01.rar", "game.part02.rar", "game.part03.rar"],
            "game.part01.rar",
        ),
        (
            SetKind::SevenZip,
            &["game.7z.001", "game.7z.002", "game.7z.003"],
            "game.7z.001",
        ),
        (
            SetKind::SplitZip,
            &["game.z01", "game.z02", "game.zip"],
            "game.zip",
        ),
        (
            SetKind::Discs,
            &[
                "Game (Disc 1).iso",
                "Game (Disc 2).iso",
                "Game (Disc 3).iso",
            ],
            "Game (Disc 1).iso",
        ),
    ];
    for (kind, names, primary) in cases {
        let temp = fixture(names);
        for selected in *names {
            assert_set(&temp, selected, *kind, primary, names);
        }
    }
    let names: Vec<_> = (1..=12).map(|n| format!("g.part{n}.rar")).collect();
    let refs: Vec<_> = names.iter().map(String::as_str).collect();
    let temp = fixture(&refs);
    assert_set(&temp, "g.part10.rar", SetKind::Rar, "g.part1.rar", &refs);
}

#[test]
fn missing_parts_are_reported_for_each_set_before_probing() {
    let cases: &[(SetKind, &[&str], &str, &[&str])] = &[
        (
            SetKind::Inno,
            &["setup.exe", "setup-2.bin"],
            "setup.exe",
            &["setup-1.bin"],
        ),
        (
            SetKind::Inno,
            &["setup-1.bin"],
            "setup-1.bin",
            &["setup.exe"],
        ),
        (
            SetKind::Rar,
            &["g.part02.rar"],
            "g.part02.rar",
            &["g.part01.rar"],
        ),
        (
            SetKind::SevenZip,
            &["g.7z.001", "g.7z.003"],
            "g.7z.001",
            &["g.7z.002"],
        ),
        (SetKind::SplitZip, &["g.z02", "g.zip"], "g.zip", &["g.z01"]),
        (SetKind::SplitZip, &["g.z01"], "g.z01", &["g.zip"]),
        (
            SetKind::Discs,
            &["Game (Disc 2).iso"],
            "Game (Disc 2).iso",
            &["game (disc 1).iso"],
        ),
    ];
    for (kind, names, selected, expected) in cases {
        let temp = fixture(names);
        match resolve_path(&temp.path().join(selected)).unwrap_err() {
            InputError::MissingParts {
                kind: actual,
                missing,
            } => {
                assert_eq!(actual, *kind);
                assert_eq!(basenames(&missing), *expected);
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}

#[test]
fn numeric_sets_are_case_insensitive_and_duplicates_are_errors() {
    let temp = fixture(&["SETUP.EXE", "setup-1.BIN", "Setup-2.bin", "other-1.bin"]);
    assert_set(
        &temp,
        "SETUP.EXE",
        SetKind::Inno,
        "SETUP.EXE",
        &["SETUP.EXE", "setup-1.BIN", "Setup-2.bin"],
    );
    let temp = fixture(&["g.part1.rar", "g.part01.rar"]);
    assert!(matches!(
        resolve_path(&temp.path().join("g.part1.rar")),
        Err(InputError::DuplicatePart { number: 1, .. })
    ));
    let temp = fixture(&["g.7z.999999999"]);
    assert!(matches!(
        resolve_path(&temp.path().join("g.7z.999999999")),
        Err(InputError::ScanLimit)
    ));
}

#[test]
fn cue_bin_references_are_ordered_and_selection_of_a_track_finds_the_cue() {
    let temp = fixture(&["Game.cue", "track one.bin", "track two.bin"]);
    std::fs::write(
        temp.path().join("Game.cue"),
        "FILE \"track one.bin\" BINARY\n TRACK 01 MODE2/2352\nfile \"track two.bin\" BINARY\n",
    )
    .unwrap();
    for selected in ["Game.cue", "track one.bin", "track two.bin"] {
        assert_set(
            &temp,
            selected,
            SetKind::CueBin,
            "Game.cue",
            &["Game.cue", "track one.bin", "track two.bin"],
        );
    }
    std::fs::remove_file(temp.path().join("track two.bin")).unwrap();
    assert!(
        matches!(resolve_path(&temp.path().join("Game.cue")), Err(InputError::MissingParts { kind: SetKind::CueBin, missing }) if basenames(&missing) == ["track two.bin"])
    );
}

#[test]
fn malformed_and_traversing_cues_are_rejected() {
    for cue in [
        "not a cue",
        "FILE \"../escape.bin\" BINARY",
        "FILE \"/escape.bin\" BINARY",
        "FILE \"C:\\escape.bin\" BINARY",
        "FILE \"unterminated BINARY",
    ] {
        let temp = fixture(&["game.cue"]);
        std::fs::write(temp.path().join("game.cue"), cue).unwrap();
        assert!(
            matches!(
                resolve_path(&temp.path().join("game.cue")),
                Err(InputError::InvalidCue(_))
            ),
            "{cue}"
        );
    }
}

#[test]
fn folders_are_classified_as_a_whole() {
    let cases: &[(FolderKind, &[&str], usize)] = &[
        (
            FolderKind::InstallerSet,
            &["setup.exe", "setup-1.bin", "README.txt"],
            1,
        ),
        (FolderKind::InstallerSet, &["game.zip", "license.md"], 1),
        (
            FolderKind::PortableGame,
            &["Game.exe", "Game_Data/data.dat", "README.txt"],
            0,
        ),
        (
            FolderKind::PortableGame,
            &["Game.app/Contents/MacOS/Game"],
            0,
        ),
        (
            FolderKind::Mixed,
            &["setup.exe", "Game.exe", "README.txt"],
            1,
        ),
        (FolderKind::Mixed, &["setup.exe", "extras/art.png"], 1),
        (FolderKind::Unknown, &["README.txt"], 0),
    ];
    for (expected, names, count) in cases {
        let temp = fixture(names);
        match resolve_path(temp.path()).unwrap() {
            ResolvedInput::Folder {
                classification,
                files,
                installers,
                ..
            } => {
                assert_eq!(classification, *expected, "{names:?}");
                assert_eq!(files.len(), names.len());
                assert_eq!(installers.len(), *count);
            }
            other => panic!("expected whole folder, got {other:?}"),
        }
    }
    let temp = fixture(&["nested/g.7z.001", "nested/g.7z.003"]);
    assert!(matches!(
        resolve_path(temp.path()),
        Err(InputError::MissingParts { .. })
    ));
}

#[cfg(unix)]
#[test]
fn folder_scan_does_not_follow_directory_symlinks() {
    let temp = fixture(&["Game.exe"]);
    std::os::unix::fs::symlink(temp.path(), temp.path().join("cycle")).unwrap();
    if let ResolvedInput::Folder { files, .. } = resolve_path(temp.path()).unwrap() {
        assert_eq!(files.len(), 1);
    } else {
        panic!("expected folder");
    }
}

fn relative_paths(value: &mut serde_json::Value, root: &Path) {
    match value {
        serde_json::Value::String(text) => {
            if let Ok(path) = Path::new(text).strip_prefix(root) {
                *text = path.to_str().unwrap().replace('\\', "/");
            }
        }
        serde_json::Value::Array(values) => values.iter_mut().for_each(|v| relative_paths(v, root)),
        serde_json::Value::Object(values) => {
            values.values_mut().for_each(|v| relative_paths(v, root))
        }
        _ => (),
    }
}

#[test]
fn resolved_archive_fixture_snapshots() {
    let root =
        dunce::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")).unwrap();
    let mut manifests: Vec<_> = std::fs::read_dir(root.join("expected"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .collect();
    manifests.sort();
    assert!(manifests.len() >= 15);
    for manifest in manifests {
        let expected: toml::Value =
            toml::from_str(&std::fs::read_to_string(manifest).unwrap()).unwrap();
        let id = expected["id"].as_str().unwrap();
        let resolved = resolve_path(&root.join(expected["input"].as_str().unwrap())).unwrap();
        let parts = match &resolved {
            ResolvedInput::File { path } => vec![path.clone()],
            ResolvedInput::MultiPart { files, .. } => files.clone(),
            _ => panic!("archive resolved as folder"),
        };
        let declared: Vec<_> = expected["parts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| dunce::canonicalize(root.join(p.as_str().unwrap())).unwrap())
            .collect();
        assert_eq!(parts, declared, "{id}");
        let mut value = serde_json::to_value(resolved).unwrap();
        relative_paths(&mut value, &root);
        insta::assert_json_snapshot!(format!("resolved_{id}"), value);
    }
}

#[test]
fn numbered_bin_tracks_take_precedence_over_inno_heuristic() {
    let temp = fixture(&["game.cue", "track-01.bin", "track-02.bin"]);
    std::fs::write(
        temp.path().join("game.cue"),
        "FILE \"track-01.bin\" BINARY\nFILE \"track-02.bin\" BINARY\n",
    )
    .unwrap();
    assert_set(
        &temp,
        "track-01.bin",
        SetKind::CueBin,
        "game.cue",
        &["game.cue", "track-01.bin", "track-02.bin"],
    );
    std::fs::remove_file(temp.path().join("track-02.bin")).unwrap();
    assert!(matches!(
        resolve_path(&temp.path().join("track-01.bin")),
        Err(InputError::MissingParts {
            kind: SetKind::CueBin,
            ..
        })
    ));
}

#[test]
fn multidisc_cue_sets_include_all_tracks_and_validate_every_disc() {
    let temp = fixture(&[
        "Game (Disc 1).cue",
        "Game (Disc 2).cue",
        "track1.bin",
        "track2.bin",
    ]);
    for n in 1..=2 {
        std::fs::write(
            temp.path().join(format!("Game (Disc {n}).cue")),
            format!("FILE track{n}.bin BINARY\n"),
        )
        .unwrap();
    }
    for selected in [
        "Game (Disc 1).cue",
        "Game (Disc 2).cue",
        "track1.bin",
        "track2.bin",
    ] {
        assert_set(
            &temp,
            selected,
            SetKind::Discs,
            "Game (Disc 1).cue",
            &[
                "Game (Disc 1).cue",
                "Game (Disc 2).cue",
                "track1.bin",
                "track2.bin",
            ],
        );
    }
    std::fs::remove_file(temp.path().join("track2.bin")).unwrap();
    assert!(matches!(
        resolve_path(&temp.path().join("Game (Disc 1).cue")),
        Err(InputError::MissingParts {
            kind: SetKind::CueBin,
            ..
        })
    ));
}

#[test]
fn cue_text_parser_is_pure_bounded_and_confines_every_file_reference() {
    use caskade_installer::input::parse_cue_references;
    assert_eq!(
        parse_cue_references(
            "FILE \"Track 01.bin\" BINARY\n TRACK 01 MODE2/2352\nfile track02.bin BINARY\n"
        )
        .unwrap(),
        vec![
            std::path::PathBuf::from("Track 01.bin"),
            std::path::PathBuf::from("track02.bin")
        ]
    );
    for text in [
        "FILE \"../outside.bin\" BINARY",
        "FILE \"C:\\outside.bin\" BINARY",
        "FILE \"/outside.bin\" BINARY",
        "FILE \"unterminated",
        "REM no tracks",
    ] {
        assert!(parse_cue_references(text).is_err(), "{text}");
    }
    assert!(matches!(
        parse_cue_references(&"x".repeat(1024 * 1024 + 1)),
        Err(caskade_installer::input::InputError::ScanLimit)
    ));
}
