//! Loading and validating definition files (EMULATION-PLAN §3). Files are
//! read in order and a later file replaces an earlier one's platform, region
//! or emulator with the same id, so plugins can fix shipped data.

use super::{
    Bios, DefinitionProfile, EmulatorDefinition, InstallKind, InstallSource, OSES, Platform,
    Region, SetupAction, SetupStep, Tracking, Vars, os_key,
    vars::{Expander, VarContext, builtin, leads_with_rom_name, names},
};
use caskade_installer::OperatingSystem;
use serde::Deserialize;
use std::{collections::BTreeMap, fmt, path::PathBuf};

/// A definition problem, named by file and key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionError {
    pub file: String,
    pub key: String,
    pub message: String,
}

impl fmt::Display for DefinitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.key.is_empty() {
            write!(f, "{}: {}", self.file, self.message)
        } else {
            write!(f, "{}: {}: {}", self.file, self.key, self.message)
        }
    }
}

impl std::error::Error for DefinitionError {}

/// Several definition problems at once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionErrors(pub Vec<DefinitionError>);

impl fmt::Display for DefinitionErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, error) in self.0.iter().enumerate() {
            if index > 0 {
                writeln!(f)?;
            }
            write!(f, "{error}")?;
        }
        Ok(())
    }
}

impl std::error::Error for DefinitionErrors {}

impl From<DefinitionErrors> for crate::Error {
    fn from(errors: DefinitionErrors) -> Self {
        crate::Error::Invalid(errors.to_string())
    }
}

/// A platform/region data file.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DataFile {
    #[serde(default)]
    platform: Vec<Platform>,
    #[serde(default)]
    region: Vec<Region>,
}

/// An item and the file that defined it.
#[derive(Debug, Clone, PartialEq)]
struct Sourced<T> {
    file: String,
    item: T,
}

impl<T> Sourced<T> {
    fn new(file: &str, item: T) -> Self {
        Self {
            file: file.to_owned(),
            item,
        }
    }
}

/// Every loaded platform, region and emulator definition.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Catalog {
    platforms: BTreeMap<String, Sourced<Platform>>,
    regions: BTreeMap<String, Sourced<Region>>,
    emulators: BTreeMap<String, Sourced<EmulatorDefinition>>,
}

impl Catalog {
    /// Reads `(file name, TOML)` pairs in order, then validates the result.
    pub fn load<'a>(
        files: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<Self, DefinitionErrors> {
        let mut catalog = Self::default();
        let mut errors = Vec::new();
        for (file, text) in files {
            if let Err(error) = catalog.add(file, text) {
                errors.push(error);
            }
        }
        if !errors.is_empty() {
            return Err(DefinitionErrors(errors));
        }
        catalog.validate()?;
        Ok(catalog)
    }

    /// Reads one file without validating the catalog. A file holds either
    /// `[[platform]]`/`[[region]]` tables or one emulator definition.
    pub fn add(&mut self, file: &str, text: &str) -> Result<(), DefinitionError> {
        let error = |key: &str, message: String| DefinitionError {
            file: file.to_owned(),
            key: key.to_owned(),
            message,
        };
        let sniff: toml::Table = toml::from_str(text).map_err(|e| error("", e.to_string()))?;
        if sniff.contains_key("id") {
            let definition: EmulatorDefinition =
                toml::from_str(text).map_err(|e| error("", e.to_string()))?;
            self.emulators
                .insert(definition.id.clone(), Sourced::new(file, definition));
            return Ok(());
        }
        let data: DataFile = toml::from_str(text).map_err(|e| error("", e.to_string()))?;
        let mut seen = std::collections::BTreeSet::new();
        let ids = data
            .platform
            .iter()
            .map(|platform| ("platform", &platform.id));
        let ids = ids.chain(data.region.iter().map(|region| ("region", &region.id)));
        for (kind, id) in ids {
            if !seen.insert((kind, id)) {
                return Err(error(
                    &format!("{kind} {id:?}"),
                    "defined twice in this file".into(),
                ));
            }
        }
        for item in data.platform {
            self.platforms
                .insert(item.id.clone(), Sourced::new(file, item));
        }
        for item in data.region {
            self.regions
                .insert(item.id.clone(), Sourced::new(file, item));
        }
        Ok(())
    }

