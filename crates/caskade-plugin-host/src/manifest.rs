//! `plugin.json`: what a plugin is, what it needs and what
//! it contributes. Contributions are declared so Caskade can list them, ask
//! for permissions and activate lazily without running the plugin's code.

use caskade_installer::fetch::grant::entry_parts;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};

/// The `caskade:plugin` API this host implements. 0.1.1 added
/// `host.locale` (#219); 0.2.0 added the `ui` export and `host.message`
/// and `host.plural` (#109), and plugins' UI moved from JavaScript halves
/// to it. 0.1 plugins still load while they're deprecated: their backends
/// link to the 0.1 world, and their JavaScript halves run in the webview
/// only.
pub const HOST_API: semver::Version = semver::Version::new(0, 2, 0);

/// [`HOST_API`] as the WIT package writes it.
pub const HOST_API_TEXT: &str = "0.2.0";

/// The last API of the previous minor, still accepted.
pub const PREVIOUS_API: semver::Version = semver::Version::new(0, 1, 1);

/// The manifest file in a plugin folder.
pub const MANIFEST: &str = "plugin.json";

/// The largest manifest read.
const MANIFEST_MAX: u64 = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Manifest {
    /// Editor hint; ignored.
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    #[specta(skip)]
    pub schema: Option<String>,
    /// Reverse-DNS, e.g. `dev.example.howlongtobeat`.
    pub id: String,
    pub name: String,
    #[schemars(with = "String")]
    #[specta(type = String)]
    pub version: semver::Version,
    /// The `caskade:plugin` API range the plugin targets, e.g. `^0.1`.
    #[schemars(with = "String")]
    #[specta(type = String)]
    pub api: semver::VersionReq,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    /// The UI half: an ES module inside the plugin folder.
    #[serde(default)]
    pub ui: Option<PathBuf>,
    /// The backend half: a WebAssembly component inside the plugin folder.
    #[serde(default)]
    pub backend: Option<PathBuf>,
    #[serde(default)]
    pub permissions: Permissions,
    /// When to activate: `onStartup`, `onView:<id>`, `onEvent:<name>`,
    /// `onCommand:<id>`, `onRequest`.
    #[serde(default)]
    pub activation: Vec<String>,
    #[serde(default)]
    pub contributes: Contributes,
}

