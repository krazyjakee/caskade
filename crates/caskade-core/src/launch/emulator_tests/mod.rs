use super::*;
use crate::{
    bus::{Bus, BusOptions},
    emulation::{ArgsOverride, Catalog, OsFolders, PlatformSettings},
    model::Game,
};
use caskade_discovery::SourceId;
use caskade_emulation_fixtures::{FixtureOs, MachineFixture};
use std::fs;

mod resolve;
/// Launches the fake emulator for real and tracks its sessions.
#[cfg(target_os = "linux")]
mod sessions;

const PLATFORMS: &str = r#"
[[platform]]
id = "sony_playstation"
name = "Sony PlayStation"
short = "PS1"
extensions = ["cue", "chd", "pbp", "m3u", "iso", "bin", "img"]
"#;

/// The shipped RetroArch definition's Linux parts and its PS1 profile.
const RETROARCH: &str = r#"
id = "retroarch"
name = "RetroArch"
tracking = { type = "default" }

[detect.linux]
path = ["retroarch"]
flatpak = "org.libretro.RetroArch"

[vars]
cores = "{EmulatorDir}/cores"
lib = ".so"
[vars.linux]
cores = "{XdgConfig}/retroarch/cores"
[vars.linux.flatpak]
cores = "{UserHome}/.var/app/org.libretro.RetroArch/config/retroarch/cores"

[display]
fullscreen = ["--fullscreen"]

[[profile]]
name = "Beetle PSX HW"
platforms = ["sony_playstation"]
extensions = ["chd", "cue", "m3u", "pbp"]
args = ["-L", "{cores}/mednafen_psx_hw_libretro{lib}", "{RomPath}"]
"#;

const FLATPAK_ID: &str = "org.libretro.RetroArch";

struct Setup {
    temp: tempfile::TempDir,
    machine: MachineFixture,
    store: LibraryStore,
    service: EmulationService,
    launches: EmulatorLaunches,
}

impl Setup {
    /// A Linux machine with the fixture's Flatpak RetroArch (`filesystems=home`)
    /// and a PS1 collection outside its home.
    async fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let machine = MachineFixture::new(FixtureOs::Linux).unwrap();
        let folders = OsFolders {
            user_home: Some(machine.home.clone()),
            xdg_config: Some(machine.home.join(".config")),
            ..OsFolders::default()
        };
        let store = LibraryStore::in_memory().await.unwrap();
        let catalog =
            Catalog::load([("platforms.toml", PLATFORMS), ("retroarch.toml", RETROARCH)]).unwrap();
        let service =
            EmulationService::new(store.clone(), Bus::new(BusOptions::default()), catalog)
                .with_host(emulation::OperatingSystem::Linux, folders)
                .with_search_path(vec![temp.path().join("path-bin")]);
        let launches = EmulatorLaunches::new(store.clone(), service.clone()).with_flatpak(
            temp.path().join("bin/flatpak"),
            vec![machine.home.join(".local/share/flatpak")],
        );
        Self {
            temp,
            machine,
            store,
            service,
            launches,
        }
    }

    fn roms(&self) -> PathBuf {
        self.temp.path().join("ROMs/PS1")
    }

    /// Writes empty ROM files and returns their paths.
    fn rom_files(&self, dir: &Path, names: &[&str]) -> Vec<PathBuf> {
        fs::create_dir_all(dir).unwrap();
        names
            .iter()
            .map(|name| {
                let path = dir.join(name);
                fs::write(&path, b"").unwrap();
                path
            })
            .collect()
    }

    async fn emulator(&self, install: EmulatorInstall, profile: EmulatorProfile) {
        self.store
            .upsert_emulator(&Emulator {
                id: "ra".into(),
                name: "RetroArch".into(),
                definition: Some("retroarch".into()),
                install,
                profiles: vec![profile],
            })
            .await
            .unwrap();
    }

    async fn platform_default(&self) {
        self.store
            .set_platform_settings(&PlatformSettings {
                platform: "sony_playstation".into(),
                default: Some(psx()),
                bios_dir: None,
            })
            .await
            .unwrap();
    }

    async fn game(
        &self,
        target: EmulatorTarget,
        roms: &[PathBuf],
        args: ArgsOverride,
    ) -> GameEntry {
        let game = Game::new("Final Fantasy VII");
        self.store.upsert_game(&game).await.unwrap();
        let mut entry = GameEntry::new(&game.id, SourceId::new("roms"), &game.id);
        entry.state = InstallState::Installed;
        entry.launch = Some(LaunchSpec::Emulator {
            platform_id: "sony_playstation".into(),
            target,
            roms: roms.iter().cloned().map(GameRom::absolute).collect(),
            args,
        });
        self.store.upsert_entry(&entry).await.unwrap()
    }

    fn cores(&self) -> PathBuf {
        self.machine
            .home
            .join(".var/app/org.libretro.RetroArch/config/retroarch/cores")
    }
}

fn psx() -> ProfileRef {
    ProfileRef {
        emulator_id: "ra".into(),
        profile_id: "psx".into(),
    }
}

fn built_in(args: ArgsOverride, hooks: ProfileHooks) -> EmulatorProfile {
    EmulatorProfile::BuiltIn {
        id: "psx".into(),
        definition_profile: "Beetle PSX HW".into(),
        args,
        hooks,
    }
}

fn flatpak() -> EmulatorInstall {
    EmulatorInstall::Flatpak {
        app_id: FLATPAK_ID.into(),
    }
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn prompt(result: Result<PreparedLaunch>) -> LaunchPrompt {
    match result {
        Err(LaunchError::NeedsChoice(prompt)) => *prompt,
        Err(error) => panic!("expected a prompt, got {error}"),
        Ok(prepared) => panic!("expected a prompt, got {:?}", prepared.command),
    }
}
