//! #62 T3: exhaustive event cancellation and exact rollback snapshots.
use caskade_installer::{
    events::{EventSink, JobEvent},
    manifest::{
        InstallManifest, InstalledFile, ManifestPath, RelocationVerdict, RootManifest, Sha256Digest,
    },
    reconcile::{ReconcileStatus, Reconciler},
    relocate::{
        RegistryStore, RegistryWrite, RelocateError, RelocateRequest, Relocator, RunningCheck,
        ShortcutEditor, replace_all,
    },
    runtime::alias::AliasStore,
    scan::{self, ScanInput},
    testing::FakeDiskSpace,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio_util::sync::CancellationToken;

#[derive(Default)]
struct Registry {
    values: Mutex<Vec<RegistryWrite>>,
    fail_once: AtomicBool,
}
impl RegistryStore for Registry {
    fn read(&self, hive: &str, key: &str, name: &str) -> io::Result<Option<RegistryWrite>> {
        Ok(self
            .values
            .lock()
            .unwrap()
            .iter()
            .find(|v| v.hive == hive && v.key == key && v.name == name)
            .cloned())
    }
    fn apply(&self, writes: &[RegistryWrite]) -> io::Result<()> {
        let fail = self.fail_once.swap(false, Ordering::SeqCst);
        let mut values = self.values.lock().unwrap();
        for write in writes {
            let slot = values
                .iter_mut()
                .find(|v| v.hive == write.hive && v.key == write.key && v.name == write.name)
                .unwrap();
            *slot = write.clone();
            if fail {
                return Err(io::Error::other(
                    "registry batch failed after its first write",
                ));
            }
        }
        Ok(())
    }
}
struct Shortcuts {
    fail: bool,
}
impl ShortcutEditor for Shortcuts {
    fn rewrite(&self, path: &Path, old: &str, new: &str) -> io::Result<()> {
        if let Some(bytes) = replace_all(&fs::read(path)?, &[(old.into(), new.into())]) {
            fs::write(path, bytes)?;
        }
        if self.fail {
            Err(io::Error::other("shortcut failed after modifying bytes"))
        } else {
            Ok(())
        }
    }
}
struct NotRunning;
impl RunningCheck for NotRunning {
    fn running(&self, _: &Path) -> bool {
        false
    }
}

#[derive(Default)]
struct Sink {
    messages: Mutex<Vec<String>>,
    cancel: CancellationToken,
    cancel_at: usize,
    corrupt: Option<PathBuf>,
}
impl EventSink for Sink {
    fn emit(&self, event: JobEvent) {
        let mut messages = self.messages.lock().unwrap();
        messages.push(event.message.clone());
        if messages.len() == self.cancel_at {
            self.cancel.cancel();
        }
        if event.message == "copied"
            && let Some(path) = &self.corrupt
        {
            let mut bytes = fs::read(path).unwrap();
            bytes[0] ^= 0xff; // Same length: only a content hash can catch it.
            fs::write(path, bytes).unwrap();
        }
    }
}

fn utf16(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}
fn hash(bytes: &[u8]) -> Sha256Digest {
    Sha256Digest::new(format!("{:x}", Sha256::digest(bytes))).unwrap()
}

#[derive(Debug, PartialEq, Eq)]
enum Entry {
    File(Vec<u8>),
    Link(PathBuf),
    Directory,
}
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Entry> {
    let mut result = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let metadata = fs::symlink_metadata(&path).unwrap();
            let item = if metadata.is_symlink() {
                Entry::Link(fs::read_link(&path).unwrap())
            } else if metadata.is_dir() {
                pending.push(path.clone());
                Entry::Directory
            } else {
                Entry::File(fs::read(&path).unwrap())
            };
            result.insert(path.strip_prefix(root).unwrap().into(), item);
        }
    }
    result
}

