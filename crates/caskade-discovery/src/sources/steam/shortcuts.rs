//! Steam non-Steam shortcuts (§5, #73): `userdata/<user>/config/shortcuts.vdf`
//! of every install and user. The shortcut app id and `rungameid` follow
//! Steam (steamlocate's computation when the file has no `appid`).
//!
//! Shortcuts that start another launcher (Heroic, Legendary, Lutris, a
//! Flatpak launcher, `steam://`) carry that launcher's URIs and are marked
//! as proxies so the merge step folds them into the launcher's game.
//! Emulator shortcuts are left to emulator detection.

use super::{
    super::{is_dir, is_file, join, list, read, resolve, tokens},
    roots, runtime, users, vdf,
};
use crate::{
    OperatingSystem,
    env::HostEnv,
    parsers::{Limits, binary_vdf},
    types::{
        DetectedGame, Evidence, ExternalId, LaunchTarget, LauncherStatus, OsSet, ScanCtx, Source,
        SourceError, SourceId, TrackingHint, WatchPath,
    },
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub const ID: SourceId = SourceId::new("steam.shortcuts");

/// The evidence rule that marks a shortcut which only starts another
/// launcher's game.
pub const PROXY_RULE: &str = "steam.shortcut.proxy";

const LAUNCHER_PROGRAMS: &[&str] = &[
    "heroic",
    "legendary",
    "rare",
    "lutris",
    "steam",
    "epicgameslauncher",
    "galaxyclient",
    "bottles-cli",
    "minigalaxy",
    "itch",
    "faugus-run",
];

const LAUNCHER_FLATPAKS: &[&str] = &[
    "com.heroicgameslauncher.hgl",
    "net.lutris.Lutris",
    "com.valvesoftware.Steam",
    "com.usebottles.bottles",
    "io.github.sharkwouter.Minigalaxy",
    "io.itch.itch",
    "io.github.Faugus.faugus-launcher",
];

const EMULATOR_PROGRAMS: &[&str] = &[
    "retroarch",
    "dolphin-emu",
    "dolphin",
    "pcsx2",
    "pcsx2-qt",
    "rpcs3",
    "cemu",
    "ryujinx",
    "yuzu",
    "citra",
    "citra-qt",
    "duckstation",
    "duckstation-qt",
    "ppsspp",
    "ppssppsdl",
    "ppssppqt",
    "xemu",
    "mame",
    "melonds",
    "mgba",
    "mgba-qt",
    "flycast",
    "redream",
    "xenia",
    "xenia_canary",
    "vita3k",
    "azahar",
    "shadps4",
    "emulationstation",
    "es-de",
];

const EMULATOR_FLATPAKS: &[&str] = &[
    "org.libretro.RetroArch",
    "org.DolphinEmu.dolphin-emu",
    "net.pcsx2.PCSX2",
    "net.rpcs3.RPCS3",
    "info.cemu.Cemu",
    "org.ryujinx.Ryujinx",
    "org.citra_emu.citra",
    "org.duckstation.DuckStation",
    "org.ppsspp.PPSSPP",
    "app.xemu.xemu",
    "org.mamedev.MAME",
    "net.kuribo64.melonDS",
    "io.mgba.mGBA",
    "org.flycast.Flycast",
    "net.shadps4.shadPS4",
    "org.azahar_emu.Azahar",
    "org.es_de.frontend",
];

#[derive(Debug, Default)]
pub struct SteamShortcuts;

impl Source for SteamShortcuts {
    fn id(&self) -> SourceId {
        ID
    }

    fn hosts(&self) -> OsSet {
        OsSet::ALL
    }

    /// Steam itself is reported by the `steam` source.
    fn detect(&self, _env: &HostEnv) -> Vec<LauncherStatus> {
        Vec::new()
    }

    fn scan(&self, env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<DetectedGame>, SourceError> {
        let mut games: BTreeMap<u32, DetectedGame> = BTreeMap::new();
        for root in roots::roots(env) {
            ctx.check()?;
            let tools = users::compat_tools(env, &root.path, ctx);
            for file in files(env, &root.path) {
                ctx.check()?;
                let bytes = match read(env, &file, LIMITS.max_input_bytes as u64) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        ctx.warn(format!("{}: {error}", file.display()));
                        continue;
                    }
                };
                let value = match binary_vdf::parse(&bytes, &LIMITS) {
                    Ok(value) => value,
                    Err(error) => {
                        ctx.warn(format!("{}: {error}", file.display()));
                        continue;
                    }
                };
                let list = vdf::child(&value, "shortcuts");
                for (_, entry) in list.map(vdf::entries).unwrap_or_default() {
                    let Some(shortcut) = Shortcut::read(entry) else {
                        ctx.warn(format!(
                            "{}: shortcut without a name or exe",
                            file.display()
                        ));
                        continue;
                    };
                    if games.contains_key(&shortcut.app_id) {
                        continue;
                    }
                    if let Some(game) = game(env, &root.path, &file, &shortcut, &tools) {
                        games.insert(shortcut.app_id, game);
                    }
                }
            }
        }
        Ok(games.into_values().collect())
    }

    fn watch_paths(&self, env: &HostEnv) -> Vec<WatchPath> {
        roots::roots(env)
            .iter()
            .flat_map(|root| files(env, &root.path))
            .map(|path| WatchPath {
                path,
                recursive: false,
            })
            .collect()
    }
}