    pub fn platform(&self, id: &str) -> Option<&Platform> {
        self.platforms.get(id).map(|sourced| &sourced.item)
    }

    pub fn platforms(&self) -> impl Iterator<Item = &Platform> {
        self.platforms.values().map(|sourced| &sourced.item)
    }

    pub fn region(&self, id: &str) -> Option<&Region> {
        self.regions.get(id).map(|sourced| &sourced.item)
    }

    pub fn regions(&self) -> impl Iterator<Item = &Region> {
        self.regions.values().map(|sourced| &sourced.item)
    }

    pub fn emulator(&self, id: &str) -> Option<&EmulatorDefinition> {
        self.emulators.get(id).map(|sourced| &sourced.item)
    }

    pub fn emulators(&self) -> impl Iterator<Item = &EmulatorDefinition> {
        self.emulators.values().map(|sourced| &sourced.item)
    }

    /// Checks everything that can be checked without a machine: ids,
    /// regexes, platform references, and that every variable resolves for
    /// every install kind on every OS.
    pub fn validate(&self) -> Result<(), DefinitionErrors> {
        let mut errors = Vec::new();
        for sourced in self.platforms.values() {
            let mut report = Report::new(&sourced.file, &mut errors);
            check_platform(&sourced.item, &mut report);
            self.check_recommended(&sourced.item, &mut report);
        }
        for sourced in self.regions.values() {
            let region = &sourced.item;
            let mut report = Report::new(&sourced.file, &mut errors);
            let key = format!("region {:?}", region.id);
            check_id(&region.id, &key, &mut report);
            check_name(&region.name, &format!("{key}.name"), &mut report);
            if region.codes.iter().any(|code| code.trim().is_empty()) {
                report.push(&format!("{key}.codes"), "codes must not be empty");
            }
        }
        for sourced in self.emulators.values() {
            self.check_emulator(&sourced.item, &mut Report::new(&sourced.file, &mut errors));
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(DefinitionErrors(errors))
        }
    }

