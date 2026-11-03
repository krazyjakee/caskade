//! Incremental rescans and file watching (#99) on a materialized copy of
//! the steam-linux-shared fixture.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use caskade_discovery::{
    Cancel, Config, HostEnv, Source,
    sources::steam::{ID as STEAM, Steam},
    watch::Library,
};

use crate::fixtures as support;

fn machine() -> (tempfile::TempDir, PathBuf) {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let scenario = support::Scenario::load(&fixtures, "steam-linux-shared").unwrap();
    let temp = crate::fixtures::tempdir();
    let machine = temp.path().join("machine");
    scenario.materialize(&machine).unwrap();
    (temp, machine)
}

fn steamapps(machine: &Path) -> PathBuf {
    machine.join("root/home/fixture/.local/share/Steam/steamapps")
}

/// Writes a manifest atomically (temp file + rename), as Steam does.
fn install(steamapps: &Path, app_id: u32, name: &str) {
    fs::create_dir_all(steamapps.join(format!("common/{name}"))).unwrap();
    let manifest = format!(
        "\"AppState\"\n{{\n\t\"appid\"\t\t\"{app_id}\"\n\t\"name\"\t\t\"{name}\"\n\t\"StateFlags\"\t\t\"4\"\n\t\"installdir\"\t\t\"{name}\"\n}}\n"
    );
    let staging = steamapps.join(format!("appmanifest_{app_id}.tmp"));
    fs::write(&staging, manifest).unwrap();
    fs::rename(staging, steamapps.join(format!("appmanifest_{app_id}.acf"))).unwrap();
}

fn titles(groups: &[caskade_discovery::GameGroup]) -> Vec<String> {
    groups
        .iter()
        .map(|group| group.primary_entry().unwrap().title.clone())
        .collect()
}

fn sources() -> Vec<Arc<dyn Source>> {
    vec![Arc::new(Steam)]
}

/// One source's rescan, with the same fixture on every host. Windows' file
/// opens cost several times Linux's (tests/HARDENING.md has the measurements).
const RESCAN_BUDGET: Duration = if cfg!(windows) {
    Duration::from_millis(400)
} else {
    Duration::from_millis(200)
};

#[test]
fn installing_and_removing_a_game_each_produce_one_delta() {
    let (_temp, machine) = machine();
    let env = HostEnv::fixture(&machine).unwrap();
    let mut library = Library::with_sources(env, Config::default(), sources());
    let initial = library.scan(&Cancel::new()).unwrap();
    assert_eq!(initial.added.len(), 4);
    let steam = BTreeSet::from([STEAM]);
    // Unchanged rescans, logged with the timed one below for the per-OS
    // budget in tests/HARDENING.md: steady state, not a cold first read.
    for _ in 0..5 {
        let started = Instant::now();
        assert!(library.rescan(&steam, &Cancel::new()).unwrap().is_empty());
        println!("unchanged rescan: {:?}", started.elapsed());
    }

    install(&steamapps(&machine), 2_000_060, "Fixture Newcomer");
    let started = Instant::now();
    let delta = library.rescan(&steam, &Cancel::new()).unwrap();
    let elapsed = started.elapsed();
    println!("one source rescan: {elapsed:?}");
    assert!(
        elapsed < RESCAN_BUDGET,
        "one source rescan took {elapsed:?}"
    );
    assert_eq!(titles(&delta.added), ["Fixture Newcomer"]);
    assert!(delta.updated.is_empty() && delta.removed.is_empty());
    assert_eq!(delta.sources.len(), 1);
    assert_eq!(library.report().groups.len(), 5);

    fs::remove_file(steamapps(&machine).join("appmanifest_2000060.acf")).unwrap();
    let delta = library.rescan(&steam, &Cancel::new()).unwrap();
    assert_eq!(titles(&delta.removed), ["Fixture Newcomer"]);
    assert!(delta.added.is_empty() && delta.updated.is_empty());

    // A finished download updates its group in place.
    let downloading = machine.join("root/mnt/games/SteamLibrary/steamapps/appmanifest_2000020.acf");
    let text = fs::read_to_string(&downloading)
        .unwrap()
        .replace("\"1026\"", "\"4\"");
    fs::write(&downloading, text).unwrap();
    let delta = library.rescan(&steam, &Cancel::new()).unwrap();
    assert_eq!(titles(&delta.updated), ["Fixture Downloading"]);
    assert!(delta.added.is_empty() && delta.removed.is_empty());
}

