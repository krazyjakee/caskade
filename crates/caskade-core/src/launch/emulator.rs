//! Emulated games (EMULATION-PLAN §4, §5.1, #121): choose the profile and the
//! disc, build the emulator's argv for its install kind, and decide how the
//! game's process is tracked. Nothing here runs a process.

use super::{EmulationLaunch, LaunchCommand, LaunchError, PreparedLaunch, Result, store_error};
use crate::{
    db::LibraryStore,
    emulation::{
        self, EmulationService, Emulator, EmulatorInstall, EmulatorProfile, EmulatorTarget,
        GameRom, InstallKind, ProfileHooks, ProfileRef, Readiness, RomLocation, Tracking,
        VarContext, Vars,
    },
    model::{GameEntry, LaunchSpec},
    settings::SettingsService,
    tracking::{TrackingMode, TrackingTarget},
};
use caskade_discovery::InstallState;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::{Path, PathBuf};

/// The user's answers when a launch asked for them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct LaunchChoice {
    /// Answers an ask-at-launch target, or a platform without a default.
    pub profile: Option<ProfileRef>,
    /// Index into the game's ROMs (the disc picker).
    pub disc: Option<u32>,
    /// The play action to start (#177); `None` is the release's default.
    #[serde(default)]
    pub action: Option<String>,
    /// Stop the game the streaming device is playing first, once Play
    /// asked ([`super::HostBusy`], #264).
    #[serde(default)]
    pub stop_host: bool,
}

/// What a launch needs to know before it can start. A list is empty when
/// that part is already decided.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct LaunchPrompt {
    pub entry_id: String,
    pub platform_id: String,
    pub profiles: Vec<ProfileOption>,
    pub discs: Vec<DiscOption>,
    /// The release's play actions, when it asks which to start (#177). It
    /// asks for them before a profile or a disc.
    #[serde(default)]
    pub actions: Vec<super::ActionOption>,
}

/// A profile that can run the game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProfileOption {
    pub emulator_id: String,
    pub emulator_name: String,
    pub profile_id: String,
    pub profile_name: String,
    /// The emulator's install is still there.
    pub installed: bool,
}

/// One disc of a multi-disc game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct DiscOption {
    pub index: u32,
    /// The ROM's label (`Disc 2`) or its file name.
    pub name: String,
}

/// Resolves `LaunchSpec::Emulator` entries against the user's emulators.
/// Follows the emulation service's OS, folders and catalog.
#[derive(Clone)]
pub struct EmulatorLaunches {
    store: LibraryStore,
    service: EmulationService,
    flatpak: PathBuf,
    /// Flatpak installations, user first; `None` uses the host's.
    installations: Option<Vec<PathBuf>>,
    /// Console mode and Desktop full screen; `None` plays windowed.
    settings: Option<SettingsService>,
}

impl EmulatorLaunches {
    pub fn new(store: LibraryStore, service: EmulationService) -> Self {
        Self {
            store,
            service,
            flatpak: "flatpak".into(),
            installations: None,
            settings: None,
        }
    }

    /// Plays fullscreen in console mode (`core.console.active`) and, with
    /// `core.emulation.desktopFullscreen`, in desktop mode too.
    pub fn with_settings(mut self, settings: SettingsService) -> Self {
        self.settings = Some(settings);
        self
    }

    /// Whether games start fullscreen now.
    async fn fullscreen(&self) -> bool {
        let Some(settings) = &self.settings else {
            return false;
        };
        settings.flag("core.console", "active").await
            || settings.flag("core.emulation", "desktopFullscreen").await
    }

    /// Another `flatpak` program and installations (tests, fixtures).
    pub fn with_flatpak(
        mut self,
        program: impl Into<PathBuf>,
        installations: Vec<PathBuf>,
    ) -> Self {
        self.flatpak = program.into();
        self.installations = Some(installations);
        self
    }

