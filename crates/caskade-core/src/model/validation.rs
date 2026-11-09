use super::*;
use crate::{Error, Result};
use std::path::{Path, PathBuf};

pub fn text(value: &str, name: &str, empty: bool, max: usize) -> Result<()> {
    if (!empty && value.trim().is_empty()) || value.contains('\0') || value.len() > max {
        return Err(Error::Invalid(crate::tr!("invalid {name}", name = name)));
    }
    Ok(())
}
pub fn id(value: &str) -> Result<()> {
    text(value, "identifier", false, 1024)
}
pub fn json<T: serde::Serialize>(value: &T) -> Result<String> {
    Ok(serde_json::to_string(value)?)
}
pub(crate) fn path_text(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| Error::Invalid(crate::tr!("path must be valid Unicode")))
}
pub(crate) fn relative(path: &Path, allow_root: bool) -> Result<PathBuf> {
    let value = path_text(path)?;
    if allow_root && value == "." {
        return Ok(PathBuf::from("."));
    }
    let normalized = caskade_installer::safety::sanitize_path(value)
        .map_err(|e| Error::Invalid(e.to_string()))?;
    Ok(PathBuf::from(path_text(&normalized)?.replace('\\', "/")))
}
pub(crate) fn absolute(path: &Path) -> bool {
    let Some(s) = path.to_str() else {
        return false;
    };
    path.is_absolute()
        || s.starts_with("\\\\")
        || s.starts_with("//")
        || (s.as_bytes().get(1) == Some(&b':')
            && s.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
            && s.as_bytes()
                .get(2)
                .is_some_and(|c| matches!(c, b'/' | b'\\')))
}
pub(crate) fn integer(value: u64) -> Result<i64> {
    i64::try_from(value)
        .map_err(|_| Error::Invalid(crate::tr!("integer exceeds SQLite signed range")))
}
impl Game {
    pub fn validate(&self) -> Result<()> {
        id(&self.id)?;
        GameField::ALL
            .into_iter()
            .try_for_each(|field| self.check(field))
    }
    /// Whether one field's value is allowed, with the reason it isn't.
    pub fn check(&self, field: GameField) -> Result<()> {
        let list = |name: &str, list: &[String]| {
            if list.len() > 256 {
                return Err(Error::Invalid(crate::tr!("too many {name}", name = name)));
            }
            list.iter()
                .try_for_each(|value| text(value, name, false, 4096))
        };
        match field {
            GameField::Title => text(&self.title, "game title", false, 4096),
            GameField::SortTitle => text(&self.sort_title, "sort title", false, 4096),
            GameField::Description => text(&self.description, "description", true, 1024 * 1024),
            GameField::Notes => text(&self.notes, "notes", true, 1024 * 1024),
            GameField::Rating if self.rating.is_some_and(|r| r > 100) => {
                Err(Error::Invalid(crate::tr!("rating must be 0 through 100")))
            }
            GameField::Rating => Ok(()),
            GameField::Aliases => list("aliases", &self.aliases),
            GameField::Genres => list("genres", &self.genres),
            GameField::Tags => list("tags", &self.tags),
            GameField::Platforms => list("platforms", &self.platforms),
            GameField::ReleaseDate => self.release_date.as_deref().map_or(Ok(()), validate_date),
        }
    }
}
pub fn validate_date(date: &str) -> Result<()> {
    let fail = || Error::Invalid(crate::tr!("release date must be YYYY-MM-DD"));
    let bytes = date.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes
            .iter()
            .enumerate()
            .any(|(i, c)| i != 4 && i != 7 && !c.is_ascii_digit())
    {
        return Err(fail());
    }
    let year = date[..4].parse::<u32>().map_err(|_| fail())?;
    let month = date[5..7].parse::<usize>().map_err(|_| fail())?;
    let day = date[8..].parse::<u32>().map_err(|_| fail())?;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if year == 0 || !(1..=12).contains(&month) || day == 0 || day > days[month - 1] {
        return Err(fail());
    }
    Ok(())
}
impl GameEntry {
    /// Normalize a clone before persistence; never silently strip an absolute
    /// path or a parent component from a managed installation.
    pub fn normalized(&self) -> Result<Self> {
        id(&self.id)?;
        id(&self.game_id)?;
        id(self.source_id.as_str())?;
        id(&self.external_id)?;
        if let Some(size) = self.size {
            integer(size)?;
        }
        let mut out = self.clone();
        if let Some(InstallLocation::Managed { root_id, rel_dir }) = &mut out.location {
            caskade_installer::manifest::RootManifest::new(root_id.clone())
                .map_err(|e| Error::Invalid(e.to_string()))?;
            *rel_dir = relative(rel_dir, false)?;
            if let Some(launch) = &mut out.launch {
                normalize_launch(launch, true)?;
            }
            if let GameRuntime::Windows { prefix, runner } = &mut out.runtime {
                if let Some(prefix) = prefix {
                    prefix.path = relative(&prefix.path, false)?;
                }
                if runner.as_ref().is_some_and(|r| r.path.is_some()) {
                    return Err(Error::Invalid(crate::tr!(
                        "managed runtime runner paths must be resolved at launch"
                    )));
                }
            }
            if let Some(tracking) = &mut out.tracking
                && let Some(dir) = &mut tracking.install_dir
            {
                *dir = relative(dir, true)?;
            }
        } else {
            if let Some(InstallLocation::External { path }) = &out.location {
                path_text(path)?;
                if !absolute(path) {
                    return Err(Error::Invalid(crate::tr!(
                        "external install location must be absolute"
                    )));
                }
            }
            if let Some(launch) = &mut out.launch {
                normalize_launch(launch, false)?;
            }
        }
        if let Some(alias) = &out.alias_id {
            caskade_installer::manifest::RootManifest::new(alias.clone())
                .map_err(|e| Error::Invalid(e.to_string()))?;
            if alias == "." || alias == ".." {
                return Err(Error::Invalid(crate::tr!("invalid alias identifier")));
            }
        }
        Ok(out)
    }
}
impl GameAction {
    /// Normalize a launch action (#177) before it is stored. A managed
    /// release's relative paths stay relative, so they move with the
    /// install; an absolute program (a tool outside it) stays absolute.
    pub fn normalized(&self, managed: bool) -> Result<Self> {
        id(&self.id)?;
        id(&self.entry_id)?;
        let mut out = self.clone();
        out.name = out.name.trim().to_owned();
        text(&out.name, "action name", false, 256)?;
        let managed_paths = managed
            && !matches!(&out.target, LaunchSpec::Executable { path, .. } if absolute(path));
        normalize_launch(&mut out.target, managed_paths)?;
        if let LaunchSpec::Executable { path, .. } = &out.target
            && path.as_os_str().is_empty()
        {
            return Err(Error::Invalid(crate::tr!("the action has no program")));
        }
        out.tracking = out.tracking.normalized(managed)?;
        out.proton = proton_build(out.proton.as_deref())?;
        Ok(out)
    }
}