    fn check_emulator(&self, definition: &EmulatorDefinition, report: &mut Report<'_>) {
        check_id(&definition.id, "id", report);
        check_name(&definition.name, "name", report);
        for os in OSES {
            let Some(rules) = definition.detect.on(os) else {
                continue;
            };
            let at = format!("detect.{}", os_key(os));
            for (field, pattern) in [
                ("executable", &rules.executable),
                ("appimage", &rules.appimage),
            ] {
                if let Some(pattern) = pattern
                    && let Err(error) = regex::Regex::new(pattern)
                {
                    report.push(&format!("{at}.{field}"), &format!("invalid regex: {error}"));
                }
            }
            for (field, value) in [("flatpak", &rules.flatpak), ("bundle", &rules.bundle)] {
                if value
                    .as_deref()
                    .is_some_and(|value| value.trim().is_empty())
                {
                    report.push(&format!("{at}.{field}"), "must not be empty");
                }
            }
            if rules
                .path
                .iter()
                .any(|name| name.trim().is_empty() || name.contains(['/', '\\']))
            {
                report.push(&format!("{at}.path"), "must be binary names, not paths");
            }
            // Locations are searched on this OS only, with the folder kind.
            for (index, location) in rules.locations.iter().enumerate() {
                let key = format!("{at}.locations[{index}]");
                check_template(
                    &definition.vars,
                    &[(os, InstallKind::Folder)],
                    location,
                    &key,
                    report,
                );
            }
        }
        let everywhere: Vec<_> = OSES
            .iter()
            .flat_map(|os| InstallKind::on(*os).iter().map(|kind| (*os, *kind)))
            .collect();
        for (key, template) in definition.vars.templates() {
            check_template(&definition.vars, &everywhere, template, &key, report);
        }
        for (field, patterns) in [
            ("files", &definition.saves.files),
            ("states", &definition.saves.states),
        ] {
            for (index, pattern) in patterns.iter().enumerate() {
                let key = format!("saves.{field}[{index}]");
                if pattern.trim().is_empty() || pattern.contains('\0') {
                    report.push(&key, "must be a nonempty path template");
                }
                check_template(&definition.vars, &everywhere, pattern, &key, report);
                let path = Some(PathBuf::from("/fixture"));
                let context = VarContext {
                    rom: Some(PathBuf::from("/fixture/Game.rom")),
                    emulator_dir: path.clone(),
                    name: Some("Game".into()),
                    platform: Some("platform".into()),
                    game_id: Some("game".into()),
                    caskade_dir: path.clone(),
                    storage_root: path.clone(),
                    folders: super::OsFolders {
                        user_home: path.clone(),
                        xdg_config: path.clone(),
                        app_data: path.clone(),
                        program_files: path.clone(),
                        app_support: path,
                    },
                };
                for (os, kind) in &everywhere {
                    let expander = Expander {
                        vars: &definition.vars,
                        os: *os,
                        kind: *kind,
                        context: &context,
                    };
                    if let Ok(expanded) = expander.expand_glob(pattern) {
                        let absolute = expanded.starts_with('/')
                            || (expanded.as_bytes().get(1) == Some(&b':')
                                && expanded.as_bytes().get(2) == Some(&b'/'));
                        if !absolute {
                            report.push(&key, "save templates must resolve to absolute paths");
                            break;
                        }
                        if let Err(error) = globset::GlobBuilder::new(&expanded)
                            .literal_separator(true)
                            .build()
                        {
                            report.push(&key, &format!("invalid save glob: {error}"));
                            break;
                        }
                    }
                }
            }
        }
        if let Some(tracking) = &definition.tracking {
            check_tracking(&definition.vars, &everywhere, tracking, "tracking", report);
        }
        for os in OSES {
            if let Some(source) = definition.install.on(os) {
                check_source(source, &format!("install.{}", os_key(os)), report);
            }
        }
        let display = &definition.display;
        for (field, flags) in [
            ("fullscreen", &display.fullscreen),
            ("windowed", &display.windowed),
        ] {
            for (index, flag) in flags.iter().enumerate() {
                let key = format!("display.{field}[{index}]");
                check_template(&definition.vars, &everywhere, flag, &key, report);
            }
        }
        for os in OSES {
            if let Some(template) = definition.requires_from.on(os) {
                let url = template
                    .replace("{file}", "core_libretro.so")
                    .replace("{arch}", "x86_64");
                if !url::Url::parse(&url).is_ok_and(|url| ["https", "http"].contains(&url.scheme()))
                {
                    report.push(
                        &format!("requires_from.{}", os_key(os)),
                        "must be an http(s) URL",
                    );
                }
            }
        }
        for (index, step) in definition.setup.iter().enumerate() {
            check_setup(definition, step, &format!("setup[{index}]"), report);
        }
        let mut seen = std::collections::BTreeSet::new();
        for profile in &definition.profiles {
            let at = format!("profile {:?}", profile.name);
            check_name(&profile.name, &at, report);
            if !seen.insert(&profile.name) {
                report.push(&at, "defined twice");
            }
            self.check_profile(definition, profile, &everywhere, &at, report);
        }
    }