    pub async fn resolve(
        &self,
        entry: &GameEntry,
        choice: &LaunchChoice,
    ) -> Result<PreparedLaunch> {
        let Some(LaunchSpec::Emulator {
            platform_id,
            target,
            roms,
            args: game_args,
        }) = &entry.launch
        else {
            return Err(LaunchError::Invalid(crate::tr!(
                "entry is not an emulated game"
            )));
        };
        if !matches!(entry.state, InstallState::Installed) {
            return Err(LaunchError::Invalid(crate::tr!("entry is not installed")));
        }
        // Fail with the game page's message (#122) rather than at spawn.
        let game = self.service.game(&entry.id).await.map_err(store_error)?;
        if !matches!(game.readiness, Readiness::Ready | Readiness::Ask) {
            return Err(LaunchError::Invalid(
                game.message
                    .unwrap_or_else(|| crate::tr!("this game can't launch yet")),
            ));
        }
        let disc = disc(roms, choice.disc)?;
        let emulators = self.store.emulators().await.map_err(store_error)?;
        let catalog = self.service.catalog();
        let options = |only: Option<&str>| -> Vec<ProfileOption> {
            emulators
                .iter()
                .filter(|emulator| only.is_none_or(|id| emulator.id == id))
                .flat_map(|emulator| {
                    emulator.profiles.iter().filter_map(|profile| {
                        let (name, platforms) = describe(&catalog, emulator, profile)?;
                        platforms.contains(platform_id).then(|| ProfileOption {
                            emulator_id: emulator.id.clone(),
                            emulator_name: emulator.name.clone(),
                            profile_id: profile.id().to_owned(),
                            profile_name: name,
                            installed: emulation::installed(
                                &emulator.install,
                                self.service.folders(),
                                self.service.search_path(),
                            ),
                        })
                    })
                })
                .collect()
        };
        let chosen = |options: &[ProfileOption]| -> Result<Option<ProfileRef>> {
            match &choice.profile {
                Some(profile)
                    if options.iter().any(|option| {
                        option.emulator_id == profile.emulator_id
                            && option.profile_id == profile.profile_id
                    }) =>
                {
                    Ok(Some(profile.clone()))
                }
                Some(_) => Err(LaunchError::Invalid(crate::tr!(
                    "the chosen profile cannot run this game"
                ))),
                None => Ok(None),
            }
        };
        let (profile, asked) = match target {
            EmulatorTarget::Profile {
                emulator_id,
                profile_id,
            } => (
                Some(ProfileRef {
                    emulator_id: emulator_id.clone(),
                    profile_id: profile_id.clone(),
                }),
                Vec::new(),
            ),
            EmulatorTarget::PlatformDefault => {
                let default = self
                    .store
                    .get_platform_settings(platform_id)
                    .await
                    .map_err(store_error)?
                    .and_then(|settings| settings.default);
                (default, Vec::new())
            }
            EmulatorTarget::AskProfile { emulator_id } => {
                let options = options(Some(emulator_id));
                (chosen(&options)?, options)
            }
            EmulatorTarget::AskEmulator => {
                let options = options(None);
                (chosen(&options)?, options)
            }
        };
        if profile.is_none() && asked.is_empty() {
            return Err(LaunchError::Invalid(crate::tr!(
                "no configured emulator can run {platform} games",
                platform = platform_id
            )));
        }
        let (Some(profile), Some(disc)) = (profile.clone(), disc) else {
            return Err(LaunchError::NeedsChoice(Box::new(LaunchPrompt {
                entry_id: entry.id.clone(),
                platform_id: platform_id.clone(),
                profiles: if profile.is_none() { asked } else { Vec::new() },
                discs: if disc.is_none() {
                    discs(roms)
                } else {
                    Vec::new()
                },
                actions: Vec::new(),
            })));
        };
        let emulator = emulators
            .iter()
            .find(|emulator| emulator.id == profile.emulator_id)
            .ok_or_else(|| {
                LaunchError::Invalid(crate::tr!("the game's emulator no longer exists"))
            })?;
        let emulator_profile = emulator.profile(&profile.profile_id).ok_or_else(|| {
            LaunchError::Invalid(crate::tr!("the game's emulator profile no longer exists"))
        })?;
        let rom = roms[disc]
            .path(&self.store)
            .await
            .map_err(|error| match error {
                crate::Error::Invalid(message) => LaunchError::Invalid(message),
                error => store_error(error),
            })?;
        if !rom.is_file() {
            return Err(LaunchError::Invalid(crate::tr!(
                "the game's ROM file is missing"
            )));
        }
        let name = self
            .store
            .game(&entry.game_id)
            .await
            .map_err(store_error)?
            .map(|game| game.title);
        let context = VarContext {
            rom: Some(rom.clone()),
            emulator_dir: emulation::emulator_dir(
                &emulator.install,
                self.service.folders(),
                self.service.search_path(),
            ),
            name,
            platform: Some(platform_id.clone()),
            game_id: Some(entry.game_id.clone()),
            caskade_dir: self.service.caskade_dir().map(Path::to_path_buf),
            storage_root: None,
            folders: self.service.folders().clone(),
        };
        let templates = templates(&catalog, emulator, emulator_profile)?;
        let expand = Expand {
            vars: templates.vars,
            os: self.service.os(),
            kind: emulator.install.kind(),
            context: &context,
        };
        let display = match templates.display {
            Some(display) if self.fullscreen().await => &display.fullscreen,
            Some(display) => &display.windowed,
            None => &Vec::new(),
        };
        let display = expand.all(display)?;
        let args = expand.all(&game_args.apply(&templates.args))?;
        let working_dir = templates
            .working_dir
            .as_deref()
            .map(|dir| expand.one(dir))
            .transpose()?;
        let env = templates
            .env
            .iter()
            .map(|(key, value)| Ok((key.clone(), expand.one(value)?)))
            .collect::<Result<_>>()?;
        let hook =
            |argv: &Option<Vec<String>>| argv.as_deref().map(|argv| expand.all(argv)).transpose();
        let (before, after_exit) = (
            hook(&templates.hooks.before)?,
            hook(&templates.hooks.after_exit)?,
        );

        let rom_dir = rom.parent().unwrap_or(Path::new("/"));
        let (program, prefix, install_dir) = match (&templates.executable, &emulator.install) {
            (Some(executable), _) => (executable.clone(), Vec::new(), None),
            (None, install) => self.program(install, rom_dir)?,
        };
        let mut command = LaunchCommand::executable(program.clone());
        command.args = prefix;
        command.args.extend(display);
        command.args.extend(args);
        command.cwd = working_dir.map(PathBuf::from).or(install_dir);
        command.env = env;
        command.emulation = Some(EmulationLaunch {
            emulator_id: emulator.id.clone(),
            profile_id: profile.profile_id.clone(),
            platform_id: platform_id.clone(),
            rom,
            before,
            after_exit,
        });
        command.validate()?;

        let tracking = match templates.tracking {
            Tracking::Default if templates.executable.is_some() => Tracking::ProcessTree,
            Tracking::Default => match emulator.install.kind() {
                InstallKind::Flatpak => Tracking::ProcessName {
                    name: String::new(),
                },
                InstallKind::AppBundle => Tracking::OriginalProcess,
                InstallKind::Folder | InstallKind::Path | InstallKind::AppImage => {
                    Tracking::ProcessTree
                }
            },
            other => other,
        };
        let mut target = TrackingTarget::default();
        let direct = match tracking {
            Tracking::ProcessTree | Tracking::Default => {
                target.mode = TrackingMode::ProcessTree;
                true
            }
            Tracking::OriginalProcess => {
                target.mode = TrackingMode::OriginalProcess;
                true
            }
            Tracking::Folder { path } => {
                target.mode = TrackingMode::Folder;
                target.real_dir = Some(PathBuf::from(expand.one(&path)?));
                false
            }
            Tracking::ProcessName { name } if !name.is_empty() => {
                target.mode = TrackingMode::ProcessName;
                target.exe_names = vec![expand.one(&name)?];
                false
            }
            Tracking::ProcessName { .. } => {
                // The install kind's default for a Flatpak: the sandboxed
                // emulator is not our child, so find it by name.
                target.mode = TrackingMode::ProcessName;
                target.exe_names = match &emulator.install {
                    EmulatorInstall::Flatpak { app_id } => self.flatpak_names(
                        app_id,
                        emulator
                            .definition
                            .as_deref()
                            .and_then(|id| catalog.emulator(id)),
                    ),
                    _ => Vec::new(),
                };
                false
            }
        };
        if !matches!(target.mode, TrackingMode::Folder) {
            target.executables.push(program);
        }
        tracing::info!(
            emulator = %emulator.id,
            profile = %profile.profile_id,
            kind = ?emulator.install.kind(),
            tracking = ?target.mode,
            "resolved emulator launch"
        );
        Ok(PreparedLaunch::new(command, target, direct))
    }

