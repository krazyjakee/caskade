//! Emulator detection (#115, EMULATION-PLAN §5). Every definition's rules
//! for the host OS are tried against standard locations, through
//! discovery's read-only OS probes (#102): `$PATH`, Flatpak installations,
//! AppImages, app bundles, Emulator desktop entries and shortcuts, and the
//! folders the definitions list. A user folder is walked for the
//! definitions' executables. Nothing is ever run.

use super::{
    Catalog, Emulator, EmulatorDefinition, EmulatorInstall, EmulatorProfile, InstallKind,
    OsFolders, VarContext, vars::Expander,
};
use caskade_discovery::{
    HostEnv, LaunchTarget, OperatingSystem, ScanCtx, SourceError, env::KnownFolder, probes,
};
use regex::{Regex, RegexBuilder};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    fs,
    path::{Path, PathBuf},
};

/// A profile of a detected install and whether its required files exist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct DetectedProfile {
    pub name: String,
    pub available: bool,
}

/// One install of one definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct DetectedInstall {
    /// Stable for one detection: definition, kind and where it is.
    pub key: String,
    pub definition_id: String,
    pub name: String,
    pub install: EmulatorInstall,
    pub version: Option<String>,
    pub profiles: Vec<DetectedProfile>,
    /// An emulator with this definition and install already exists.
    pub configured: bool,
}

/// Folder walks stop here.
const WALK_DEPTH: usize = 3;
const USER_DEPTH: usize = 6;
const ENTRY_BUDGET: usize = 50_000;

/// A definition's compiled rules for one OS.
struct Rules<'a> {
    definition: &'a EmulatorDefinition,
    executable: Option<Regex>,
    appimage: Option<Regex>,
}

fn compile(pattern: Option<&str>, case_insensitive: bool) -> Option<Regex> {
    RegexBuilder::new(pattern?)
        .case_insensitive(case_insensitive)
        .build()
        .ok()
}

/// The exact file name a simple `^name\.exe$` regex matches, for shortcuts.
fn literal(regex: &str) -> Option<String> {
    let inner = regex
        .strip_prefix('^')?
        .strip_suffix('$')?
        .replace(r"\.", ".");
    (!inner.is_empty()
        && !inner.contains([
            '\\', '*', '+', '?', '(', ')', '[', ']', '|', '{', '}', '^', '$',
        ]))
    .then_some(inner)
}

/// The OS folders as the definitions' variables see them.
pub fn folders(env: &HostEnv) -> OsFolders {
    let folder = |which| env.known_folder(which).map(Path::to_path_buf);
    let os = env.os();
    OsFolders {
        user_home: Some(env.home().to_path_buf()),
        xdg_config: (os == OperatingSystem::Linux)
            .then(|| folder(KnownFolder::ConfigHome))
            .flatten(),
        app_data: (os == OperatingSystem::Windows)
            .then(|| folder(KnownFolder::AppData))
            .flatten(),
        program_files: (os == OperatingSystem::Windows)
            .then(|| folder(KnownFolder::ProgramFiles))
            .flatten(),
        app_support: (os == OperatingSystem::Macos)
            .then(|| folder(KnownFolder::DataHome))
            .flatten(),
    }
}