/// What the plugin asks for; granted by the user (#124 enforces).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Permissions {
    /// Hosts the backend may reach, e.g. `howlongtobeat.com`.
    #[serde(default)]
    pub network: Vec<String>,
    /// Folders the backend may open, e.g. `$HOME/ROMs:ro`.
    #[serde(default)]
    pub fs: Vec<String>,
    /// May spawn processes and rewrite launch commands.
    #[serde(default)]
    pub process: bool,
    #[serde(default)]
    pub library: LibraryAccess,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub enum LibraryAccess {
    #[default]
    None,
    Read,
    Write,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Contributes {
    /// UI contribution points, e.g. `game.details.tabs`.
    #[serde(default)]
    pub slots: Vec<Slot>,
    /// Full pages with sidebar entries.
    #[serde(default)]
    pub views: Vec<View>,
    /// Events and interceptor points the backend handles.
    #[serde(default)]
    pub hooks: Vec<String>,
    /// Providers the backend implements, e.g. `library-source`.
    #[serde(default)]
    pub providers: Vec<String>,
    #[serde(default)]
    pub commands: Vec<Command>,
    /// A JSON Schema for the plugin's settings (#128).
    #[serde(default)]
    #[specta(type = Option<serde_json::Value>)]
    pub settings: Option<serde_json::Value>,
    /// Data contributions such as emulator definitions.
    #[serde(default)]
    pub emulation: Vec<PathBuf>,
    /// Theme folders the plugin bundles, each with a `theme.json` (#139).
    #[serde(default)]
    pub themes: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Slot {
    pub slot: String,
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    /// `desktop`, `console` or both. Empty means the slot's default: the
    /// game page and Home show in console mode too, other slots only on the
    /// desktop (#152).
    #[serde(default)]
    pub modes: Vec<String>,
}

/// The modes a view or slot can declare.
pub const MODES: &[&str] = &["desktop", "console"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct View {
    pub id: String,
    pub title: String,
    /// A lucide icon name for the sidebar entry.
    #[serde(default)]
    pub icon: Option<String>,
    /// `desktop`, `console` or both (default).
    #[serde(default)]
    pub modes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Command {
    pub id: String,
    pub title: String,
}

/// A manifest problem, with the key it concerns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct ManifestError {
    /// JSON path of the offending key, e.g. `permissions.library`; empty
    /// for the file as a whole.
    pub key: String,
    pub message: String,
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.key.is_empty() {
            f.write_str(&self.message)
        } else {
            write!(f, "{}: {}", self.key, self.message)
        }
    }
}

fn error(key: &str, message: impl Into<String>) -> ManifestError {
    ManifestError {
        key: key.into(),
        message: message.into(),
    }
}

/// Event and interceptor names a backend may declare in `contributes.hooks`.
pub const HOOK_POINTS: [&str; 26] = [
    "app.ready",
    "library.scan.completed",
    "library.game.added",
    "library.game.updated",
    "library.game.removed",
    "game.launching",
    "game.started",
    "game.exited",
    "settings.changed",
    "emulation.changed",
    "emulation.scan.completed",
    // Installer events (#55); `install.progress` is for the UI only.
    "install.queued",
    "install.downloaded",
    "install.completed",
    "install.failed",
    "uninstall.completed",
    "game.moved",
    "input.devices.changed",
    // A game's achievements were fetched (#185): only its id. The unlock
    // announcements carry account and achievement names, and stay out.
    "achievements.changed",
    "launch.before",
    "launch.after",
    "library.import",
    "metadata.merge",
    "install.plan",
    "install.after",
    "install.moved",
];

/// UI contribution points; the shell renders them (#127).
pub const SLOTS: [&str; 12] = [
    "views",
    "home.sections",
    "game.details.tabs",
    "game.details.panels",
    "game.card.badges",
    "game.card.overlay",
    "game.actions",
    "topbar.items",
    "sidebar.items",
    "settings.pages",
    "commands",
    "notifications",
];

/// Provider contracts a backend may implement (#126).
pub const PROVIDERS: [&str; 5] = [
    "library-source",
    "metadata-provider",
    "runner",
    "install-handler",
    "url-resolver",
];

fn valid_id(id: &str) -> bool {
    let parts: Vec<&str> = id.split('.').collect();
    id.len() <= 128
        && parts.len() >= 2
        && parts.iter().all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                && !part.starts_with('-')
                && !part.ends_with('-')
        })
}

/// Whether `api` accepts this host: the current or the previous 0.x minor.
pub fn api_supported(api: &semver::VersionReq) -> bool {
    api.matches(&HOST_API) || api.matches(&PREVIOUS_API)
}

/// Whether a plugin targets 0.2 or later: its UI is drawn from its
/// backend's `ui` export, and it may not name a JavaScript `ui` module.
pub fn draws_ui(api: &semver::VersionReq) -> bool {
    api.matches(&HOST_API)
}

/// A path inside the plugin folder: relative, no `..`, and (when the folder
/// is given) not escaping it through symlinks.
fn inside(dir: Option<&Path>, path: &Path, key: &str) -> Result<(), ManifestError> {
    let text = path.to_string_lossy();
    if text.is_empty()
        || path.is_absolute()
        || text.starts_with(['/', '\\'])
        || text.contains(':')
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_) | Component::CurDir))
    {
        return Err(error(key, "must be a path inside the plugin folder"));
    }
    if let Some(dir) = dir {
        let root = dir
            .canonicalize()
            .map_err(|e| error(key, format!("plugin folder: {e}")))?;
        let file = root
            .join(path)
            .canonicalize()
            // Missing is the common case; say so, and keep any other cause
            // (no access, a link loop) instead of calling it missing (#160).
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => {
                    error(key, format!("{} does not exist", path.display()))
                }
                _ => error(key, format!("{} can't be read: {e}", path.display())),
            })?;
        if !file.starts_with(&root) {
            return Err(error(key, "leaves the plugin folder"));
        }
        if !file.is_file() {
            return Err(error(key, "is not a file"));
        }
    }
    Ok(())
}

impl Manifest {
    /// Parses `plugin.json` text; unknown keys and bad values fail with the
    /// key named.
    pub fn parse(text: &str) -> Result<Self, Vec<ManifestError>> {
        let mut deserializer = serde_json::Deserializer::from_str(text);
        serde_path_to_error::deserialize(&mut deserializer).map_err(|failure| {
            let key = failure.path().to_string();
            vec![error(
                if key == "." { "" } else { &key },
                failure.into_inner().to_string(),
            )]
        })
    }