const LIMITS: Limits = Limits {
    max_input_bytes: 16 << 20,
    max_string_bytes: 64 << 10,
    max_nodes: 200_000,
    max_depth: 16,
};

/// Every user's `shortcuts.vdf` in one install.
fn files(env: &HostEnv, root: &Path) -> Vec<PathBuf> {
    let userdata = join(root, "userdata");
    list(env, &userdata)
        .unwrap_or_default()
        .iter()
        .filter(|name| name.parse::<u64>().is_ok_and(|id| id != 0))
        .map(|name| join(&userdata, &format!("{name}/config/shortcuts.vdf")))
        .filter(|path| is_file(env, path))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Shortcut {
    app_id: u32,
    name: String,
    exe: String,
    start_dir: String,
    options: String,
    icon: String,
    last_played: Option<u64>,
}

impl Shortcut {
    fn read(entry: &binary_vdf::Value) -> Option<Self> {
        let text = |name| match vdf::child(entry, name) {
            Some(binary_vdf::Value::String(text) | binary_vdf::Value::WideString(text)) => {
                text.trim().to_owned()
            }
            _ => String::new(),
        };
        let int = |name| match vdf::child(entry, name) {
            Some(binary_vdf::Value::Int(value)) => Some(*value),
            _ => None,
        };
        let name = text("AppName");
        let exe = text("Exe");
        if name.is_empty() || exe.is_empty() {
            return None;
        }
        let start_dir = text("StartDir");
        // The file's appid is the u32 Steam uses; old files have none, and
        // Steam derives it from the exe and name.
        let app_id = int("appid")
            .map(|id| id as u32)
            .filter(|id| *id != 0)
            .unwrap_or_else(|| {
                let legacy =
                    steamlocate::Shortcut::new(0, name.clone(), exe.clone(), start_dir.clone());
                (legacy.steam_id() >> 32) as u32
            });
        Some(Self {
            app_id,
            exe,
            start_dir,
            options: text("LaunchOptions"),
            icon: text("icon"),
            last_played: int("LastPlayTime")
                .filter(|time| *time > 0)
                .map(|time| time as u64),
            name,
        })
    }

    /// `steam://rungameid/<id>`'s 64-bit game id.
    fn game_id(&self) -> u64 {
        (u64::from(self.app_id) << 32) | 0x0200_0000
    }
}

fn unquote(text: &str) -> &str {
    let text = text.trim();
    text.strip_prefix('"')
        .and_then(|text| text.strip_suffix('"'))
        .unwrap_or(text)
}

/// A program's file name, lowercased and without `.exe`.
fn program_name(program: &str) -> String {
    let name = program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(program)
        .to_ascii_lowercase();
    name.strip_suffix(".exe").map(str::to_owned).unwrap_or(name)
}

fn is_uri(token: &str) -> bool {
    let Some((scheme, rest)) = token.split_once(':') else {
        return false;
    };
    let scheme_ok = scheme.len() > 1
        && scheme.starts_with(|char: char| char.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|char| char.is_ascii_alphanumeric() || matches!(char, '+' | '-' | '.'));
    scheme_ok && (rest.starts_with("//") || scheme.eq_ignore_ascii_case("lutris"))
}

fn game(
    env: &HostEnv,
    root: &Path,
    file: &Path,
    shortcut: &Shortcut,
    tools: &BTreeMap<u32, String>,
) -> Option<DetectedGame> {
    let exe = unquote(&shortcut.exe);
    let args = tokens(&shortcut.options);
    let program = program_name(exe);
    let flatpak = (program == "flatpak" && args.first().map(String::as_str) == Some("run"))
        .then(|| {
            let mut rest = args[1..].iter().skip_while(|arg| arg.starts_with('-'));
            let app_id = rest.next()?.clone();
            Some((app_id, rest.cloned().collect::<Vec<_>>()))
        })
        .flatten();
    let emulator = EMULATOR_PROGRAMS.contains(&program.as_str())
        || flatpak
            .as_ref()
            .is_some_and(|(app_id, _)| EMULATOR_FLATPAKS.contains(&app_id.as_str()));
    if emulator {
        tracing::debug!(name = %shortcut.name, "leaving emulator shortcut to emulator detection");
        return None;
    }
    let uris: Vec<String> = std::iter::once(exe)
        .chain(args.iter().map(String::as_str))
        .map(|token| token.trim_start_matches('-'))
        .filter(|token| is_uri(token))
        .map(str::to_owned)
        .collect();
    let proxy = !uris.is_empty()
        || LAUNCHER_PROGRAMS.contains(&program.as_str())
        || flatpak
            .as_ref()
            .is_some_and(|(app_id, _)| LAUNCHER_FLATPAKS.contains(&app_id.as_str()));

    // Absolute paths are made readable; a bare command (`legendary`) stays
    // as written for the launcher's PATH lookup.
    let reported = |text: &str| {
        let text = unquote(text);
        let absolute = text.starts_with(['/', '\\'])
            || text
                .as_bytes()
                .get(1..3)
                .is_some_and(|tail| tail == b":\\" || tail == b":/");
        (!text.is_empty()).then(|| {
            absolute
                .then(|| resolve(env, text))
                .flatten()
                .unwrap_or_else(|| PathBuf::from(text))
        })
    };
    let mut game = DetectedGame::new(ID, shortcut.app_id.to_string(), shortcut.name.clone());
    game.ids = vec![ExternalId::SteamShortcut {
        app_id: shortcut.app_id,
    }];
    game.ids
        .extend(uris.iter().filter_map(|uri| steam_app(uri)));
    game.launch = vec![LaunchTarget::Uri {
        uri: format!("steam://rungameid/{}", shortcut.game_id()),
    }];
    game.launch.extend(
        uris.iter()
            .map(|uri| LaunchTarget::Uri { uri: uri.clone() }),
    );
    match flatpak {
        Some((app_id, args)) => game.launch.push(LaunchTarget::Flatpak { app_id, args }),
        None if !is_uri(exe) => game.launch.push(LaunchTarget::Exec {
            program: reported(exe)?,
            args,
            cwd: reported(&shortcut.start_dir),
            env: BTreeMap::new(),
        }),
        None => {}
    }
    if !proxy {
        // A start folder that no longer exists is not an install dir, and a
        // shortcut whose own program is gone is a broken, uncertain entry.
        game.install_dir = reported(&shortcut.start_dir).filter(|dir| is_dir(env, dir));
        let target = unquote(exe);
        let absolute = Path::new(target).has_root()
            || target
                .as_bytes()
                .get(1..3)
                .is_some_and(|tail| tail == b":\\" || tail == b":/");
        let missing = absolute && reported(exe).is_none_or(|program| !is_file(env, &program));
        if missing {
            game.state = crate::types::InstallState::Broken {
                reason: "shortcut target missing".into(),
            };
            game.confidence = crate::types::Confidence::Maybe;
        }
        game.runtime = runtime(env, root, shortcut.app_id, tools);
    } else if env.os() == OperatingSystem::Windows {
        game.runtime = crate::types::GameRuntime::Windows {
            prefix: None,
            runner: None,
        };
    }
    game.last_played_unix = shortcut.last_played;
    game.icon = reported(&shortcut.icon).filter(|icon| is_file(env, icon));
    game.evidence = vec![Evidence::File {
        path: file.to_path_buf(),
    }];
    if proxy {
        game.evidence.push(Evidence::Rule {
            rule: PROXY_RULE.into(),
            score: 0,
        });
    }
    game.tracking = Some(TrackingHint {
        steam_app_id: None,
        install_dir: game.install_dir.clone(),
        exe_names: (!proxy && !program.is_empty())
            .then(|| exe.rsplit(['/', '\\']).next().unwrap_or(exe).to_owned())
            .into_iter()
            .collect(),
    });
    Some(game)
}

/// A real Steam game behind a `steam://rungameid/<id>` shortcut.
fn steam_app(uri: &str) -> Option<ExternalId> {
    let id = uri
        .strip_prefix("steam://rungameid/")?
        .parse::<u64>()
        .ok()?;
    let app_id = u32::try_from(id).ok()?;
    Some(ExternalId::Steam { app_id })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_option_tokens() {
        assert_eq!(
            tokens(r#"--no-gui "heroic://launch/legendary/Fixture" -x "a \"b\"" """#),
            [
                "--no-gui",
                "heroic://launch/legendary/Fixture",
                "-x",
                r#"a "b""#,
                ""
            ]
        );
        assert!(tokens("   ").is_empty());
    }

    #[test]
    fn uris_and_programs() {
        assert!(is_uri("heroic://launch/legendary/x"));
        assert!(is_uri("com.epicgames.launcher://apps/x?action=launch"));
        assert!(is_uri("lutris:rungame/celeste"));
        assert!(!is_uri(r"C:\Games\x.exe"));
        assert!(!is_uri("/usr/bin/x"));
        assert_eq!(
            program_name(r"C:\Program Files\Heroic\Heroic.exe"),
            "heroic"
        );
        assert_eq!(program_name("/usr/bin/flatpak"), "flatpak");
        assert_eq!(
            steam_app("steam://rungameid/620"),
            Some(ExternalId::Steam { app_id: 620 })
        );
        assert_eq!(steam_app("steam://rungameid/9223372036888330240"), None);
    }

    #[test]
    fn app_id_from_file_or_steam_formula() {
        let entry = |appid: Option<i32>| {
            let mut pairs = vec![
                (
                    "AppName".to_owned(),
                    binary_vdf::Value::String("Fixture".into()),
                ),
                (
                    "Exe".to_owned(),
                    binary_vdf::Value::String("\"/games/fixture\"".into()),
                ),
            ];
            if let Some(appid) = appid {
                pairs.push(("appid".into(), binary_vdf::Value::Int(appid)));
            }
            binary_vdf::Value::Object(pairs)
        };
        let stored = Shortcut::read(&entry(Some(-2_147_483_000))).unwrap();
        assert_eq!(stored.app_id, 2_147_484_296);
        assert_eq!(stored.game_id(), (2_147_484_296_u64 << 32) | 0x0200_0000);
        let legacy = Shortcut::read(&entry(None)).unwrap();
        let expected = steamlocate::Shortcut::new(
            0,
            "Fixture".into(),
            "\"/games/fixture\"".into(),
            String::new(),
        );
        assert_eq!(legacy.game_id(), expected.steam_id());
        assert!(legacy.app_id & 0x8000_0000 != 0);
    }
}