/// Finds every install of every definition, newest rules first. `folder`
/// adds a recursive walk of a user-chosen folder.
pub fn detect<'a>(
    catalog: &'a Catalog,
    configured: &[Emulator],
    env: &HostEnv,
    ctx: &ScanCtx,
    folder: Option<&Path>,
) -> Result<Vec<DetectedInstall>, SourceError> {
    let os = env.os();
    let windows = os == OperatingSystem::Windows;
    let folders = folders(env);
    let rules: Vec<Rules<'a>> = catalog
        .emulators()
        .filter_map(|definition| {
            let detect = definition.detect.on(os)?;
            Some(Rules {
                definition,
                executable: compile(detect.executable.as_deref(), windows),
                appimage: compile(detect.appimage.as_deref(), true),
            })
        })
        .collect();
    let mut found: Vec<(&'a EmulatorDefinition, EmulatorInstall, Option<String>)> = Vec::new();
    let mut add =
        |definition: &'a EmulatorDefinition, install: EmulatorInstall, version: Option<String>| {
            if !found.iter().any(|(other, existing, _)| {
                std::ptr::eq(*other, definition) && *existing == install
            }) {
                found.push((definition, install, version));
            }
        };

    // $PATH binaries.
    for rule in &rules {
        let names: Vec<&str> = rule
            .definition
            .detect
            .on(os)
            .map(|detect| detect.path.iter().map(String::as_str).collect())
            .unwrap_or_default();
        if names.is_empty() {
            continue;
        }
        for path in probes::path::executables(env, ctx, &names)? {
            let leaf = path.file_stem().unwrap_or_default().to_string_lossy();
            if let Some(name) = names.iter().find(|name| {
                name.eq_ignore_ascii_case(&leaf)
                    || **name == path.file_name().unwrap_or_default().to_string_lossy()
            }) {
                add(
                    rule.definition,
                    EmulatorInstall::Path {
                        binary: (*name).to_owned(),
                    },
                    None,
                );
            }
        }
    }

    // Flatpaks, by app id.
    if os == OperatingSystem::Linux {
        for rule in &rules {
            if let Some(id) = rule
                .definition
                .detect
                .on(os)
                .and_then(|d| d.flatpak.as_deref())
                && let Some(app) = probes::flatpak::find(env, ctx, id)?
            {
                add(
                    rule.definition,
                    EmulatorInstall::Flatpak {
                        app_id: id.to_owned(),
                    },
                    app.version,
                );
            }
        }
        // AppImages in ~/Applications and ~/.local/bin, by file name.
        let images = probes::appimage::applications(env, ctx)?;
        for rule in &rules {
            let Some(pattern) = &rule.appimage else {
                continue;
            };
            for image in &images {
                let name = image
                    .location
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy();
                if pattern.is_match(&name) {
                    add(
                        rule.definition,
                        EmulatorInstall::AppImage {
                            file: image.location.clone(),
                        },
                        image.version.clone(),
                    );
                }
            }
        }
    }

    // macOS bundles, by bundle id.
    if os == OperatingSystem::Macos {
        let bundles = probes::bundles::applications(env, ctx)?;
        for rule in &rules {
            let Some(id) = rule
                .definition
                .detect
                .on(os)
                .and_then(|d| d.bundle.as_deref())
            else {
                continue;
            };
            for bundle in bundles
                .iter()
                .filter(|app| app.identifier.as_deref() == Some(id))
            {
                add(
                    rule.definition,
                    EmulatorInstall::AppBundle {
                        bundle: bundle.location.clone(),
                    },
                    bundle.version.clone(),
                );
            }
        }
    }

    // The folders each definition lists, then Emulator desktop entries and
    // shortcuts to a definition's executable.
    let context = VarContext {
        folders: folders.clone(),
        ..VarContext::default()
    };
    for rule in &rules {
        let Some(pattern) = &rule.executable else {
            continue;
        };
        let locations = rule
            .definition
            .detect
            .on(os)
            .map(|detect| detect.locations.as_slice())
            .unwrap_or_default();
        let expander = Expander {
            vars: &rule.definition.vars,
            os,
            kind: InstallKind::Folder,
            context: &context,
        };
        for location in locations {
            // A location naming an OS folder this machine lacks is skipped.
            let Ok(dir) = expander.expand(location) else {
                continue;
            };
            // Absolute for the target OS (`C:/…` too); the env maps it.
            if !crate::model::validation::absolute(Path::new(&dir)) {
                continue;
            }
            let Ok(dir) = env.resolve_path(Path::new(&dir)) else {
                continue;
            };
            let mut budget = ENTRY_BUDGET;
            for file in walk(ctx, &dir, WALK_DEPTH, &mut budget, &|name| {
                pattern.is_match(name)
            })? {
                add(rule.definition, folder_install(&file), None);
            }
        }
    }
    let shortcut_names: Vec<String> = rules
        .iter()
        .filter_map(|rule| {
            rule.definition
                .detect
                .on(os)?
                .executable
                .as_deref()
                .and_then(literal)
        })
        .collect();
    let shortcut_names: Vec<&str> = shortcut_names.iter().map(String::as_str).collect();
    for candidate in probes::emulator_candidates(env, ctx, &shortcut_names)? {
        let LaunchTarget::Exec { program, .. } = &candidate.application.launch else {
            if let LaunchTarget::Flatpak { app_id, .. } = &candidate.application.launch
                && let Some(rule) = rules.iter().find(|rule| {
                    rule.definition
                        .detect
                        .on(os)
                        .and_then(|d| d.flatpak.as_deref())
                        == Some(app_id)
                })
            {
                add(
                    rule.definition,
                    EmulatorInstall::Flatpak {
                        app_id: app_id.clone(),
                    },
                    None,
                );
            }
            continue;
        };
        let leaf = program
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        for rule in &rules {
            let on_path = rule
                .definition
                .detect
                .on(os)
                .is_some_and(|detect| detect.path.contains(&leaf));
            if program.is_absolute() {
                if rule
                    .executable
                    .as_ref()
                    .is_some_and(|pattern| pattern.is_match(&leaf))
                {
                    let file = env
                        .resolve_path(program)
                        .unwrap_or_else(|_| program.clone());
                    if file.is_file() {
                        add(rule.definition, folder_install(&file), None);
                    }
                }
            } else if on_path {
                add(
                    rule.definition,
                    EmulatorInstall::Path {
                        binary: leaf.clone(),
                    },
                    None,
                );
            }
        }
    }

    // A user folder: any definition's executable, at any depth.
    if let Some(folder) = folder {
        let folder = env
            .resolve_path(folder)
            .map_err(|e| SourceError::io(folder, e))?;
        let mut budget = ENTRY_BUDGET;
        let matches = |name: &str| {
            rules
                .iter()
                .any(|rule| rule.executable.as_ref().is_some_and(|p| p.is_match(name)))
        };
        for file in walk(ctx, &folder, USER_DEPTH, &mut budget, &matches)? {
            let name = file
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            for rule in &rules {
                if rule.executable.as_ref().is_some_and(|p| p.is_match(&name)) {
                    add(rule.definition, folder_install(&file), None);
                }
            }
        }
    }

    // A macOS app's inner executable is the bundle already found.
    let bundles: Vec<(&str, PathBuf)> = found
        .iter()
        .filter_map(|(definition, install, _)| match install {
            EmulatorInstall::AppBundle { bundle } => Some((definition.id.as_str(), bundle.clone())),
            _ => None,
        })
        .collect();
    found.retain(|(definition, install, _)| match install {
        EmulatorInstall::Folder { dir, .. } => !bundles
            .iter()
            .any(|(id, bundle)| *id == definition.id && dir.starts_with(bundle)),
        _ => true,
    });

    Ok(found
        .into_iter()
        .map(|(definition, install, version)| {
            let profiles = profiles(definition, &install, os, &folders, env);
            let configured = configured.iter().any(|emulator| {
                emulator.definition.as_deref() == Some(definition.id.as_str())
                    && emulator.install == install
            });
            DetectedInstall {
                key: key(&definition.id, &install),
                definition_id: definition.id.clone(),
                name: definition.name.clone(),
                install,
                version,
                profiles,
                configured,
            }
        })
        .collect())
}

