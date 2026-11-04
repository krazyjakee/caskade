//! Emulator and profile views: install presence, profile availability,
//! BIOS state and the resolved command line for a sample ROM.

use super::{EmulationService, command, emulator_dir, installed};
use crate::{
    Result,
    emulation::{
        ArgsOverride, Bios, BiosState, Catalog, Emulator, EmulatorInstall, EmulatorProfile,
        VarContext, Vars, bios_dirs, resolve,
    },
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{collections::BTreeMap, path::PathBuf};

/// A profile's command line for a sample ROM, one token per chip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct ArgvPreview {
    pub sample_rom: String,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    /// Why the command line can't be resolved, instead of a broken one.
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum Availability {
    Available,
    /// Required files (cores, …) that don't exist.
    MissingFiles {
        missing: Vec<String>,
    },
    /// The emulator itself is gone.
    NotInstalled,
    /// The profile can't be resolved, e.g. its definition is unknown.
    Invalid {
        reason: String,
    },
}

/// A Replace override made against definition args that have changed since.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct ArgsDrift {
    pub overridden_from: Vec<String>,
    pub definition_args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProfileView {
    pub id: String,
    pub name: String,
    pub platforms: Vec<String>,
    pub extensions: Vec<String>,
    pub availability: Availability,
    /// The platforms' BIOS files and the profile's own.
    pub bios: Vec<BiosEntry>,
    pub preview: ArgvPreview,
    pub drift: Option<ArgsDrift>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub enum EmulatorStatus {
    Ready,
    SomeMissing,
    NotFound,
}

/// A definition profile not yet added to an emulator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct AddableProfile {
    pub name: String,
    pub platforms: Vec<String>,
    pub available: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct EmulatorView {
    pub emulator: Emulator,
    pub status: EmulatorStatus,
    pub profiles: Vec<ProfileView>,
    /// Available ones first, then in the definition's order.
    pub addable: Vec<AddableProfile>,
}

/// A BIOS file and, where an emulator is known to look for it, its state.
/// `found`/`state` are `None` when no emulator is configured to check with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct BiosEntry {
    pub file: String,
    pub required: bool,
    pub found: Option<bool>,
    pub state: Option<BiosState>,
    /// Where it was found (or the mismatching file).
    pub path: Option<String>,
    /// The platform that lists it; `None` for a profile's own BIOS.
    pub platform: Option<String>,
}

/// A profile being edited, previewed before it is saved.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProfileDraft {
    pub install: EmulatorInstall,
    pub definition: Option<String>,
    pub profile: EmulatorProfile,
}

/// A profile's templates and what it applies to.
struct Templates<'a> {
    name: String,
    platforms: Vec<String>,
    extensions: Vec<String>,
    vars: &'a Vars,
    args: Vec<String>,
    working_dir: Option<String>,
    requires: Vec<String>,
    executable: Option<PathBuf>,
    /// The listing platform (if any) and the BIOS file.
    bios: Vec<(Option<String>, Bios)>,
    drift: Option<ArgsDrift>,
}

impl EmulationService {
    pub async fn emulators(&self) -> Result<Vec<EmulatorView>> {
        let catalog = self.catalog();
        let overrides = self.bios_overrides().await?;
        Ok(self
            .inner
            .store
            .emulators()
            .await?
            .into_iter()
            .map(|emulator| self.view(&catalog, emulator, &overrides))
            .collect())
    }