/// A launch action's Proton build name, trimmed; empty is the automatic
/// choice (`None`).
pub(crate) fn proton_build(name: Option<&str>) -> Result<Option<String>> {
    let Some(name) = name.map(str::trim).filter(|name| !name.is_empty()) else {
        return Ok(None);
    };
    text(name, "Proton build", false, 256)?;
    if name.contains(['/', '\\']) || name == "." || name == ".." {
        return Err(Error::Invalid(crate::tr!(
            "a Proton build is named by its folder's name, without a path"
        )));
    }
    Ok(Some(name.to_owned()))
}
impl ActionTracking {
    pub fn normalized(&self, managed: bool) -> Result<Self> {
        let mut out = self.clone();
        out.names.retain(|name| !name.trim().is_empty());
        if out.names.len() > 64 {
            return Err(Error::Invalid(crate::tr!("too many process names")));
        }
        for name in &mut out.names {
            *name = name.trim().to_owned();
            text(name, "process name", false, 1024)?;
            if name.contains(['/', '\\']) {
                return Err(Error::Invalid(crate::tr!(
                    "a process name is a file name, without a folder"
                )));
            }
        }
        if let Some(folder) = &mut out.folder {
            if folder.as_os_str().is_empty() {
                out.folder = None;
            } else if absolute(folder) {
                path_text(folder)?;
            } else if managed {
                *folder = relative(folder, true)?;
            } else {
                return Err(Error::Invalid(crate::tr!(
                    "the tracking folder must be an absolute path"
                )));
            }
        }
        Ok(out)
    }
}
fn normalize_launch(spec: &mut LaunchSpec, managed: bool) -> Result<()> {
    match spec {
        LaunchSpec::Uri { uri } => text(uri, "launch URI", false, 16384)?,
        LaunchSpec::Executable {
            path,
            args,
            cwd,
            env,
        } => {
            if managed {
                *path = relative(path, false)?;
                if let Some(cwd) = cwd {
                    *cwd = relative(cwd, true)?;
                }
            } else {
                text(path_text(path)?, "executable path", false, 32768)?;
            }
            if args.len() > 4096 || env.len() > 4096 {
                return Err(Error::Invalid(crate::tr!(
                    "launch argument/environment limit"
                )));
            }
            for arg in args {
                text(arg, "launch argument", true, 32768)?;
            }
            for (key, value) in env {
                text(key, "environment key", false, 1024)?;
                text(value, "environment value", true, 32768)?;
                if key.contains('=') {
                    return Err(Error::Invalid(crate::tr!("environment key contains =")));
                }
            }
        }
        LaunchSpec::Flatpak { app_id, args } => {
            text(app_id, "Flatpak application ID", false, 1024)?;
            for arg in args {
                text(arg, "argument", true, 32768)?;
            }
        }
        LaunchSpec::Emulator {
            platform_id,
            target,
            roms,
            args,
        } => {
            use crate::emulation::{EmulatorTarget, RomLocation};
            id(platform_id)?;
            match target {
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
            for rom in roms {
                match &mut rom.location {
                    RomLocation::Scanner { scanner_id, path } => {
                        id(scanner_id)?;
                        *path = relative(path, false)?;
                    }
                    RomLocation::Absolute { path } => {
                        if !absolute(path) {
                            return Err(Error::Invalid(crate::tr!("ROM path must be absolute")));
                        }
                        path_text(path)?;
                    }
                }
                if let Some(name) = &rom.name {
                    text(name, "ROM name", false, 256)?;
                }
            }
            for arg in args.args() {
                text(arg, "argument", true, 32768)?;
            }
        }
        LaunchSpec::Plugin {
            plugin_id,
            runner_id,
            payload,
        } => {
            id(plugin_id)?;
            id(runner_id)?;
            if json(payload)?.len() > 1024 * 1024 {
                return Err(Error::Invalid(crate::tr!("plugin launch payload limit")));
            }
        }
        LaunchSpec::Web { url } => {
            text(url, "web game URL", false, 16384)?;
            *url = crate::web::page_url(url)?.into();
        }
        LaunchSpec::Stream { host, app } => {
            text(host, "streaming device", false, 256)?;
            text(app, "streamed game", false, 1024)?;
        }
    }
    Ok(())
}
impl StorageRoot {
    pub fn validate(&self) -> Result<()> {
        caskade_installer::manifest::RootManifest::new(self.id.clone())
            .map_err(|e| Error::Invalid(e.to_string()))?;
        path_text(&self.path)?;
        if !absolute(&self.path) {
            return Err(Error::Invalid(crate::tr!(
                "storage root path must be absolute"
            )));
        }
        if let Some(label) = &self.label {
            text(label, "storage label", true, 4096)?;
        }
        Ok(())
    }
}
impl Artwork {
    pub fn validate(&self) -> Result<()> {
        id(&self.id)?;
        id(&self.game_id)?;
        text(&self.source, "artwork source", false, 16384)?;
        if self.content_hash.len() != 64
            || !self.content_hash.bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err(Error::Invalid(crate::tr!(
                "artwork content hash must be SHA-256 hex"
            )));
        }
        relative(Path::new(&self.cached_path), false)?;
        if self.width == 0 || self.height == 0 || self.thumbnails.len() > 64 {
            return Err(Error::Invalid(crate::tr!(
                "invalid artwork dimensions/count"
            )));
        }
        for thumb in &self.thumbnails {
            relative(Path::new(&thumb.path), false)?;
            if thumb.size == 0 || thumb.width == 0 || thumb.height == 0 {
                return Err(Error::Invalid(crate::tr!("invalid thumbnail dimensions")));
            }
        }
        Ok(())
    }
}
impl PlaySession {
    pub fn validate(&self) -> Result<()> {
        id(&self.id)?;
        id(&self.game_id)?;
        id(&self.entry_id)?;
        integer(self.duration_ms)?;
        if let Some(tracking) = &self.tracking
            && json(tracking)?.len() > 1024 * 1024
        {
            return Err(Error::Invalid(crate::tr!("tracking payload limit")));
        }
        Ok(())
    }
}
impl ListGames {
    pub fn validate(&self) -> Result<()> {
        if self.limit > 1000 {
            return Err(Error::Invalid(crate::tr!(
                "library page limit exceeds 1000"
            )));
        }
        if let Some(query) = &self.query {
            text(query, "search query", true, 512)?;
        }
        if let Some(platform) = &self.platform {
            text(platform, "platform", false, 128)?;
        }
        Ok(())
    }
}