    /// The program, the tokens before the profile's arguments and the
    /// default working directory for an install (§5.1).
    fn program(
        &self,
        install: &EmulatorInstall,
        rom_dir: &Path,
    ) -> Result<(PathBuf, Vec<String>, Option<PathBuf>)> {
        Ok(match install {
            EmulatorInstall::Folder { dir, executable } => {
                (dir.join(executable), Vec::new(), Some(dir.clone()))
            }
            EmulatorInstall::Path { binary } => (
                emulation::which(binary, self.service.search_path()).ok_or_else(|| {
                    LaunchError::Invalid(crate::tr!("`{binary}` is not on PATH", binary = binary))
                })?,
                Vec::new(),
                None,
            ),
            EmulatorInstall::Flatpak { app_id } => {
                if !super::command::flatpak_id(app_id) {
                    return Err(LaunchError::Invalid(crate::tr!("invalid Flatpak app id")));
                }
                let mut prefix = vec!["run".to_owned()];
                if !self.flatpak_sees(app_id, rom_dir) {
                    let dir = rom_dir
                        .to_str()
                        .filter(|dir| !dir.contains(':'))
                        .ok_or_else(|| {
                            LaunchError::Invalid(crate::tr!(
                                "the ROM folder can't be shared with a Flatpak"
                            ))
                        })?;
                    prefix.push(format!("--filesystem={dir}:ro"));
                }
                prefix.push(app_id.clone());
                (self.flatpak.clone(), prefix, None)
            }
            EmulatorInstall::AppImage { file } => (file.clone(), Vec::new(), None),
            EmulatorInstall::AppBundle { bundle } => (bundle_executable(bundle), Vec::new(), None),
        })
    }