#[cfg(feature = "watch")]
mod live {
    use super::*;
    use caskade_discovery::{
        ScanDelta,
        watch::{WatchOptions, watch_with_sources},
    };
    use std::sync::mpsc;

    fn options() -> WatchOptions {
        WatchOptions {
            debounce: Duration::from_millis(150),
            max_delay: Duration::from_secs(2),
            ..WatchOptions::default()
        }
    }

    fn next(deltas: &mpsc::Receiver<ScanDelta>) -> ScanDelta {
        deltas
            .recv_timeout(Duration::from_secs(10))
            .expect("a delta within 10 s")
    }

    fn quiet(deltas: &mpsc::Receiver<ScanDelta>) {
        if let Ok(delta) = deltas.recv_timeout(Duration::from_millis(800)) {
            panic!("unexpected extra delta: {delta:?}");
        }
    }

    #[test]
    fn file_events_produce_one_delta_per_change() {
        let (_temp, machine) = machine();
        let env = HostEnv::fixture(&machine).unwrap();
        let (sender, deltas) = mpsc::channel();
        let watcher =
            watch_with_sources(env, Config::default(), options(), sources(), move |delta| {
                let _ = sender.send(delta);
            })
            .unwrap();
        assert_eq!(next(&deltas).added.len(), 4);
        quiet(&deltas);

        install(&steamapps(&machine), 2_000_060, "Fixture Newcomer");
        let delta = next(&deltas);
        assert_eq!(titles(&delta.added), ["Fixture Newcomer"]);
        quiet(&deltas);

        fs::remove_file(steamapps(&machine).join("appmanifest_2000060.acf")).unwrap();
        let delta = next(&deltas);
        assert_eq!(titles(&delta.removed), ["Fixture Newcomer"]);
        quiet(&deltas);
        watcher.stop();
    }

    #[test]
    fn a_launcher_installed_after_the_watch_started_is_picked_up() {
        let temp = crate::fixtures::tempdir();
        let machine = temp.path().join("machine");
        fs::create_dir_all(machine.join("root/home/fixture/.local/share")).unwrap();
        fs::write(
            machine.join("machine.toml"),
            "schema_version = 1\nos = \"linux\"\n",
        )
        .unwrap();
        let env = HostEnv::fixture(&machine).unwrap();
        let (sender, deltas) = mpsc::channel();
        let _watcher =
            watch_with_sources(env, Config::default(), options(), sources(), move |delta| {
                let _ = sender.send(delta);
            })
            .unwrap();
        quiet(&deltas);

        // Install Steam with one game, appearing in one step.
        let staging = temp.path().join("Steam");
        let apps = staging.join("steamapps");
        fs::create_dir_all(&apps).unwrap();
        install(&apps, 2_000_070, "Fixture Late Arrival");
        fs::rename(
            &staging,
            machine.join("root/home/fixture/.local/share/Steam"),
        )
        .unwrap();
        let delta = next(&deltas);
        assert_eq!(titles(&delta.added), ["Fixture Late Arrival"]);
        assert_eq!(delta.launchers.len(), 1);
        assert!(delta.launchers[0].found);
    }

    #[cfg(feature = "deep")]
    #[test]
    fn a_game_copied_into_a_deep_scan_folder_is_picked_up() {
        let temp = crate::fixtures::tempdir();
        let machine = temp.path().join("machine");
        let library = machine.join("root/home/fixture/Games");
        fs::create_dir_all(&library).unwrap();
        fs::write(
            machine.join("machine.toml"),
            "schema_version = 1\nos = \"linux\"\n",
        )
        .unwrap();
        let env = HostEnv::fixture(&machine).unwrap();
        let mut config = Config::default();
        config.deep.folders = vec![library.clone()];
        let sources: Vec<Arc<dyn Source>> = vec![Arc::new(caskade_discovery::deep::Deep)];
        let (sender, deltas) = mpsc::channel();
        let _watcher = watch_with_sources(env, config, options(), sources, move |delta| {
            let _ = sender.send(delta);
        })
        .unwrap();
        quiet(&deltas);
        let staging = temp.path().join("Fixture Deep Find");
        for file in [
            "game.x86_64",
            "UnityPlayer.so",
            "game_Data/globalgamemanagers",
        ] {
            let path = staging.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "inert").unwrap();
        }
        fs::rename(&staging, library.join("Fixture Deep Find")).unwrap();
        let delta = next(&deltas);
        assert_eq!(delta.added.len(), 1, "{delta:?}");
    }
}