    /// Reads and validates `<dir>/plugin.json`, checking the halves exist
    /// inside the folder.
    pub fn read(dir: &Path) -> Result<Self, Vec<ManifestError>> {
        let path = dir.join(MANIFEST);
        let size = std::fs::metadata(&path)
            .map_err(|e| vec![error("", format!("{MANIFEST}: {e}"))])?
            .len();
        if size > MANIFEST_MAX {
            return Err(vec![error(
                "",
                format!("{MANIFEST} is larger than 256 KiB"),
            )]);
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| vec![error("", format!("{MANIFEST}: {e}"))])?;
        let manifest = Self::parse(&text)?;
        manifest.validate(Some(dir))?;
        Ok(manifest)
    }

    /// Checks values serde can't: ids, the API range, paths and names.
    pub fn validate(&self, dir: Option<&Path>) -> Result<(), Vec<ManifestError>> {
        let mut errors = Vec::new();
        if !valid_id(&self.id) {
            errors.push(error(
                "id",
                "must be reverse-DNS: lowercase dot-separated parts, e.g. dev.example.hello",
            ));
        }
        if self.name.trim().is_empty() || self.name.len() > 128 {
            errors.push(error("name", "must be 1–128 characters"));
        }
        if !api_supported(&self.api) {
            errors.push(error(
                "api",
                format!(
                    "{} does not accept this Caskade's plugin API {HOST_API}",
                    self.api
                ),
            ));
        }
        if self.ui.is_some() && draws_ui(&self.api) {
            errors.push(error(
                "ui",
                "from plugin API 0.2 a plugin's UI is drawn by its backend's `ui` export; \
                 JavaScript UI modules are for 0.1 plugins",
            ));
        }
        let drawn = draws_ui(&self.api)
            && self.backend.is_none()
            && (!self.contributes.slots.is_empty() || !self.contributes.views.is_empty());
        if drawn {
            errors.push(error(
                "backend",
                "slots and views are drawn by the backend from plugin API 0.2",
            ));
        }
        if self.ui.is_none()
            && self.backend.is_none()
            && self.contributes.emulation.is_empty()
            && self.contributes.themes.is_empty()
        {
            errors.push(error(
                "",
                "a plugin needs `ui`, `backend` or data contributions",
            ));
        }
        for (key, path) in [("ui", &self.ui), ("backend", &self.backend)] {
            if let Some(path) = path
                && let Err(problem) = inside(dir, path, key)
            {
                errors.push(problem);
            }
        }
        for (index, path) in self.contributes.emulation.iter().enumerate() {
            if let Err(problem) = inside(dir, path, &format!("contributes.emulation[{index}]")) {
                errors.push(problem);
            }
        }
        for (index, path) in self.contributes.themes.iter().enumerate() {
            let key = format!("contributes.themes[{index}]");
            if let Err(problem) = inside(dir, &path.join("theme.json"), &key) {
                errors.push(problem);
            }
        }
        for (index, entry) in self.permissions.network.iter().enumerate() {
            // An entry may name a port (`127.0.0.1:8384`); an IPv6 address
            // goes in brackets (`[::1]`, `[::1]:8080`).
            // `*` only works as a leading `*.` (capabilities::host_allowed).
            // `*.com` would allow a whole top-level domain, and `*.0.1`
            // would match IP addresses, so the domain needs a dot and a
            // top-level label with a letter (`*.localhost` is fine).
            let valid = match entry_parts(entry) {
                Some((host, _)) if entry.starts_with('[') => {
                    host.parse::<std::net::Ipv6Addr>().is_ok()
                }
                Some((host, _)) => {
                    let name = host.strip_prefix("*.");
                    let domain = name.unwrap_or(host);
                    !domain.is_empty()
                        && host.len() <= 253
                        && domain
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
                        && (name.is_none()
                            || domain == "localhost"
                            || (domain.contains('.') && has_letter_tld(domain)))
                }
                None => false,
            };
            if !valid {
                errors.push(error(
                    &format!("permissions.network[{index}]"),
                    "must be a host name such as example.com, or *.example.com for its subdomains, optionally followed by :port; IPv6 addresses go in brackets",
                ));
            }
        }
        for (index, folder) in self.permissions.fs.iter().enumerate() {
            if folder_mode(folder).is_none_or(|(path, _)| path.is_empty()) {
                errors.push(error(
                    &format!("permissions.fs[{index}]"),
                    "must be a folder, optionally followed by :ro or :rw",
                ));
            }
        }
        for (index, activation) in self.activation.iter().enumerate() {
            let valid = matches!(activation.as_str(), "onStartup" | "onRequest")
                || ["onView:", "onEvent:", "onCommand:"].iter().any(|prefix| {
                    activation
                        .strip_prefix(prefix)
                        .is_some_and(|rest| !rest.is_empty())
                });
            if !valid {
                errors.push(error(
                    &format!("activation[{index}]"),
                    "must be onStartup, onRequest, onView:<id>, onEvent:<name> or onCommand:<id>",
                ));
            }
        }
        let has_backend = self.backend.is_some();
        for (index, hook) in self.contributes.hooks.iter().enumerate() {
            let key = format!("contributes.hooks[{index}]");
            if !HOOK_POINTS.contains(&hook.as_str()) {
                errors.push(error(&key, format!("unknown hook point {hook:?}")));
            } else if !has_backend {
                errors.push(error(&key, "hooks need a backend"));
            }
        }
        for (index, provider) in self.contributes.providers.iter().enumerate() {
            let key = format!("contributes.providers[{index}]");
            if !PROVIDERS.contains(&provider.as_str()) {
                errors.push(error(&key, format!("unknown provider {provider:?}")));
            } else if !has_backend {
                errors.push(error(&key, "providers need a backend"));
            }
        }
        let mut seen = BTreeMap::new();
        for (index, slot) in self.contributes.slots.iter().enumerate() {
            if !SLOTS.contains(&slot.slot.as_str()) {
                errors.push(error(
                    &format!("contributes.slots[{index}].slot"),
                    format!("unknown contribution point {:?}", slot.slot),
                ));
            } else if slot.id.is_empty() {
                errors.push(error(
                    &format!("contributes.slots[{index}].id"),
                    "must not be empty",
                ));
            } else if seen
                .insert((slot.slot.clone(), slot.id.clone()), index)
                .is_some()
            {
                errors.push(error(
                    &format!("contributes.slots[{index}]"),
                    "is declared twice",
                ));
            }
        }
        let declared = self
            .contributes
            .slots
            .iter()
            .enumerate()
            .map(|(index, slot)| (format!("contributes.slots[{index}].modes"), &slot.modes))
            .chain(
                self.contributes
                    .views
                    .iter()
                    .enumerate()
                    .map(|(index, view)| {
                        (format!("contributes.views[{index}].modes"), &view.modes)
                    }),
            );
        for (key, modes) in declared {
            for mode in modes {
                if !MODES.contains(&mode.as_str()) {
                    errors.push(error(
                        &key,
                        format!("unknown mode {mode:?}; use \"desktop\" or \"console\""),
                    ));
                }
            }
        }
        if let Some(settings) = &self.contributes.settings
            && !settings.is_object()
        {
            errors.push(error(
                "contributes.settings",
                "must be a JSON Schema object",
            ));
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// The manifest's JSON Schema, committed for editors (`$schema`).
pub fn schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(Manifest)).expect("the schema serialises")
}

/// Whether a host name's last label has a letter, so it isn't part of an
/// IP address.
fn has_letter_tld(host: &str) -> bool {
    host.rsplit('.')
        .next()
        .is_some_and(|label| label.chars().any(|c| c.is_ascii_alphabetic()))
}

/// Splits an `fs` permission into its folder and whether it is read-only:
/// `$HOME/ROMs:ro`, `~/Saves:rw`, or a bare folder (read/write). The colon
/// of a Windows drive (`C:\Games`) isn't a mode: a suffix has no path
/// separator in it. `None` for a suffix other than `ro` or `rw`
/// (`$HOME/ROMs:rx`), which validation rejects and grants skip.
pub(crate) fn folder_mode(entry: &str) -> Option<(&str, bool)> {
    match entry.rsplit_once(':') {
        Some((path, "ro")) => Some((path, true)),
        Some((path, "rw")) => Some((path, false)),
        Some((_, suffix)) if !suffix.contains(['/', '\\']) => None,
        _ => Some((entry, false)),
    }
}

#[cfg(test)]
#[path = "manifest_tests.rs"]
mod tests;