    /// Creates or updates an emulator after checking it against the
    /// definitions. A Replace override records the definition args it
    /// replaces, so later definition changes can be flagged.
    pub async fn save_emulator(&self, mut emulator: Emulator) -> Result<EmulatorView> {
        // New emulators and profiles arrive without ids.
        if emulator.id.is_empty() {
            emulator.id = crate::model::new_id();
        }
        for profile in &mut emulator.profiles {
            let (EmulatorProfile::BuiltIn { id, .. } | EmulatorProfile::Custom { id, .. }) =
                profile;
            if id.is_empty() {
                *id = crate::model::new_id();
            }
        }
        let catalog = self.catalog();
        emulator.validate(Some(&catalog))?;
        let definition = emulator
            .definition
            .as_deref()
            .and_then(|id| catalog.emulator(id));
        let previous = self.inner.store.get_emulator(&emulator.id).await?;
        for profile in &mut emulator.profiles {
            let EmulatorProfile::BuiltIn {
                id,
                definition_profile,
                args: ArgsOverride::Replace { replaced, .. },
                ..
            } = profile
            else {
                continue;
            };
            let kept = previous
                .as_ref()
                .and_then(|previous| previous.profile(id))
                .and_then(|previous| match previous {
                    EmulatorProfile::BuiltIn {
                        args: ArgsOverride::Replace { replaced, .. },
                        ..
                    } if !replaced.is_empty() => Some(replaced.clone()),
                    _ => None,
                });
            if replaced.is_empty() {
                *replaced = kept
                    .or_else(|| {
                        definition
                            .and_then(|definition| definition.profile(definition_profile))
                            .map(|profile| profile.args.clone())
                    })
                    .unwrap_or_default();
            }
        }
        let saved = self.inner.store.upsert_emulator(&emulator).await?;
        self.changed("emulators");
        self.changed("platforms");
        let overrides = self.bios_overrides().await?;
        Ok(self.view(&catalog, saved, &overrides))
    }

    pub async fn delete_emulator(&self, id: &str) -> Result<bool> {
        let deleted = self.inner.store.delete_emulator(id).await?;
        if deleted {
            self.changed("emulators");
            self.changed("platforms");
        }
        Ok(deleted)
    }

    /// The command line of a profile being edited.
    pub fn preview(&self, draft: &ProfileDraft) -> ArgvPreview {
        let catalog = self.catalog();
        match self.templates(&catalog, draft.definition.as_deref(), &draft.profile) {
            Ok(templates) => self.resolve(&draft.install, &templates).0,
            Err(reason) => ArgvPreview::unresolved(&draft.install, reason),
        }
    }