    fn installations(&self) -> Vec<PathBuf> {
        self.installations.clone().unwrap_or_else(|| {
            let user = self
                .service
                .folders()
                .user_home
                .as_ref()
                .map(|home| home.join(".local/share/flatpak"));
            user.into_iter()
                .chain([PathBuf::from("/var/lib/flatpak")])
                .collect()
        })
    }

    /// Whether the app's own permissions and overrides already expose `dir`.
    pub fn flatpak_sees(&self, app_id: &str, dir: &Path) -> bool {
        let installations = self.installations();
        let mut grants: Vec<String> = Vec::new();
        if let Some(metadata) = flatpak_metadata(&installations, app_id).next() {
            apply_grants(&mut grants, &metadata);
        }
        // System overrides apply first, the user's last.
        for install in installations.iter().rev() {
            for name in ["global", app_id] {
                if let Ok(text) = std::fs::read_to_string(install.join("overrides").join(name)) {
                    apply_grants(&mut grants, &text);
                }
            }
        }
        let home = self.service.folders().user_home.as_deref();
        let dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        grants.iter().any(|grant| {
            grant_root(grant, home).is_some_and(|root| {
                let root = std::fs::canonicalize(&root).unwrap_or(root);
                dir.starts_with(root)
            })
        })
    }

    /// Process names the sandboxed emulator may run as: the app's
    /// `command`, the definition's `$PATH` names, and the id's last part.
    fn flatpak_names(
        &self,
        app_id: &str,
        definition: Option<&emulation::EmulatorDefinition>,
    ) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        if let Some(command) = flatpak_metadata(&self.installations(), app_id)
            .find_map(|text| keyfile_value(&text, "Application", "command"))
        {
            names.extend(
                Path::new(&command)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_owned),
            );
        }
        if let Some(rules) = definition.and_then(|definition| definition.detect.linux.as_ref()) {
            names.extend(rules.path.iter().cloned());
        }
        names.extend(app_id.rsplit('.').next().map(str::to_ascii_lowercase));
        names.retain(|name| !name.is_empty());
        let mut unique = Vec::new();
        for name in names {
            if !unique.contains(&name) {
                unique.push(name);
            }
        }
        unique
    }
}

/// The ROM to launch: a playlist when the game has one, the only ROM, or the
/// chosen disc. `None` asks.
fn disc(roms: &[GameRom], chosen: Option<u32>) -> Result<Option<usize>> {
    if roms.is_empty() {
        return Err(LaunchError::Invalid(crate::tr!("the game has no ROM")));
    }
    if let Some(index) = chosen {
        let index = index as usize;
        return if index < roms.len() {
            Ok(Some(index))
        } else {
            Err(LaunchError::Invalid(crate::tr!(
                "the chosen disc doesn't exist"
            )))
        };
    }
    if let Some(playlist) = roms.iter().position(|rom| {
        rom_file(rom)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("m3u"))
    }) {
        return Ok(Some(playlist));
    }
    Ok((roms.len() == 1).then_some(0))
}