    fn check_profile(
        &self,
        definition: &EmulatorDefinition,
        profile: &DefinitionProfile,
        everywhere: &[(OperatingSystem, InstallKind)],
        at: &str,
        report: &mut Report<'_>,
    ) {
        if profile.platforms.is_empty() {
            report.push(&format!("{at}.platforms"), "names no platform");
        }
        for platform in &profile.platforms {
            if self.platform(platform).is_none() {
                report.push(
                    &format!("{at}.platforms"),
                    &format!("unknown platform {platform:?}"),
                );
            }
        }
        check_extensions(&profile.extensions, &format!("{at}.extensions"), report);
        check_bios(&profile.bios, &format!("{at}.bios"), report);
        let templates = profile
            .args
            .iter()
            .enumerate()
            .map(|(index, arg)| (format!("{at}.args[{index}]"), arg))
            .chain(
                profile
                    .requires
                    .iter()
                    .enumerate()
                    .map(|(index, path)| (format!("{at}.requires[{index}]"), path)),
            )
            .chain(
                profile
                    .working_dir
                    .iter()
                    .map(|dir| (format!("{at}.working_dir"), dir)),
            );
        for (key, template) in templates {
            check_template(&definition.vars, everywhere, template, &key, report);
        }
        if let Some(tracking) = &profile.tracking {
            check_tracking(
                &definition.vars,
                everywhere,
                tracking,
                &format!("{at}.tracking"),
                report,
            );
        }
    }
}

impl Catalog {
    /// A platform's recommended emulators that this catalog has, best
    /// first, with the profile each names (or the first that runs the
    /// platform). Entries whose emulator or profile is gone, e.g. replaced
    /// by a plugin's definition, are skipped.
    pub fn recommended(&self, platform: &str) -> Vec<(&EmulatorDefinition, &DefinitionProfile)> {
        let Some(found) = self.platform(platform) else {
            return Vec::new();
        };
        found
            .recommended
            .iter()
            .filter_map(|entry| {
                let (emulator, profile) = match entry.split_once(':') {
                    Some((emulator, profile)) => (emulator, Some(profile)),
                    None => (entry.as_str(), None),
                };
                let definition = self.emulator(emulator)?;
                let runs = |candidate: &&DefinitionProfile| {
                    candidate.platforms.iter().any(|id| id == platform)
                };
                let profile = match profile {
                    Some(name) => definition.profile(name).filter(runs),
                    None => definition.profiles.iter().find(runs),
                }?;
                Some((definition, profile))
            })
            .collect()
    }

    /// `recommended` entries are `<emulator>` or `<emulator>:<profile>`.
    fn check_recommended(&self, platform: &Platform, report: &mut Report<'_>) {
        let key = format!("platform {:?}.recommended", platform.id);
        for entry in &platform.recommended {
            let emulator = entry.split_once(':').map_or(entry.as_str(), |(id, _)| id);
            let profile = entry.split_once(':').map(|(_, profile)| profile);
            check_id(emulator, &key, report);
            if profile.is_some_and(|profile| profile.trim().is_empty()) {
                report.push(&key, &format!("{entry:?} names an empty profile"));
            }
        }
    }
}

fn check_source(source: &InstallSource, key: &str, report: &mut Report<'_>) {
    let filled = |text: &str| !text.trim().is_empty();
    let check_url = |template: &str, report: &mut Report<'_>| {
        let url = template
            .replace("{version}", "1.0")
            .replace("{tag}", "v1.0");
        match url::Url::parse(&url) {
            Ok(url) if url.scheme() == "https" => {}
            _ => report.push(&format!("{key}.url"), "must be an https URL"),
        }
    };
    match source {
        InstallSource::Github {
            repo, asset, url, ..
        } => {
            let valid = repo.split('/').count() == 2
                && repo.split('/').all(|part| {
                    filled(part)
                        && part
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
                });
            if !valid {
                report.push(&format!("{key}.repo"), "must be `owner/repo`");
            }
            match (asset, url) {
                (Some(asset), None) => {
                    if let Err(error) = regex::Regex::new(asset) {
                        report.push(&format!("{key}.asset"), &format!("invalid regex: {error}"));
                    }
                }
                (None, Some(url)) => check_url(url, report),
                _ => report.push(key, "needs one of `asset` and `url`"),
            }
        }
        InstallSource::Dolphin { artifact } if !filled(artifact) => {
            report.push(&format!("{key}.artifact"), "must not be empty");
        }
        InstallSource::Dolphin { .. } => {}
        InstallSource::Url { url } => check_url(url, report),
        InstallSource::Flatpak { app } if !filled(app) => {
            report.push(&format!("{key}.app"), "must not be empty");
        }
        InstallSource::Flatpak { .. } => {}
    }
}

