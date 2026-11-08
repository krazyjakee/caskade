use crate::emulation::{ArgsOverride, EmulatorTarget, GameRom};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum LaunchSpec {
    Uri {
        uri: String,
    },
    Executable {
        path: PathBuf,
        args: Vec<String>,
        cwd: Option<PathBuf>,
        env: BTreeMap<String, String>,
    },
    /// Retain discovery's structured target rather than inventing a shell command.
    Flatpak {
        app_id: String,
        args: Vec<String>,
    },
    /// An emulated game: its platform, what it launches
    /// with, its ROMs (several for multi-disc) and per-game arguments.
    Emulator {
        platform_id: String,
        target: EmulatorTarget,
        roms: Vec<GameRom>,
        args: ArgsOverride,
    },
    Plugin {
        plugin_id: String,
        runner_id: String,
        payload: serde_json::Value,
    },
    /// A web game: its page, `http` or `https`, played
    /// in Caskade's game window.
    Web {
        url: String,
    },
    /// A game another device offers for streaming:
    /// the host's id and the app's name there, played through Caskade's
    /// streaming client.
    Stream {
        host: String,
        app: String,
    },
}
impl From<caskade_discovery::LaunchTarget> for LaunchSpec {
    fn from(target: caskade_discovery::LaunchTarget) -> Self {
        match target {
            caskade_discovery::LaunchTarget::Uri { uri } => Self::Uri { uri },
            caskade_discovery::LaunchTarget::Exec {
                program,
                args,
                cwd,
                env,
            } => Self::Executable {
                path: program,
                args,
                cwd,
                env,
            },
            caskade_discovery::LaunchTarget::Flatpak { app_id, args } => {
                Self::Flatpak { app_id, args }
            }
        }
    }
}
impl From<caskade_installer::LaunchTarget> for LaunchSpec {
    fn from(target: caskade_installer::LaunchTarget) -> Self {
        Self::Executable {
            path: target.path,
            args: target.args,
            cwd: Some(target.working_dir),
            env: target.env,
        }
    }
}
