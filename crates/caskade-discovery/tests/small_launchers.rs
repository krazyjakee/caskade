#![cfg(feature = "linux-sources")]
use caskade_discovery::{
    sources::{faugus::Faugus, heroic::Heroic, minigalaxy::Minigalaxy},
    *,
};
use std::{fs, path::PathBuf, sync::Arc};
use crate::fixtures;
fn corpus() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}
fn machine(name: &str) -> PathBuf {
    corpus().join("machines").join(name)
}
fn games(source: &dyn Source, name: &str) -> Vec<DetectedGame> {
    source
        .scan(
            &HostEnv::fixture(machine(name)).unwrap(),
            &ScanCtx::new(Cancel::new()),
        )
        .unwrap()
}
#[test]
fn minigalaxy_gog_launch_metadata_legacy_info_and_windows_prefix() {
    let env = HostEnv::fixture(machine("minigalaxy-linux")).unwrap();
    assert!(Minigalaxy.detect(&env)[0].found);
    let games = games(&Minigalaxy, "minigalaxy-linux");
    assert_eq!(games.len(), 3);
    let adventure = games
        .iter()
        .find(|g| {
            g.ids.contains(&ExternalId::Gog {
                product_id: 1207658924,
            })
        })
        .unwrap();
    let LaunchTarget::Exec {
        program, args, cwd, ..
    } = &adventure.launch[0]
    else {
        panic!("expected FileTask")
    };
    assert!(program.ends_with("GOG Games/Fixture Adventure/start.sh"));
    assert_eq!(args, &["--name", "Fixture Adventure"]);
    assert_eq!(cwd, &adventure.install_dir);
    let windows = games
        .iter()
        .find(|g| {
            g.ids.contains(&ExternalId::Gog {
                product_id: 1207658925,
            })
        })
        .unwrap();
    let GameRuntime::Windows {
        prefix: Some(prefix),
        ..
    } = &windows.runtime
    else {
        panic!("no Minigalaxy prefix")
    };
    assert!(prefix.path.ends_with("GOG Games/Windows Mini"));
    assert_eq!(prefix.owner, Some(SourceId::new("minigalaxy")));
    let legacy = games.iter().find(|g| g.title == "Legacy Info").unwrap();
    assert!(legacy.ids.is_empty());
    assert_eq!(legacy.launch.len(), 1);
    assert!(
        Minigalaxy
            .watch_paths(&env)
            .iter()
            .any(|w| w.recursive && w.path.ends_with("GOG Games"))
    );
    fixtures::assert_snapshot("minigalaxy", &games, &machine("minigalaxy-linux"));
}
#[test]
fn minigalaxy_and_heroic_share_one_gog_group() {
    let env = HostEnv::fixture(machine("minigalaxy-linux")).unwrap();
    let sources: Vec<Arc<dyn Source>> = vec![Arc::new(Minigalaxy), Arc::new(Heroic)];
    let report = scan_with_sources(&env, &Config::default(), &Cancel::new(), &sources).unwrap();
    assert!(
        report.sources.iter().all(|s| s.error.is_none()),
        "{report:?}"
    );
    assert_eq!(report.groups.len(), 3);
    let gog = report
        .groups
        .iter()
        .find(|g| {
            g.entries.iter().any(|e| {
                e.ids.contains(&ExternalId::Gog {
                    product_id: 1207658924,
                })
            })
        })
        .unwrap();
    assert_eq!(gog.entries.len(), 2);
    assert!(gog.entries.iter().any(|e| e.source.as_str() == "heroic"));
    assert!(
        gog.entries
            .iter()
            .any(|e| e.source.as_str() == "minigalaxy")
    );
}
#[test]
fn faugus_current_legacy_and_flatpak_files_preserve_prefixes_without_executing_hooks() {
    let env = HostEnv::fixture(machine("faugus-linux")).unwrap();
    assert_eq!(Faugus.detect(&env).len(), 2);
    let ctx = ScanCtx::new(Cancel::new());
    let games = Faugus.scan(&env, &ctx).unwrap();
    assert_eq!(games.len(), 4);
    let windows = games
        .iter()
        .find(|g| g.key == "native:fixture-windows")
        .unwrap();
    assert_eq!(windows.title, "Faugus Fixture Windows");
    assert_eq!(windows.playtime_secs, Some(1800));
    assert_eq!(windows.last_played_unix, None);
    let GameRuntime::Windows {
        prefix: Some(prefix),
        runner: Some(runner),
    } = &windows.runtime
    else {
        panic!("missing prefix")
    };
    assert!(prefix.path.ends_with("Faugus/Fixture"));
    assert_eq!(prefix.owner, Some(SourceId::new("faugus")));
    assert_eq!(runner.kind, RunnerKind::Umu);
    let LaunchTarget::Exec { args, .. } = &windows.launch[0] else {
        panic!("missing exe")
    };
    assert_eq!(args, &["--name", "Fixture Windows"]);
    assert_eq!(
        games
            .iter()
            .find(|g| g.key == "native:fixture-native")
            .unwrap()
            .runtime,
        GameRuntime::Native
    );
    assert!(
        games
            .iter()
            .any(|g| matches!(g.launch[0], LaunchTarget::Uri { .. }))
    );
    assert!(!ctx.take_warnings().is_empty());
    assert_eq!(Faugus.watch_paths(&env).len(), 3);
    fixtures::assert_snapshot("faugus", &games, &machine("faugus-linux"));
}
#[test]
fn malformed_or_oversized_inputs_cancel_and_unsafe_task_paths_are_bounded() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("mini");
    fixtures::Scenario::load(&corpus(), "minigalaxy-linux")
        .unwrap()
        .materialize(&root)
        .unwrap();
    let dir = root.join("root/home/fixture/GOG Games/Fixture Adventure");
    fs::write(dir.join("goggame-1207658924.info"),r#"{"name":"Fixture Adventure","playTasks":[{"type":"FileTask","path":"../../outside.exe"}]}"#).unwrap();
    fs::remove_file(dir.join("start.sh")).unwrap();
    let env = HostEnv::fixture(&root).unwrap();
    let scan = Minigalaxy.scan(&env, &ScanCtx::new(Cancel::new())).unwrap();
    assert!(
        scan.iter()
            .find(|g| g.key == "1207658924")
            .unwrap()
            .launch
            .is_empty()
    );
    let config = root.join("root/home/fixture/.config/minigalaxy/config.json");
    fs::File::create(&config)
        .unwrap()
        .set_len((1 << 20) + 1)
        .unwrap();
    let ctx = ScanCtx::new(Cancel::new());
    assert!(Minigalaxy.scan(&env, &ctx).unwrap().is_empty());
    assert!(!ctx.take_warnings().is_empty());
    let cancel = Cancel::new();
    cancel.cancel();
    assert_eq!(
        Minigalaxy.scan(&env, &ScanCtx::new(cancel)),
        Err(SourceError::Cancelled)
    );
    let root = temp.path().join("faugus");
    fixtures::Scenario::load(&corpus(), "faugus-linux")
        .unwrap()
        .materialize(&root)
        .unwrap();
    fs::write(
        root.join("root/home/fixture/.local/share/faugus-launcher/games.json"),
        "{bad",
    )
    .unwrap();
    let ctx = ScanCtx::new(Cancel::new());
    let games = Faugus
        .scan(&HostEnv::fixture(&root).unwrap(), &ctx)
        .unwrap();
    assert_eq!(games.len(), 2);
    assert!(!ctx.take_warnings().is_empty());
}
