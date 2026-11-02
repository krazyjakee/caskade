use crate::{
    ludusavi::Entry,
    model::{GameEntry, GameRuntime},
};
use caskade_discovery::{HostEnv, OperatingSystem, env::KnownFolder};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// Explicit values also let Windows path logic run unchanged on a Linux test
/// host. Values are escaped as literal glob components, apart from the
/// intentional `<storeUserId>` wildcard when no account id is known.
#[derive(Debug, Clone)]
pub struct Context {
    pub os: OperatingSystem,
    pub host_os: OperatingSystem,
    pub store: Option<String>,
    pub values: BTreeMap<String, String>,
    pub case_sensitive: bool,
    pub warnings: Vec<String>,
    pub(crate) windows: Option<HostEnv>,
    pub(crate) prefix: Option<PathBuf>,
}

impl Context {
    pub fn new(os: OperatingSystem) -> Self {
        Self {
            os,
            host_os: os,
            store: None,
            values: BTreeMap::new(),
            case_sensitive: os == OperatingSystem::Linux,
            warnings: Vec::new(),
            windows: None,
            prefix: None,
        }
    }

    pub fn for_release(host: &HostEnv, entry: &GameEntry, base: &Path, root: &Path) -> Self {
        let os = if matches!(entry.runtime, GameRuntime::Windows { .. }) {
            OperatingSystem::Windows
        } else {
            host.os()
        };
        let mut context = Self::new(os);
        context.host_os = host.os();
        context.values.insert("base".into(), text(base));
        context.values.insert("root".into(), text(root));
        if let Some(name) = base.file_name() {
            context
                .values
                .insert("game".into(), name.to_string_lossy().into_owned());
        }
        context.values.insert("storeUserId".into(), "*".into());
        if host.os() != OperatingSystem::Windows {
            for (name, folder) in [
                ("xdgData", KnownFolder::DataHome),
                ("xdgConfig", KnownFolder::ConfigHome),
            ] {
                if let Some(path) = host.known_folder(folder) {
                    context.values.insert(name.into(), text(path));
                }
            }
        }
        let view = match &entry.runtime {
            GameRuntime::Windows {
                prefix: Some(prefix),
                ..
            } if host.os() != OperatingSystem::Windows => {
                let prefix = if prefix.path.is_absolute() {
                    prefix.path.clone()
                } else {
                    base.join(&prefix.path)
                };
                context.prefix = Some(prefix.clone());
                match host.wine_prefix(&prefix) {
                    Ok(view) => Some(view),
                    Err(error) => {
                        context.warnings.push(crate::tr!(
                            "Could not read the game's Wine or Proton prefix: {error}",
                            error = error
                        ));
                        None
                    }
                }
            }
            GameRuntime::Windows { .. } if host.os() != OperatingSystem::Windows => {
                context.warnings.push(crate::tr!(
                    "This release has no recorded Wine or Proton prefix."
                ));
                None
            }
            _ => Some(host.clone()),
        };
        if let Some(view) = &view {
            context.values.insert("home".into(), text(view.home()));
            if let Some(user) = view.var(if os == OperatingSystem::Windows {
                "USERNAME"
            } else {
                "USER"
            }) {
                context
                    .values
                    .insert("osUserName".into(), user.to_string_lossy().into_owned());
            }
            if os == OperatingSystem::Windows {
                for (name, folder) in [
                    ("winAppData", KnownFolder::AppData),
                    ("winLocalAppData", KnownFolder::LocalAppData),
                    ("winDocuments", KnownFolder::Documents),
                    ("winProgramData", KnownFolder::ProgramData),
                ] {
                    if let Some(path) = view.known_folder(folder) {
                        let registry_name = match name {
                            "winAppData" => Some("AppData"),
                            "winLocalAppData" => Some("Local AppData"),
                            "winDocuments" => Some("Personal"),
                            _ => None,
                        };
                        let path = registry_name
                            .and_then(|name| shell_folder(view, name))
                            .unwrap_or_else(|| path.to_owned());
                        context.values.insert(name.into(), text(&path));
                    }
                }
                context.values.insert(
                    "winLocalAppDataLow".into(),
                    text(
                        &shell_folder(view, "{A520A1A4-1780-4FF6-BD18-167343C5AF16}")
                            .unwrap_or_else(|| view.home().join("AppData/LocalLow")),
                    ),
                );
                for (name, variable, fallback) in [
                    ("winPublic", "PUBLIC", "C:/users/Public"),
                    ("winDir", "SystemRoot", "C:/Windows"),
                ] {
                    let value = view
                        .var(variable)
                        .map(PathBuf::from)
                        .unwrap_or_else(|| fallback.into());
                    if let Ok(path) = view.resolve_path(&value) {
                        context.values.insert(name.into(), text(&path));
                    }
                }
                context.windows = Some(view.clone());
            }
        }
        context
    }

