#![no_main]
use caskade_discovery::{HostEnv, ScanCtx, Source, sources::epic::EpicGamesLauncher};
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    if data.len() > 65536 {
        return;
    }
    // Exercise the actual source parser and manifest filtering, not a parallel
    // JSON imitation. Only a fresh private synthetic Windows machine is read.
    let temp = tempfile::tempdir().unwrap();
    let manifests = temp
        .path()
        .join("root/drives/c/ProgramData/Epic/EpicGamesLauncher/Data/Manifests");
    std::fs::create_dir_all(&manifests).unwrap();
    std::fs::write(
        temp.path().join("machine.toml"),
        "schema_version=1\nos='windows'\n",
    )
    .unwrap();
    std::fs::write(manifests.join("input.item"), data).unwrap();
    let env = HostEnv::fixture(temp.path()).unwrap();
    let _ = EpicGamesLauncher.scan(&env, &ScanCtx::default());
});