fn check_setup(
    definition: &EmulatorDefinition,
    step: &SetupStep,
    key: &str,
    report: &mut Report<'_>,
) {
    if let Some(os) = step
        .os
        .iter()
        .find(|os| !OSES.iter().any(|known| os_key(*known) == os.as_str()))
    {
        report.push(&format!("{key}.os"), &format!("unknown OS {os:?}"));
        return;
    }
    let kinds: Vec<_> = OSES
        .iter()
        .filter(|os| step.applies(**os))
        .flat_map(|os| InstallKind::on(*os).iter().map(|kind| (*os, *kind)))
        .collect();
    let templates: Vec<(&str, &str)> = match step.action() {
        Some(SetupAction::Ini {
            path,
            section,
            key,
            value,
        }) => {
            if key.trim().is_empty() || section.contains([']', '\n']) {
                report.push(key, "needs a section and a key");
            }
            vec![("ini", path), ("value", value)]
        }
        // `text` is literal: config files use braces.
        Some(SetupAction::File { path, .. }) => vec![("file", path)],
        None => {
            report.push(
                key,
                "must be `ini` + `section` + `key` + `value`, or `file` (+ `text`)",
            );
            return;
        }
    };
    for (field, template) in templates {
        check_template(
            &definition.vars,
            &kinds,
            template,
            &format!("{key}.{field}"),
            report,
        );
    }
}

/// Collects errors for one file.
struct Report<'a> {
    file: &'a str,
    errors: &'a mut Vec<DefinitionError>,
}

impl<'a> Report<'a> {
    fn new(file: &'a str, errors: &'a mut Vec<DefinitionError>) -> Self {
        Self { file, errors }
    }

    fn push(&mut self, key: &str, message: &str) {
        self.errors.push(DefinitionError {
            file: self.file.to_owned(),
            key: key.to_owned(),
            message: message.to_owned(),
        });
    }
}

fn check_id(id: &str, key: &str, report: &mut Report<'_>) {
    let valid = !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
    if !valid {
        report.push(
            key,
            &format!("id {id:?} must be lowercase letters, digits, `_` or `-`"),
        );
    }
}

fn check_name(name: &str, key: &str, report: &mut Report<'_>) {
    if name.trim().is_empty() {
        report.push(key, "name must not be empty");
    }
}

fn check_extensions(extensions: &[String], key: &str, report: &mut Report<'_>) {
    for extension in extensions {
        let valid = extension == "<none>"
            || (!extension.is_empty()
                && !extension.starts_with('.')
                && extension
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
        if !valid {
            report.push(
                key,
                &format!("extension {extension:?} must be lowercase without the dot"),
            );
        }
    }
}

fn check_bios(bios: &[Bios], key: &str, report: &mut Report<'_>) {
    for (index, file) in bios.iter().enumerate() {
        let at = format!("{key}[{index}]");
        // A file name, or a relative path in the BIOS folder such as
        // `dc/dc_boot.bin`, written with `/`.
        let safe = !file.file.trim().is_empty()
            && !file.file.contains(['\\', ':'])
            && file
                .file
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != "..");
        if !safe {
            report.push(
                &format!("{at}.file"),
                "must be a file name or a relative path with `/`",
            );
        }
        if let Some(md5) = &file.md5
            && !(md5.len() == 32 && md5.chars().all(|c| c.is_ascii_hexdigit()))
        {
            report.push(&format!("{at}.md5"), "must be 32 hex digits");
        }
    }
}