fn folder_install(file: &Path) -> EmulatorInstall {
    EmulatorInstall::Folder {
        dir: file.parent().unwrap_or(Path::new("")).to_path_buf(),
        executable: PathBuf::from(file.file_name().unwrap_or_default()),
    }
}

fn key(definition: &str, install: &EmulatorInstall) -> String {
    let place = match install {
        EmulatorInstall::Folder { dir, executable } => dir.join(executable),
        EmulatorInstall::Path { binary } => PathBuf::from(binary),
        EmulatorInstall::Flatpak { app_id } => PathBuf::from(app_id),
        EmulatorInstall::AppImage { file } => file.clone(),
        EmulatorInstall::AppBundle { bundle } => bundle.clone(),
    };
    format!("{definition}:{}:{}", install.kind().key(), place.display())
}

/// Every definition profile and whether its required files exist.
fn profiles(
    definition: &EmulatorDefinition,
    install: &EmulatorInstall,
    os: OperatingSystem,
    folders: &OsFolders,
    env: &HostEnv,
) -> Vec<DetectedProfile> {
    let emulator_dir = match install {
        EmulatorInstall::Folder { dir, .. } => Some(dir.clone()),
        EmulatorInstall::Path { binary } => {
            probes::path::executables(env, &ScanCtx::new(Default::default()), &[binary])
                .ok()
                .and_then(|found| found.into_iter().next())
                .and_then(|path| path.parent().map(Path::to_path_buf))
        }
        EmulatorInstall::Flatpak { app_id } => Some(env.home().join(".var/app").join(app_id)),
        EmulatorInstall::AppImage { file } => file.parent().map(Path::to_path_buf),
        EmulatorInstall::AppBundle { bundle } => Some(bundle.clone()),
    };
    let context = VarContext {
        emulator_dir,
        folders: folders.clone(),
        ..VarContext::default()
    };
    definition
        .profiles
        .iter()
        .map(|profile| DetectedProfile {
            name: profile.name.clone(),
            available: definition
                .resolve(&profile.name, os, install.kind(), &context)
                .is_ok_and(|resolved| {
                    resolved
                        .requires
                        .iter()
                        .all(|path| env.resolve_path(path).is_ok_and(|path| path.exists()))
                }),
        })
        .collect()
}

/// Files under `dir` whose names match, depth- and entry-bounded, without
/// following symlinked folders.
fn walk(
    ctx: &ScanCtx,
    dir: &Path,
    depth: usize,
    budget: &mut usize,
    matches: &dyn Fn(&str) -> bool,
) -> Result<Vec<PathBuf>, SourceError> {
    let mut found = Vec::new();
    let mut pending = vec![(dir.to_path_buf(), 0)];
    while let Some((dir, level)) = pending.pop() {
        ctx.check()?;
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if *budget == 0 {
                return Err(SourceError::Other {
                    message: "emulator folder walk entry budget exceeded".into(),
                });
            }
            *budget -= 1;
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_dir() {
                if level < depth {
                    pending.push((path, level + 1));
                }
            } else if kind.is_file() && matches(&entry.file_name().to_string_lossy()) {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}

/// The emulator an imported install becomes: the definition's available
/// profiles, in recommended order, with default arguments.
pub fn emulator_for(found: &DetectedInstall, name: String) -> Emulator {
    Emulator {
        id: String::new(),
        name,
        definition: Some(found.definition_id.clone()),
        install: found.install.clone(),
        profiles: found
            .profiles
            .iter()
            .filter(|profile| profile.available)
            .map(|profile| EmulatorProfile::built_in(&profile.name))
            .collect(),
    }
}

#[cfg(test)]
#[path = "detect_tests.rs"]
mod tests;
