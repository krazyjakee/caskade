//! Read-only Windows Add/Remove Programs discovery. Original implementation;
//! no third-party code ported. Field/visibility format references:
//! https://learn.microsoft.com/en-us/windows/win32/msi/uninstall-registry-key
//! https://learn.microsoft.com/en-us/windows/win32/msi/arpsystemcomponent
mod commands;
mod identity;
mod records;
use crate::{
    DetectedGame, Evidence, ExternalId, GameRuntime, HostEnv, LaunchTarget, LauncherStatus, OsSet,
    ScanCtx, Source, SourceError, SourceId, TrackingHint, UninstallInfo, WatchPath,
    classify::{self, Metadata},
};
use caskade_installer::host::{
    FileSystemCaps, HostCaps, PackageCapabilities, RunnerInfo, RunnerKind,
};
pub use commands::command_target;
use commands::{icon_path, resolve_path};
pub use identity::ALIAS_RULE;
pub use records::{REGISTRY_PATH, UninstallRecord, read_records};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
pub const ID: SourceId = SourceId::new("uninstall");
#[derive(Debug, Default)]
pub struct UninstallSource;
impl UninstallSource {
    pub fn new() -> Self {
        Self
    }
}
impl Source for UninstallSource {
    fn id(&self) -> SourceId {
        ID
    }
    fn hosts(&self) -> OsSet {
        OsSet::WINDOWS
    }
    fn detect(&self, _: &HostEnv) -> Vec<LauncherStatus> {
        vec![]
    }
    fn watch_paths(&self, _: &HostEnv) -> Vec<WatchPath> {
        vec![]
    }
    fn scan(&self, env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<DetectedGame>, SourceError> {
        let mut output = vec![];
        for record in read_records(env, ctx)? {
            ctx.check()?;
            if let Some(game) = game(env, ctx, record)? {
                output.push(game);
            }
        }
        Ok(output)
    }
}
fn game(
    env: &HostEnv,
    ctx: &ScanCtx,
    record: UninstallRecord,
) -> Result<Option<DetectedGame>, SourceError> {
    let mut game = DetectedGame::new(ID, record.key.to_lowercase(), &record.display_name);
    game.runtime = GameRuntime::Windows {
        prefix: None,
        runner: None,
    };
    game.install_dir = record
        .install_location
        .as_deref()
        .and_then(|s| resolve_path(env, s));
    let icon = record
        .display_icon
        .as_deref()
        .and_then(|s| icon_path(env, s));
    let executable = icon
        .as_ref()
        .filter(|p| p.extension().is_some_and(|s| s.eq_ignore_ascii_case("exe")) && p.is_file())
        .cloned();
    if game.install_dir.is_none() {
        game.install_dir = executable
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf);
    }
    game.icon = icon.filter(|p| {
        p.is_file()
            && p.extension().is_some_and(|s| {
                ["ico", "png", "jpg", "jpeg", "bmp"]
                    .iter()
                    .any(|ext| s.eq_ignore_ascii_case(ext))
            })
    });
    game.size = record.estimated_bytes;
    game.uninstall = record
        .uninstall_string
        .as_deref()
        .and_then(|s| command_target(env, s))
        .map(|target| UninstallInfo {
            target,
            quiet: record
                .quiet_uninstall_string
                .as_deref()
                .and_then(|s| command_target(env, s)),
            registry_key: Some(record.key.clone()),
        });
    game.evidence.push(Evidence::Registry {
        key: record.key.clone(),
        value: Some("DisplayName".into()),
    });
    let (owned, ids) = identity::launcher_ids(&record);
    game.ids = ids;
    if owned {
        game.evidence.push(Evidence::Rule {
            rule: ALIAS_RULE.into(),
            score: 0,
        });
        return Ok(Some(game));
    }
    let metadata = Metadata {
        title: Some(record.display_name),
        publisher: record.publisher,
        install_dir: game.install_dir.clone(),
        executable: executable.clone(),
        ..Default::default()
    };
    let classification = if let Some(dir) = game.install_dir.as_ref().filter(|p| p.is_dir()) {
        match classify::classify_path(
            env,
            &host_profile(env),
            dir,
            &metadata,
            &ctx.config().classifier,
        ) {
            Ok(result) => result,
            Err(error) => {
                ctx.warn(format!("{}: {error}", record.key));
                return Ok(None);
            }
        }
    } else {
        classify::classify_inventory(&metadata, &[], &ctx.config().classifier).map_err(|error| {
            SourceError::Other {
                message: error.to_string(),
            }
        })?
    };
    let Some(confidence) = classification.confidence() else {
        return Ok(None);
    };
    game.confidence = confidence;
    game.evidence.extend(classification.evidence);
    for warning in classification.warnings {
        ctx.warn(format!("{}: {warning}", record.key));
    }
    game.launch = classification.launch.into_iter().collect();
    if game.launch.is_empty()
        && let Some(program) = executable
    {
        game.launch.push(LaunchTarget::Exec {
            program,
            args: vec![],
            cwd: game.install_dir.clone(),
            env: BTreeMap::new(),
        });
    }
    game.tracking = Some(TrackingHint {
        install_dir: game.install_dir.clone(),
        exe_names: game
            .launch
            .iter()
            .filter_map(|target| match target {
                LaunchTarget::Exec { program, .. } => {
                    program.file_name()?.to_str().map(str::to_owned)
                }
                _ => None,
            })
            .collect(),
        ..Default::default()
    });
    Ok(Some(game))
}
/// Capabilities from the captured profile, never detection of the scanning host.
pub(crate) fn host_profile(env: &HostEnv) -> HostCaps {
    let arch = env
        .var("PROCESSOR_ARCHITECTURE")
        .and_then(|s| s.into_string().ok())
        .map(|s| {
            match s.to_ascii_lowercase().as_str() {
                "arm64" => "aarch64",
                "x86" => "x86",
                _ => "x86_64",
            }
            .to_owned()
        })
        .unwrap_or_else(|| "x86_64".into());
    HostCaps {
        os: env.os(),
        arch,
        runners: vec![RunnerInfo {
            kind: RunnerKind::Native,
            program: PathBuf::new(),
        }],
        seven_zip: None,
        proton_builds: vec![],
        filesystem: FileSystemCaps {
            writable: false,
            exec_bits: false,
            symlinks: false,
            case_sensitive: false,
        },
        packages: PackageCapabilities::default(),
    }
}
