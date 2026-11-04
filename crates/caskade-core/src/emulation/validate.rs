//! Save-time checks of the user's emulation configuration. With the
//! catalog, built-in profiles are checked against their definition too, so
//! an unknown variable fails when saved instead of at launch (§3.3).

use super::{
    Catalog, Emulator, EmulatorInstall, EmulatorProfile, EmulatorTarget, PlatformSettings,
    ProfileHooks, ProfileRef, RomScanner, Tracking, Vars, check_templates,
};
use crate::{
    Error, Result,
    model::validation::{absolute, id, path_text, text},
};
use std::path::Path;

fn absolute_path(path: &Path, name: &str) -> Result<()> {
    path_text(path)?;
    if !absolute(path) {
        return Err(Error::Invalid(crate::tr!(
            "{name} must be an absolute path",
            name = name
        )));
    }
    Ok(())
}

fn argv(args: &[String], name: &str) -> Result<()> {
    for arg in args {
        text(arg, name, true, 32768)?;
    }
    Ok(())
}

fn hooks(hooks: &ProfileHooks) -> Result<()> {
    for command in [&hooks.before, &hooks.after_exit].into_iter().flatten() {
        if command
            .first()
            .is_none_or(|program| program.trim().is_empty())
        {
            return Err(Error::Invalid(crate::tr!("a hook command needs a program")));
        }
        argv(command, &crate::trp!("field name", "hook argument"))?;
    }
    Ok(())
}