struct Fixture {
    temp: tempfile::TempDir,
    root: PathBuf,
    game: PathBuf,
    target: PathBuf,
    outside: PathBuf,
    aliases: AliasStore,
    id: String,
    registry: Arc<Registry>,
    request: RelocateRequest,
}
impl Fixture {
    fn new(copy: bool) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let root = base.join("old root");
        let target = base.join("new root");
        let game = root.join("Café Game");
        fs::create_dir_all(&game).unwrap();
        fs::create_dir(&target).unwrap();
        RootManifest::new("moving-root-old")
            .unwrap()
            .write(&root)
            .unwrap();
        RootManifest::new("moving-root-new")
            .unwrap()
            .write(&target)
            .unwrap();
        let path = game.to_str().unwrap();
        let mut files = vec![];
        for (name, bytes) in [
            (
                "utf8.ini",
                format!("é=1\r\npath={path}/data\r\n").into_bytes(),
            ),
            (
                "bom.ini",
                [
                    b"\xef\xbb\xbf".as_slice(),
                    format!("path={path}\n").as_bytes(),
                ]
                .concat(),
            ),
            (
                "utf16.cfg",
                [
                    b"\xff\xfe".as_slice(),
                    &utf16(&format!("é=1\r\npath={path}\r\n")),
                ]
                .concat(),
            ),
            (
                "ansi.cfg",
                encoding_rs::WINDOWS_1252
                    .encode(&format!("é=1\r\npath={path}\r\n"))
                    .0
                    .into_owned(),
            ),
            ("launch.lnk", utf16(&format!("{path}/game.exe\0"))),
            ("payload.bin", b"original binary payload\0".to_vec()),
        ] {
            fs::write(game.join(name), &bytes).unwrap();
            files.push(InstalledFile {
                path: ManifestPath::new(name).unwrap(),
                size: bytes.len() as u64,
                sha256: Some(hash(&bytes)),
            });
        }
        let outside = base.join("external.desktop");
        fs::write(&outside, format!("[Desktop Entry]\nPath={path}\n")).unwrap();
        let registry = Arc::new(Registry::default());
        *registry.values.lock().unwrap() = ["REG_SZ", "REG_EXPAND_SZ", "REG_MULTI_SZ"]
            .into_iter()
            .map(|kind| RegistryWrite {
                hive: "HKCU".into(),
                key: "Software\\Moving".into(),
                name: kind.into(),
                value_type: kind.into(),
                data: utf16(&if kind == "REG_MULTI_SZ" {
                    format!("{path}\0{path}/data\0\0")
                } else {
                    format!("{path}\0")
                }),
            })
            .collect();
        let values: Vec<_> = registry
            .values
            .lock()
            .unwrap()
            .iter()
            .map(|v| scan::RegistryValue {
                hive: v.hive.clone(),
                key: v.key.clone(),
                name: v.name.clone(),
                value_type: v.value_type.clone(),
                data: v.data.clone(),
            })
            .collect();
        let report = scan::scan_references(&ScanInput {
            game_dir: &game,
            real_path: path,
            alias_path: None,
            real_variants: &[],
            registry: &values,
            external_files: std::slice::from_ref(&outside),
            delegated: false,
            limits: Default::default(),
        })
        .unwrap();
        assert_eq!(report.verdict, RelocationVerdict::Rewritable);
        let aliases = AliasStore::new(base.join("aliases"));
        let id = AliasStore::short_id("moving-transaction");
        aliases.link(&id, &game).unwrap();
        // A relative directory link must remain a link during cross-volume copy.
        #[cfg(unix)]
        {
            fs::create_dir_all(game.join(".caskade/prefix/drive_g")).unwrap();
            std::os::unix::fs::symlink("../../..", game.join(".caskade/prefix/drive_g/Game"))
                .unwrap();
        }
        let mut record =
            InstallManifest::from_json(include_bytes!("manifests/install.v1.posix.json"))
                .unwrap()
                .into_record();
        record.files = files;
        record.plan.root_id = "moving-root-old".into();
        record.plan.relative_dir = "Café Game".into();
        record.launch_targets.clear();
        record.uninstall_steps.clear();
        record.external_effects.clear();
        record.alias = Some(id.clone());
        record.relocation.last_path = Some(path.into());
        record.relocation.verdict = report.verdict;
        record.relocation.references = report.references.clone();
        InstallManifest::new(record).unwrap().write(&game).unwrap();
        let request = RelocateRequest {
            job_id: "moving-test".into(),
            game_dir: game.clone(),
            target_root: target.clone(),
            references: report.references,
            spellings: vec![],
            alias: Some((aliases.clone(), id.clone())),
            force_copy: copy,
        };
        Self {
            temp,
            root,
            game,
            target,
            outside,
            aliases,
            id,
            registry,
            request,
        }
    }
    fn worker(&self, sink: Arc<Sink>, fail_shortcut: bool) -> Relocator {
        Relocator {
            disk: Arc::new(FakeDiskSpace::fixed(u64::MAX)),
            sink,
            registry: self.registry.clone(),
            shortcuts: Arc::new(Shortcuts {
                fail: fail_shortcut,
            }),
            running: Arc::new(NotRunning),
        }
    }
    fn destination(&self) -> PathBuf {
        self.target.join(self.game.file_name().unwrap())
    }
}

