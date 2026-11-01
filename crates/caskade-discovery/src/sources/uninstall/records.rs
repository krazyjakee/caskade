use super::*;
use crate::registry::{RegistryEntry, RegistryKey, RegistryValue};
use crate::sources::REGISTRY_VIEWS;
use serde::Serialize;

pub const REGISTRY_PATH: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
/// The bounded metadata read from one visible Add/Remove Programs entry.
/// InstallDate is retained as YYYYMMDD; it is not a last-played timestamp.
#[derive(Debug, Clone, Serialize)]
pub struct UninstallRecord {
    pub key: String,
    pub name: String,
    pub display_name: String,
    pub install_location: Option<String>,
    pub display_icon: Option<String>,
    pub publisher: Option<String>,
    pub estimated_bytes: Option<u64>,
    pub install_date: Option<String>,
    pub uninstall_string: Option<String>,
    pub quiet_uninstall_string: Option<String>,
}

/// Inspect all four hive/view combinations through the injected registry.
/// Inaccessible keys warn and do not hide readable siblings; budget exhaustion
/// is an error rather than an apparently complete partial scan.
pub fn read_records(env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<UninstallRecord>, SourceError> {
    let mut records = vec![];
    let mut count = 0u64;
    let mut bytes = 0usize;
    for (hive, view) in REGISTRY_VIEWS {
        ctx.check()?;
        let root = RegistryKey::new(hive, view, REGISTRY_PATH);
        let mut children = match env.registry().subkeys(&root) {
            Ok(children) => children,
            Err(error) => {
                ctx.warn(format!("uninstall {hive:?}/{view:?}: {error}"));
                continue;
            }
        };
        count = count.saturating_add(children.len() as u64);
        if count > ctx.config().classifier.max_entries.min(100_000) {
            return Err(SourceError::Other {
                message: "uninstall registry key budget exceeded".into(),
            });
        }
        children.sort_by_key(|name| name.to_lowercase());
        for name in children {
            ctx.check()?;
            if name.len() > 32768 || name.contains(['\\', '/', '\0']) {
                continue;
            }
            let key = RegistryKey::new(hive, view, format!(r"{REGISTRY_PATH}\{name}"));
            let values = match env.registry().values(&key) {
                Ok(values) => values,
                Err(error) => {
                    ctx.warn(format!("{}: {error}", key.path));
                    continue;
                }
            };
            if values.len() > 128 {
                ctx.warn(format!("{}: too many uninstall values", key.path));
                continue;
            }
            let values: BTreeMap<_, _> = values
                .into_iter()
                .filter_map(|RegistryEntry { name, value }| {
                    if name.len() > 256 {
                        return None;
                    }
                    if let RegistryValue::String(s) | RegistryValue::ExpandString(s) = &value {
                        if s.len() > 32768 || s.contains('\0') {
                            return None;
                        }
                        bytes = bytes.saturating_add(s.len());
                    }
                    Some((name.to_ascii_lowercase(), value))
                })
                .collect();
            if bytes > 16 * 1024 * 1024 {
                return Err(SourceError::Other {
                    message: "uninstall metadata byte budget exceeded".into(),
                });
            }
            let text = |field: &str| match values.get(field) {
                Some(RegistryValue::String(s) | RegistryValue::ExpandString(s))
                    if !s.trim().is_empty() =>
                {
                    Some(s.trim().to_owned())
                }
                _ => None,
            };
            let hidden = matches!(values.get("systemcomponent"),Some(RegistryValue::Dword(v)) if *v != 0)
                || text("parentkeyname").is_some()
                || text("releasetype").is_some_and(|s| {
                    matches!(
                        s.to_ascii_lowercase().replace([' ', '-'], "").as_str(),
                        "update" | "hotfix" | "securityupdate" | "servicepack" | "updaterollup"
                    )
                });
            if hidden {
                continue;
            }
            let Some(display_name) = text("displayname") else {
                continue;
            };
            let estimated_bytes = match values.get("estimatedsize") {
                Some(RegistryValue::Dword(v)) => Some(u64::from(*v) * 1024),
                Some(RegistryValue::Qword(v)) => v.checked_mul(1024),
                _ => None,
            };
            let install_date = text("installdate").filter(|date| valid_date(date));
            records.push(UninstallRecord {
                key: format!("{hive:?}:{view:?}\\{}", key.path),
                name,
                display_name,
                install_location: text("installlocation"),
                display_icon: text("displayicon"),
                publisher: text("publisher"),
                estimated_bytes,
                install_date,
                uninstall_string: text("uninstallstring"),
                quiet_uninstall_string: text("quietuninstallstring"),
            });
        }
    }
    Ok(records)
}
fn valid_date(value: &str) -> bool {
    if value.len() != 8 || !value.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let year: u32 = value[..4].parse().unwrap_or(0);
    let month: u32 = value[4..6].parse().unwrap_or(0);
    let day: u32 = value[6..].parse().unwrap_or(0);
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let max = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => 0,
    };
    year > 0 && day > 0 && day <= max
}