fn extensions(extensions: &[String]) -> Result<()> {
    for extension in extensions {
        let valid = extension == "<none>"
            || (!extension.is_empty()
                && extension.len() <= 32
                && extension
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
        if !valid {
            return Err(Error::Invalid(crate::tr!(
                "extension {extension} must be lowercase without the dot",
                extension = format!("{extension:?}")
            )));
        }
    }
    Ok(())
}

fn profile_ref(profile: &ProfileRef) -> Result<()> {
    id(&profile.emulator_id)?;
    id(&profile.profile_id)
}

impl Emulator {
    /// Checks the emulator; with `catalog`, built-in profiles must exist in
    /// its definition and use only variables it knows.
    pub fn validate(&self, catalog: Option<&Catalog>) -> Result<()> {
        id(&self.id)?;
        text(
            &self.name,
            &crate::trp!("field name", "emulator name"),
            false,
            256,
        )?;
        if let Some(definition) = &self.definition {
            id(definition)?;
        }
        match &self.install {
            EmulatorInstall::Folder { dir, executable } => {
                absolute_path(dir, &crate::trp!("field name", "emulator folder"))?;
                path_text(executable)?;
            }
            EmulatorInstall::Path { binary } => {
                text(
                    binary,
                    &crate::trp!("field name", "binary name"),
                    false,
                    256,
                )?;
                if binary.contains(['/', '\\']) {
                    return Err(Error::Invalid(crate::tr!(
                        "a $PATH binary is a name, not a path"
                    )));
                }
            }
            EmulatorInstall::Flatpak { app_id } => text(
                app_id,
                &crate::trp!("field name", "Flatpak app id"),
                false,
                255,
            )?,
            EmulatorInstall::AppImage { file } => {
                absolute_path(file, &crate::trp!("field name", "AppImage"))?
            }
            EmulatorInstall::AppBundle { bundle } => {
                absolute_path(bundle, &crate::trp!("field name", "app bundle"))?
            }
        }
        let definition = self
            .definition
            .as_deref()
            .zip(catalog)
            .map(|(id, catalog)| {
                catalog.emulator(id).ok_or_else(|| {
                    Error::Invalid(crate::tr!("Caskade doesn't know this emulator."))
                })
            })
            .transpose()?;
        let kind = self.install.kind();
        let mut seen = std::collections::BTreeSet::new();
        for profile in &self.profiles {
            id(profile.id())?;
            if !seen.insert(profile.id()) {
                return Err(Error::Invalid(crate::tr!(
                    "This emulator lists the same profile twice."
                )));
            }
            match profile {
                EmulatorProfile::BuiltIn {
                    definition_profile,
                    args,
                    hooks: profile_hooks,
                    ..
                } => {
                    text(
                        definition_profile,
                        &crate::trp!("field name", "definition profile"),
                        false,
                        256,
                    )?;
                    if self.definition.is_none() {
                        return Err(Error::Invalid(crate::tr!(
                            "a built-in profile needs an emulator definition"
                        )));
                    }
                    argv(args.args(), &crate::trp!("field name", "argument"))?;
                    match definition {
                        Some(definition) => {
                            if definition.profile(definition_profile).is_none() {
                                return Err(Error::Invalid(crate::tr!(
                                    "{emulator} no longer has this profile.",
                                    emulator = definition.name
                                )));
                            }
                            check_templates(
                                &definition.vars,
                                kind,
                                args.args().iter().map(String::as_str),
                            )?;
                        }
                        // Without the catalog only the syntax can be checked.
                        None => {
                            for arg in args.args() {
                                super::names(arg).map_err(Error::Invalid)?;
                            }
                        }
                    }
                    hooks(profile_hooks)?;
                }
                EmulatorProfile::Custom {
                    name,
                    executable,
                    args,
                    working_dir,
                    platforms,
                    extensions: custom_extensions,
                    env,
                    tracking,
                    hooks: profile_hooks,
                    ..
                } => {
                    text(name, &crate::trp!("field name", "profile name"), false, 256)?;
                    if let Some(executable) = executable {
                        path_text(executable)?;
                    }
                    argv(args, &crate::trp!("field name", "argument"))?;
                    let mut templates: Vec<&str> = args.iter().map(String::as_str).collect();
                    templates.extend(working_dir.as_deref());
                    if let Tracking::Folder { path } = tracking {
                        templates.push(path);
                    }
                    check_templates(Vars::NONE, kind, templates)?;
                    if let Tracking::ProcessName { name } = tracking {
                        text(name, &crate::trp!("field name", "process name"), false, 256)?;
                    }
                    for platform in platforms {
                        id(platform)?;
                    }
                    extensions(custom_extensions)?;
                    for (key, value) in env {
                        text(
                            key,
                            &crate::trp!("field name", "environment key"),
                            false,
                            1024,
                        )?;
                        if key.contains('=') {
                            return Err(Error::Invalid(crate::tr!(
                                "An environment variable's name can't contain =."
                            )));
                        }
                        text(
                            value,
                            &crate::trp!("field name", "environment value"),
                            true,
                            32768,
                        )?;
                    }
                    hooks(profile_hooks)?;
                }
            }
        }
        Ok(())
    }
}

impl PlatformSettings {
    pub fn validate(&self) -> Result<()> {
        id(&self.platform)?;
        if let Some(default) = &self.default {
            profile_ref(default)?;
        }
        if let Some(dir) = &self.bios_dir {
            absolute_path(dir, &crate::trp!("field name", "BIOS folder"))?;
        }
        Ok(())
    }
}

impl RomScanner {
    pub fn validate(&self) -> Result<()> {
        id(&self.id)?;
        text(
            &self.name,
            &crate::trp!("field name", "scanner name"),
            false,
            256,
        )?;
        absolute_path(&self.folder, &crate::trp!("field name", "scanner folder"))?;
        if let Some(platform) = &self.platform {
            id(platform)?;
        }
        profile_ref(&self.profile)?;
        match &self.launch_with {
            EmulatorTarget::PlatformDefault | EmulatorTarget::AskEmulator => {}
            EmulatorTarget::AskProfile { emulator_id } => id(emulator_id)?,
            EmulatorTarget::Profile {
                emulator_id,
                profile_id,
            } => {
                id(emulator_id)?;
                id(profile_id)?;
            }
        }
        for exclusion in &self.exclusions {
            text(
                exclusion,
                &crate::trp!("field name", "exclusion"),
                false,
                4096,
            )?;
        }
        Ok(())
    }
}