#[test]
fn cancellation_at_every_event_restores_exact_original_state() {
    for copy in [false, true] {
        let baseline = Fixture::new(copy);
        let sink = Arc::new(Sink::default());
        baseline
            .worker(sink.clone(), false)
            .relocate(&baseline.request, &sink.cancel)
            .unwrap();
        let events = sink.messages.lock().unwrap().clone();
        assert_eq!(events.last().unwrap(), "completed");
        let committed = events
            .iter()
            .position(|event| event == "committed")
            .expect("durable commit notification")
            + 1;
        assert!(committed < events.len());
        for index in 0..=events.len() {
            let fixture = Fixture::new(copy);
            let before = snapshot(fixture.temp.path());
            let registry = fixture.registry.values.lock().unwrap().clone();
            let sink = Arc::new(Sink {
                cancel_at: index,
                ..Default::default()
            });
            if index == 0 {
                sink.cancel.cancel();
            }
            let result = fixture
                .worker(sink.clone(), false)
                .relocate(&fixture.request, &sink.cancel);
            if index >= committed {
                // Once the journal commits, cancellation cannot undo the
                // successful move. Source cleanup completes durably after it.
                assert!(result.is_ok(), "{events:?}");
                assert!(!fixture.game.exists());
                assert_eq!(
                    fixture.aliases.target(&fixture.id).unwrap().unwrap(),
                    fixture.destination()
                );
                assert!(
                    InstallManifest::read(&fixture.destination())
                        .unwrap()
                        .verify(&fixture.destination())
                        .unwrap()
                        .is_ok()
                );
                continue;
            }
            assert!(
                matches!(result, Err(RelocateError::Cancelled)),
                "copy={copy}, event={index}, {result:?}"
            );
            assert_eq!(
                snapshot(fixture.temp.path()),
                before,
                "copy={copy}, event={index}"
            );
            assert_eq!(*fixture.registry.values.lock().unwrap(), registry);
            assert_eq!(
                fixture.aliases.target(&fixture.id).unwrap().unwrap(),
                fixture.game
            );
        }
    }
}

#[test]
fn partial_shortcut_and_registry_failures_restore_the_manifest_and_all_effects() {
    for copy in [false, true] {
        for shortcut in [false, true] {
            let fixture = Fixture::new(copy);
            let before = snapshot(fixture.temp.path());
            let registry = fixture.registry.values.lock().unwrap().clone();
            fixture
                .registry
                .fail_once
                .store(!shortcut, Ordering::SeqCst);
            let result = fixture
                .worker(Arc::new(Sink::default()), shortcut)
                .relocate(&fixture.request, &CancellationToken::new());
            assert!(
                matches!(result, Err(RelocateError::RolledBack(_))),
                "{result:?}"
            );
            assert_eq!(snapshot(fixture.temp.path()), before);
            assert_eq!(*fixture.registry.values.lock().unwrap(), registry);
        }
    }
}

#[test]
fn corrupted_same_size_cross_volume_copy_keeps_source_and_discards_destination() {
    let fixture = Fixture::new(true);
    let before = snapshot(fixture.temp.path());
    let sink = Arc::new(Sink {
        corrupt: Some(fixture.destination().join("payload.bin")),
        ..Default::default()
    });
    let result = fixture
        .worker(sink.clone(), false)
        .relocate(&fixture.request, &sink.cancel);
    assert!(
        sink.messages.lock().unwrap().iter().any(|m| m == "copied"),
        "copy verification must expose its pre-verification event"
    );
    assert!(
        matches!(result, Err(RelocateError::RolledBack(ref reason)) if reason.contains("copied file differs")),
        "{result:?}"
    );
    assert_eq!(snapshot(fixture.temp.path()), before);
}