fn discs(roms: &[GameRom]) -> Vec<DiscOption> {
    roms.iter()
        .enumerate()
        .map(|(index, rom)| DiscOption {
            index: index as u32,
            name: rom.name.clone().unwrap_or_else(|| {
                rom_file(rom)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default()
            }),
        })
        .collect()
}

fn rom_file(rom: &GameRom) -> &Path {
    match &rom.location {
        RomLocation::Absolute { path } | RomLocation::Scanner { path, .. } => path,
    }
}

/// A profile's display name and platforms.
fn describe(
    catalog: &emulation::Catalog,
    emulator: &Emulator,
    profile: &EmulatorProfile,
) -> Option<(String, Vec<String>)> {
    match profile {
        EmulatorProfile::BuiltIn {
            definition_profile, ..
        } => {
            let definition = catalog.emulator(emulator.definition.as_deref()?)?;
            let base = definition.profile(definition_profile)?;
            Some((base.name.clone(), base.platforms.clone()))
        }
        EmulatorProfile::Custom {
            name, platforms, ..
        } => Some((name.clone(), platforms.clone())),
    }
}

/// A profile's templates before variables are expanded.
struct Templates<'a> {
    vars: &'a Vars,
    args: Vec<String>,
    working_dir: Option<String>,
    executable: Option<PathBuf>,
    env: Vec<(String, String)>,
    tracking: Tracking,
    hooks: ProfileHooks,
    /// The definition's fullscreen and windowed flags; custom profiles
    /// have none.
    display: Option<&'a emulation::Display>,
}

fn templates<'a>(
    catalog: &'a emulation::Catalog,
    emulator: &Emulator,
    profile: &EmulatorProfile,
) -> Result<Templates<'a>> {
    match profile {
        EmulatorProfile::BuiltIn {
            definition_profile,
            args,
            hooks,
            ..
        } => {
            let definition = emulator
                .definition
                .as_deref()
                .and_then(|id| catalog.emulator(id))
                .ok_or_else(|| {
                    LaunchError::Invalid(crate::tr!("the emulator's definition is not available"))
                })?;
            let base = definition.profile(definition_profile).ok_or_else(|| {
                LaunchError::Invalid(crate::tr!(
                    "{emulator} has no profile {profile}",
                    emulator = definition.name,
                    profile = format!("{definition_profile:?}")
                ))
            })?;
            Ok(Templates {
                vars: &definition.vars,
                display: Some(&definition.display),
                args: args.apply(&base.args),
                working_dir: base.working_dir.clone(),
                executable: None,
                env: Vec::new(),
                tracking: base
                    .tracking
                    .clone()
                    .or_else(|| definition.tracking.clone())
                    .unwrap_or_default(),
                hooks: hooks.clone(),
            })
        }
        EmulatorProfile::Custom {
            executable,
            args,
            working_dir,
            env,
            tracking,
            hooks,
            ..
        } => Ok(Templates {
            vars: Vars::NONE,
            display: None,
            args: args.clone(),
            working_dir: working_dir.clone(),
            executable: executable.clone(),
            env: env.clone(),
            tracking: tracking.clone(),
            hooks: hooks.clone(),
        }),
    }
}

/// Expands templates with one launch's variables.
struct Expand<'a> {
    vars: &'a Vars,
    os: emulation::OperatingSystem,
    kind: InstallKind,
    context: &'a VarContext,
}

impl Expand<'_> {
    fn all(&self, templates: &[String]) -> Result<Vec<String>> {
        emulation::resolve(
            self.vars,
            templates,
            None,
            &[],
            self.os,
            self.kind,
            self.context,
        )
        .map(|resolved| resolved.args)
        .map_err(LaunchError::Invalid)
    }

    fn one(&self, template: &str) -> Result<String> {
        Ok(self.all(&[template.to_owned()])?.remove(0))
    }
}

/// The app's metadata keyfile in each installation that has it.
fn flatpak_metadata<'a>(
    installations: &'a [PathBuf],
    app_id: &'a str,
) -> impl Iterator<Item = String> + 'a {
    installations.iter().filter_map(move |install| {
        std::fs::read_to_string(
            install
                .join("app")
                .join(app_id)
                .join("current/active/metadata"),
        )
        .ok()
    })
}