    /// Constraints in a `when` list are alternatives. Fields within one
    /// constraint are conjunctive, and unknown OS/store values do not match.
    pub fn applies(&self, entry: &Entry) -> bool {
        entry.when.is_empty()
            || entry.when.iter().any(|constraint| {
                let os = match self.os {
                    OperatingSystem::Windows => "windows",
                    OperatingSystem::Linux => "linux",
                    OperatingSystem::Macos => "mac",
                };
                constraint
                    .os
                    .as_deref()
                    .is_none_or(|wanted| wanted == os || (wanted == "macos" && os == "mac"))
                    && constraint
                        .store
                        .as_deref()
                        .is_none_or(|wanted| self.store.as_deref() == Some(wanted))
            })
    }
}

/// Ludusavi's `<game>` is the install directory's name, `<base>` its full
/// path. A missing placeholder is an error, never an empty string or guess.
pub fn expand(template: &str, context: &Context) -> Result<Vec<String>, String> {
    let template = template
        .replace("*<storeUserId>", "<storeUserId>")
        .replace("<storeUserId>*", "<storeUserId>");
    let mut variants = vec![context.values.clone()];
    if context.prefix.is_some()
        && let Some(home) = context.values.get("home")
    {
        let mut legacy = context.values.clone();
        for (name, tail) in [
            ("winDocuments", "My Documents"),
            ("winAppData", "Application Data"),
            ("winLocalAppData", "Local Settings/Application Data"),
        ] {
            legacy.insert(name.into(), format!("{home}/{tail}"));
        }
        variants.push(legacy);
    }
    let mut result = Vec::new();
    for values in variants {
        let mut rest = template.as_str();
        let mut path = String::new();
        while let Some(open) = rest.find('<') {
            path.push_str(&rest[..open]);
            let close = rest[open..]
                .find('>')
                .map(|close| open + close)
                .ok_or_else(|| crate::tr!("The save path contains an unclosed placeholder."))?;
            let name = &rest[open + 1..close];
            let value = values.get(name).ok_or_else(|| {
                crate::tr!(
                    "Save placeholder <{name}> is unavailable for this release.",
                    name = name
                )
            })?;
            if name == "storeUserId" && value == "*" {
                path.push('*');
            } else {
                path.push_str(&globset::escape(&value.replace('\\', "/")));
            }
            rest = &rest[close + 1..];
        }
        path.push_str(rest);
        path = path.replace('\\', "/");
        if path.as_bytes().get(1) == Some(&b':') && context.host_os != OperatingSystem::Windows {
            let windows = context.windows.as_ref().ok_or_else(|| {
                crate::tr!(
                    "A Windows save path needs a recorded Wine or Proton prefix on this device."
                )
            })?;
            path = text(
                &windows
                    .map_windows_path(&path)
                    .map_err(|error| error.to_string())?,
            );
        }
        if !result.contains(&path) {
            result.push(path);
        }
    }
    Ok(result)
}

pub(crate) fn text(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn shell_folder(view: &HostEnv, name: &str) -> Option<PathBuf> {
    use caskade_discovery::registry::{RegistryHive, RegistryKey, RegistryValue, RegistryView};
    for path in [
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders",
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\Shell Folders",
    ] {
        let key = RegistryKey::new(RegistryHive::CurrentUser, RegistryView::Native, path);
        if let Some(RegistryValue::String(value) | RegistryValue::ExpandString(value)) =
            view.registry().value(&key, name).ok().flatten()
            && let Ok(path) = view.expand_path(&value)
        {
            return Some(path);
        }
    }
    None
}