#[test]
fn every_encoding_bom_line_ending_and_multistring_survives_a_move() {
    for copy in [false, true] {
        let fixture = Fixture::new(copy);
        let before = snapshot(&fixture.game);
        let old = fixture.game.to_str().unwrap();
        let destination = fixture.destination();
        let new = destination.to_str().unwrap();
        let report = fixture
            .worker(Arc::new(Sink::default()), false)
            .relocate(&fixture.request, &CancellationToken::new())
            .unwrap();
        assert_eq!(report.rewritten_values, 3);
        for (path, entry) in before {
            if let Entry::File(bytes) = entry {
                if path.starts_with(".caskade") {
                    continue;
                }
                let expected = if path == Path::new("payload.bin") {
                    bytes
                } else {
                    // Expected bytes are encoded independently from the rewrite.
                    let text = if path == Path::new("ansi.cfg") {
                        encoding_rs::WINDOWS_1252.decode(&bytes).0.into_owned()
                    } else if path == Path::new("utf16.cfg") {
                        String::from_utf16(
                            &bytes[2..]
                                .as_chunks::<2>()
                                .0
                                .iter()
                                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                                .collect::<Vec<_>>(),
                        )
                        .unwrap()
                    } else if path == Path::new("launch.lnk") {
                        String::from_utf16(
                            &bytes
                                .as_chunks::<2>()
                                .0
                                .iter()
                                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                                .collect::<Vec<_>>(),
                        )
                        .unwrap()
                    } else {
                        String::from_utf8(bytes.clone()).unwrap()
                    };
                    let text = text.replace(old, new);
                    if path == Path::new("ansi.cfg") {
                        encoding_rs::WINDOWS_1252.encode(&text).0.into_owned()
                    } else if path == Path::new("utf16.cfg") {
                        [b"\xff\xfe".as_slice(), &utf16(&text)].concat()
                    } else if path == Path::new("launch.lnk") {
                        utf16(&text)
                    } else {
                        text.into_bytes()
                    }
                };
                assert_eq!(
                    fs::read(destination.join(&path)).unwrap(),
                    expected,
                    "{path:?}"
                );
            }
        }
        let manifest = InstallManifest::read(&destination).unwrap();
        assert_eq!(manifest.record().plan.root_id, "moving-root-new");
        let verified = manifest.verify(&destination).unwrap();
        assert!(
            verified.changed.is_empty() && verified.missing.is_empty(),
            "{verified:?}"
        );
        assert_eq!(manifest.record().relocation.last_path.as_deref(), Some(new));
        assert_eq!(
            fs::read_to_string(&fixture.outside).unwrap(),
            format!("[Desktop Entry]\nPath={new}\n")
        );
        for value in fixture.registry.values.lock().unwrap().iter() {
            let text = if value.value_type == "REG_MULTI_SZ" {
                format!("{new}\0{new}/data\0\0")
            } else {
                format!("{new}\0")
            };
            assert_eq!(value.data, utf16(&text));
        }
    }
}

#[tokio::test]
async fn managed_move_then_external_root_rename_reconciles_by_the_new_root_identity() {
    let fixture = Fixture::new(true);
    let worker = Arc::new(fixture.worker(Arc::new(Sink::default()), false));
    worker
        .relocate(&fixture.request, &CancellationToken::new())
        .unwrap();
    let renamed = fixture.temp.path().join("drive remounted");
    fs::rename(&fixture.target, &renamed).unwrap();
    let report = Reconciler::new(worker)
        .with_aliases(fixture.aliases.clone())
        .with_external_roots(vec![fixture.temp.path().into()])
        .scan_root(&renamed, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(report.root_id, "moving-root-new");
    assert_eq!(report.games.len(), 1);
    assert_eq!(
        report.games[0].status,
        ReconcileStatus::Reconciled,
        "{report:?}"
    );
    let game = renamed.join("Café Game");
    let manifest = InstallManifest::read(&game).unwrap();
    assert_eq!(manifest.record().plan.root_id, "moving-root-new");
    assert_eq!(
        manifest.record().relocation.last_path.as_deref(),
        game.to_str()
    );
    assert_eq!(
        RootManifest::read(&renamed).unwrap().root_id(),
        "moving-root-new"
    );
    assert_eq!(
        fixture.aliases.target(&fixture.id).unwrap(),
        Some(game.clone())
    );
    assert!(!fixture.root.join("Café Game").exists());
    let verified = manifest.verify(&game).unwrap();
    assert!(
        verified.changed.is_empty() && verified.missing.is_empty(),
        "{verified:?}"
    );
}