/// Adds a keyfile's `[Context] filesystems`; `!name` removes a grant.
fn apply_grants(grants: &mut Vec<String>, keyfile: &str) {
    let Some(value) = keyfile_value(keyfile, "Context", "filesystems") else {
        return;
    };
    for item in value
        .split(';')
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        match item.strip_prefix('!') {
            Some(removed) => {
                let removed = strip_mode(removed);
                grants.retain(|grant| strip_mode(grant) != removed);
            }
            None => grants.push(item.to_owned()),
        }
    }
}

fn keyfile_value(text: &str, section: &str, key: &str) -> Option<String> {
    let mut current = "";
    for line in text.lines().map(str::trim) {
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            current = name;
        } else if current == section
            && let Some((name, value)) = line.split_once('=')
            && name.trim() == key
        {
            return Some(value.trim().to_owned());
        }
    }
    None
}

fn strip_mode(grant: &str) -> &str {
    match grant.rsplit_once(':') {
        Some((path, "ro" | "rw" | "create")) => path,
        _ => grant,
    }
}

/// The host folder a `filesystems` grant exposes.
fn grant_root(grant: &str, home: Option<&Path>) -> Option<PathBuf> {
    let grant = strip_mode(grant);
    let (name, rest) = grant.split_once('/').unwrap_or((grant, ""));
    let base = match name {
        "host" => return Some(PathBuf::from("/")),
        "home" | "~" => home?.to_path_buf(),
        "" => PathBuf::from("/"),
        "xdg-config" => home?.join(".config"),
        "xdg-data" => home?.join(".local/share"),
        "xdg-cache" => home?.join(".cache"),
        _ => {
            let dir = name.strip_prefix("xdg-")?;
            home?.join(user_dir(home?, dir)?)
        }
    };
    Some(if rest.is_empty() {
        base
    } else {
        base.join(rest)
    })
}

/// An XDG user directory (`xdg-download` → `~/Downloads`), honouring
/// `~/.config/user-dirs.dirs`.
fn user_dir(home: &Path, name: &str) -> Option<PathBuf> {
    let (key, default) = match name {
        "desktop" => ("DESKTOP", "Desktop"),
        "documents" => ("DOCUMENTS", "Documents"),
        "download" => ("DOWNLOAD", "Downloads"),
        "music" => ("MUSIC", "Music"),
        "pictures" => ("PICTURES", "Pictures"),
        "videos" => ("VIDEOS", "Videos"),
        "public-share" => ("PUBLICSHARE", "Public"),
        "templates" => ("TEMPLATES", "Templates"),
        _ => return None,
    };
    let configured = std::fs::read_to_string(home.join(".config/user-dirs.dirs"))
        .ok()
        .and_then(|text| {
            text.lines().find_map(|line| {
                let value = line.trim().strip_prefix(&format!("XDG_{key}_DIR="))?;
                let value = value.trim_matches('"');
                Some(match value.strip_prefix("$HOME/") {
                    Some(relative) => home.join(relative),
                    None => PathBuf::from(value),
                })
            })
        });
    Some(configured.unwrap_or_else(|| home.join(default)))
}

/// A `.app`'s executable, from `CFBundleExecutable` (not `open -a`, so the
/// launched process is the emulator itself).
fn bundle_executable(bundle: &Path) -> PathBuf {
    let limits = caskade_discovery::parsers::Limits {
        max_input_bytes: 1 << 20,
        max_string_bytes: 65_536,
        max_nodes: 20_000,
        max_depth: 32,
    };
    let name = std::fs::read(bundle.join("Contents/Info.plist"))
        .ok()
        .and_then(|bytes| caskade_discovery::parsers::plist::parse(&bytes, &limits).ok())
        .and_then(|value| {
            value
                .as_dictionary()?
                .get("CFBundleExecutable")?
                .as_string()
                .map(str::to_owned)
        })
        .filter(|name| {
            !name.is_empty() && !name.contains(['/', '\\']) && name != ".." && name != "."
        })
        .unwrap_or_else(|| {
            bundle
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
    bundle.join("Contents/MacOS").join(name)
}

#[cfg(all(test, unix))]
#[path = "emulator_tests/mod.rs"]
mod tests;