fn check_platform(platform: &Platform, report: &mut Report<'_>) {
    let key = format!("platform {:?}", platform.id);
    check_id(&platform.id, &key, report);
    check_name(&platform.name, &format!("{key}.name"), report);
    check_name(&platform.short, &format!("{key}.short"), report);
    check_extensions(&platform.extensions, &format!("{key}.extensions"), report);
    check_bios(&platform.bios, &format!("{key}.bios"), report);
    if let Some(dump) = &platform.ids.hasheous
        && !crate::hasheous::valid_dump(dump)
    {
        report.push(
            &format!("{key}.ids.hasheous"),
            "must be a dump name, without `.zip`",
        );
    }
}

fn check_tracking(
    vars: &Vars,
    everywhere: &[(OperatingSystem, InstallKind)],
    tracking: &Tracking,
    key: &str,
    report: &mut Report<'_>,
) {
    match tracking {
        Tracking::Folder { path } => check_template(vars, everywhere, path, key, report),
        Tracking::ProcessName { name } if name.trim().is_empty() => {
            report.push(key, "process name must not be empty");
        }
        _ => {}
    }
}

/// Every variable in `template` must resolve for every `(os, kind)`, and
/// definition variables must not refer to themselves.
fn check_template(
    vars: &Vars,
    targets: &[(OperatingSystem, InstallKind)],
    template: &str,
    key: &str,
    report: &mut Report<'_>,
) {
    if let Err(error) = names(template) {
        report.push(key, &error);
        return;
    }
    let context = VarContext::default();
    let mut failed = std::collections::BTreeSet::new();
    for &(os, kind) in targets {
        let expander = Expander {
            vars,
            os,
            kind,
            context: &context,
        };
        if let Err(error) = expander.expand(template) {
            let message = if error.starts_with("unknown variable") {
                format!("{error} on {} ({})", os_key(os), kind.key())
            } else {
                error
            };
            if failed.insert(message.clone()) {
                report.push(key, &message);
            }
        }
    }
}

/// The resolved argv tokens, working directory and required files of a
/// profile for one install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub args: Vec<String>,
    pub working_dir: Option<String>,
    pub requires: Vec<PathBuf>,
}

/// Expands argument templates and required paths. Built-in profiles pass
/// their definition's `vars`; custom profiles pass empty ones.
pub fn resolve(
    vars: &Vars,
    args: &[String],
    working_dir: Option<&str>,
    requires: &[String],
    os: OperatingSystem,
    kind: InstallKind,
    context: &VarContext,
) -> Result<Resolved, String> {
    let expander = Expander {
        vars,
        os,
        kind,
        context,
    };
    Ok(Resolved {
        args: args
            .iter()
            .map(|arg| {
                let value = expander.expand(arg)?;
                if value.starts_with('-') && leads_with_rom_name(vars, arg, os, kind) {
                    return Err(crate::tr!(
                        "the ROM file name {file} starts with `-`, which the emulator would read as an option; rename the file",
                        file = format!("{value:?}")
                    ));
                }
                Ok(value)
            })
            .collect::<Result<_, _>>()?,
        working_dir: working_dir.map(|dir| expander.expand(dir)).transpose()?,
        requires: requires
            .iter()
            .map(|path| expander.expand(path).map(PathBuf::from))
            .collect::<Result<_, _>>()?,
    })
}

impl EmulatorDefinition {
    /// The definition profile `name`, resolved for one install.
    pub fn resolve(
        &self,
        name: &str,
        os: OperatingSystem,
        kind: InstallKind,
        context: &VarContext,
    ) -> Result<Resolved, String> {
        let profile = self.profile(name).ok_or_else(|| {
            crate::tr!(
                "{emulator} no longer has this profile.",
                emulator = self.name
            )
        })?;
        resolve(
            &self.vars,
            &profile.args,
            profile.working_dir.as_deref(),
            &profile.requires,
            os,
            kind,
            context,
        )
    }
}

/// Whether a variable name is known to `vars` or built in, for an install of
/// `kind` on `os`. Used to list variables in the editor and to validate
/// custom profiles.
pub fn known(vars: &Vars, name: &str, os: OperatingSystem, kind: InstallKind) -> bool {
    builtin(name).is_some() || vars.get(name, os, kind).is_some()
}