    fn view(
        &self,
        catalog: &Catalog,
        emulator: Emulator,
        overrides: &BTreeMap<String, PathBuf>,
    ) -> EmulatorView {
        let present = installed(&emulator.install, &self.inner.folders, self.search_path());
        let profiles: Vec<ProfileView> = emulator
            .profiles
            .iter()
            .map(
                |profile| match self.templates(catalog, emulator.definition.as_deref(), profile) {
                    Ok(templates) => {
                        let (preview, missing) = self.resolve(&emulator.install, &templates);
                        let bios = self.check_bios(&emulator.install, &templates, overrides);
                        let availability = if !present {
                            Availability::NotInstalled
                        } else if let Some(error) = &preview.error {
                            Availability::Invalid {
                                reason: error.clone(),
                            }
                        } else if missing.is_empty() {
                            Availability::Available
                        } else {
                            Availability::MissingFiles { missing }
                        };
                        ProfileView {
                            id: profile.id().to_owned(),
                            name: templates.name,
                            platforms: templates.platforms,
                            extensions: templates.extensions,
                            availability,
                            bios,
                            preview,
                            drift: templates.drift,
                        }
                    }
                    Err(reason) => ProfileView {
                        id: profile.id().to_owned(),
                        name: match profile {
                            EmulatorProfile::BuiltIn {
                                definition_profile, ..
                            } => definition_profile.clone(),
                            EmulatorProfile::Custom { name, .. } => name.clone(),
                        },
                        platforms: Vec::new(),
                        extensions: Vec::new(),
                        availability: Availability::Invalid {
                            reason: reason.clone(),
                        },
                        bios: Vec::new(),
                        preview: ArgvPreview::unresolved(&emulator.install, reason),
                        drift: None,
                    },
                },
            )
            .collect();
        let status = if !present {
            EmulatorStatus::NotFound
        } else if profiles
            .iter()
            .all(|profile| profile.availability == Availability::Available)
        {
            EmulatorStatus::Ready
        } else {
            EmulatorStatus::SomeMissing
        };
        let mut addable: Vec<AddableProfile> = emulator
            .definition
            .as_deref()
            .and_then(|id| catalog.emulator(id))
            .map(|definition| {
                definition
                    .profiles
                    .iter()
                    .filter(|candidate| {
                        !emulator.profiles.iter().any(|profile| {
                            matches!(profile, EmulatorProfile::BuiltIn { definition_profile, .. }
                                if *definition_profile == candidate.name)
                        })
                    })
                    .map(|candidate| {
                        let context = self.context(&emulator.install, &candidate.platforms, &[]);
                        let available = present
                            && definition
                                .resolve(
                                    &candidate.name,
                                    self.inner.os,
                                    emulator.install.kind(),
                                    &context,
                                )
                                .is_ok_and(|resolved| resolved.available());
                        AddableProfile {
                            name: candidate.name.clone(),
                            platforms: candidate.platforms.clone(),
                            available,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        // Stable: definition order within each group.
        addable.sort_by_key(|profile| !profile.available);
        EmulatorView {
            emulator,
            status,
            profiles,
            addable,
        }
    }

    fn templates<'a>(
        &self,
        catalog: &'a Catalog,
        definition: Option<&str>,
        profile: &EmulatorProfile,
    ) -> std::result::Result<Templates<'a>, String> {
        match profile {
            EmulatorProfile::BuiltIn {
                definition_profile,
                args,
                ..
            } => {
                let definition_id = definition
                    .ok_or_else(|| crate::tr!("a built-in profile needs an emulator definition"))?;
                let definition = catalog
                    .emulator(definition_id)
                    .ok_or_else(|| crate::tr!("Caskade doesn't know this emulator."))?;
                let base = definition.profile(definition_profile).ok_or_else(|| {
                    crate::tr!(
                        "{emulator} no longer has this profile.",
                        emulator = definition.name
                    )
                })?;
                let drift = match args {
                    ArgsOverride::Replace { replaced, .. }
                        if !replaced.is_empty() && *replaced != base.args =>
                    {
                        Some(ArgsDrift {
                            overridden_from: replaced.clone(),
                            definition_args: base.args.clone(),
                        })
                    }
                    _ => None,
                };
                Ok(Templates {
                    name: base.name.clone(),
                    platforms: base.platforms.clone(),
                    extensions: base.extensions.clone(),
                    vars: &definition.vars,
                    args: args.apply(&base.args),
                    working_dir: base.working_dir.clone(),
                    requires: base.requires.clone(),
                    executable: None,
                    bios: self.bios(catalog, &base.platforms, &base.bios),
                    drift,
                })
            }
            EmulatorProfile::Custom {
                name,
                executable,
                args,
                working_dir,
                platforms,
                extensions,
                ..
            } => Ok(Templates {
                name: name.clone(),
                platforms: platforms.clone(),
                extensions: extensions.clone(),
                vars: Vars::NONE,
                args: args.clone(),
                working_dir: working_dir.clone(),
                requires: Vec::new(),
                executable: executable.clone(),
                bios: self.bios(catalog, platforms, &[]),
                drift: None,
            }),
        }
    }

    /// The BIOS files of `platforms` and a profile's extras, once each.
    pub(super) fn bios(
        &self,
        catalog: &Catalog,
        platforms: &[String],
        extra: &[Bios],
    ) -> Vec<(Option<String>, Bios)> {
        let mut entries: Vec<(Option<String>, Bios)> = Vec::new();
        let all = platforms
            .iter()
            .filter_map(|id| catalog.platform(id))
            .flat_map(|platform| {
                platform
                    .bios
                    .iter()
                    .map(|bios| (Some(platform.id.clone()), bios.clone()))
            })
            .chain(extra.iter().map(|bios| (None, bios.clone())));
        for (platform, bios) in all {
            if !entries.iter().any(|(_, entry)| entry.file == bios.file) {
                entries.push((platform, bios));
            }
        }
        entries
    }

    /// Checks a profile's BIOS files where its emulator looks for them; a
    /// platform's BIOS folder override wins for that platform's files.
    fn check_bios(
        &self,
        install: &EmulatorInstall,
        templates: &Templates<'_>,
        overrides: &BTreeMap<String, PathBuf>,
    ) -> Vec<BiosEntry> {
        let context = VarContext {
            emulator_dir: emulator_dir(install, &self.inner.folders, self.search_path()),
            folders: self.inner.folders.clone(),
            caskade_dir: self.inner.caskade_dir.clone(),
            ..VarContext::default()
        };
        templates
            .bios
            .iter()
            .map(|(platform, bios)| {
                let dirs = bios_dirs(
                    platform
                        .as_ref()
                        .and_then(|platform| overrides.get(platform))
                        .map(PathBuf::as_path),
                    templates.vars,
                    self.inner.os,
                    install.kind(),
                    &context,
                );
                let (state, path) = self.inner.hashes.check(bios, &dirs);
                BiosEntry {
                    found: Some(state == BiosState::Found),
                    state: Some(state),
                    path: path.map(|path| path.to_string_lossy().into_owned()),
                    platform: platform.clone(),
                    ..bios_entry(bios)
                }
            })
            .collect()
    }

    /// Platform BIOS folder overrides.
    async fn bios_overrides(&self) -> Result<BTreeMap<String, PathBuf>> {
        Ok(self
            .inner
            .store
            .platform_settings()
            .await?
            .into_iter()
            .filter_map(|settings| Some((settings.platform, settings.bios_dir?)))
            .collect())
    }

    /// A context with a sample ROM of the profile's first platform.
    fn context(
        &self,
        install: &EmulatorInstall,
        platforms: &[String],
        extensions: &[String],
    ) -> VarContext {
        let catalog = self.catalog();
        let platform = platforms.first().and_then(|id| catalog.platform(id));
        let extension = extensions
            .iter()
            .chain(
                platform
                    .map(|platform| &platform.extensions)
                    .into_iter()
                    .flatten(),
            )
            .find(|extension| *extension != "<none>");
        let file = match extension {
            Some(extension) => format!("Sample Game (USA).{extension}"),
            None => "Sample Game (USA)".to_owned(),
        };
        let folder = platform.map_or("ROMs".to_owned(), |platform| platform.short.clone());
        let home = self.inner.folders.user_home.clone().unwrap_or_default();
        VarContext {
            rom: Some(home.join("ROMs").join(folder).join(file)),
            emulator_dir: emulator_dir(install, &self.inner.folders, self.search_path()),
            name: Some("Sample Game".into()),
            platform: platforms.first().cloned(),
            game_id: Some("sample".into()),
            caskade_dir: self.inner.caskade_dir.clone(),
            storage_root: None,
            folders: self.inner.folders.clone(),
        }
    }

    /// The preview and the required files that don't exist.
    fn resolve(
        &self,
        install: &EmulatorInstall,
        templates: &Templates<'_>,
    ) -> (ArgvPreview, Vec<String>) {
        let context = self.context(install, &templates.platforms, &templates.extensions);
        let sample_rom = context
            .rom
            .as_deref()
            .map(|rom| rom.to_string_lossy().into_owned())
            .unwrap_or_default();
        let command = command(install);
        let program = match &templates.executable {
            Some(executable) => executable.to_string_lossy().into_owned(),
            None => command.program,
        };
        match resolve(
            templates.vars,
            &templates.args,
            templates.working_dir.as_deref(),
            &templates.requires,
            self.inner.os,
            install.kind(),
            &context,
        ) {
            Ok(resolved) => {
                let missing = resolved
                    .requires
                    .iter()
                    .filter(|path| !path.exists())
                    .map(|path| path.to_string_lossy().into_owned())
                    .collect();
                let mut args = if templates.executable.is_some() {
                    Vec::new()
                } else {
                    command.prefix
                };
                args.extend(resolved.args);
                (
                    ArgvPreview {
                        sample_rom,
                        program,
                        args,
                        cwd: resolved.working_dir,
                        error: None,
                    },
                    missing,
                )
            }
            Err(error) => (
                ArgvPreview {
                    sample_rom,
                    program,
                    args: Vec::new(),
                    cwd: None,
                    error: Some(error),
                },
                Vec::new(),
            ),
        }
    }
}

impl ArgvPreview {
    /// The install's program alone, with why the profile doesn't resolve.
    fn unresolved(install: &EmulatorInstall, reason: String) -> Self {
        Self {
            sample_rom: String::new(),
            program: command(install).program,
            args: Vec::new(),
            cwd: None,
            error: Some(reason),
        }
    }
}

pub(super) fn bios_entry(bios: &Bios) -> BiosEntry {
    BiosEntry {
        file: bios.file.clone(),
        required: bios.required,
        found: None,
        state: None,
        path: None,
        platform: None,
    }
}
