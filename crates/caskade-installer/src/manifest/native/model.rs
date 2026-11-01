use super::super::{ExternalLocation, Sha256Digest};
use crate::{Program, RegistryValue, RuntimeSpec, Step, plan::presets::InstallerFamily};
use serde::{Deserialize, Serialize};
use std::io;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeFamily {
    Inno,
    Nsis,
    Msi,
}
impl NativeFamily {
    pub(super) fn preset(self) -> InstallerFamily {
        match self {
            Self::Inno => InstallerFamily::Inno,
            Self::Nsis => InstallerFamily::Nsis,
            Self::Msi => InstallerFamily::Msi,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeUninstallPolicy {
    PreferNative,
    ManifestOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum RegistryEffect {
    /// Restore a pre-install value, or delete a value created by this install.
    Value {
        location: ExternalLocation,
        name: String,
        original: Option<RegistryValue>,
    },
    /// Remove a newly-created key only when it is empty after value reversal.
    Key { location: ExternalLocation },
    /// An owned Uninstall registration. The fingerprint permits cleanup after
    /// relocation without storing the installer's absolute path in our record.
    UninstallKey {
        location: ExternalLocation,
        fingerprint: Sha256Digest,
    },
}
impl RegistryEffect {
    pub fn location(&self) -> &ExternalLocation {
        match self {
            Self::Value { location, .. }
            | Self::Key { location }
            | Self::UninstallKey { location, .. } => location,
        }
    }
    pub fn validate(&self) -> io::Result<()> {
        registry_address(self.location())?;
        if let Self::UninstallKey { location, .. } = self
            && !uninstall_location(location)
        {
            return Err(io::Error::other("expected exact Uninstall registration"));
        }
        if let Self::Value { name, original, .. } = self {
            if name.len() > 16_384 || name.contains('\0') {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "invalid registry value name",
                ));
            }
            if serde_json::to_vec(original)
                .map_err(io::Error::other)?
                .len()
                > 1024 * 1024
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "registry original-value limit",
                ));
            }
        }
        Ok(())
    }
}
/// Scope IDs are portable; they never contain an absolute game directory.
/// Only HKCU/HKLM Software in an explicit 32/64-bit view may be recorded.
pub(super) fn registry_address(location: &ExternalLocation) -> io::Result<(bool, bool, String)> {
    let (machine, bits64) = match location.scope.as_str() {
        "registry.hkcu.32" => (false, false),
        "registry.hkcu.64" => (false, true),
        "registry.hklm.32" => (true, false),
        "registry.hklm.64" => (true, true),
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unknown registry scope",
            ));
        }
    };
    let key = location.path.as_str();
    if !key
        .split('/')
        .next()
        .is_some_and(|part| part.eq_ignore_ascii_case("Software"))
        || !key.contains('/')
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "registry effects must be below Software",
        ));
    }
    Ok((machine, bits64, key.replace('/', "\\")))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UninstallRegistration {
    pub location: ExternalLocation,
    pub install_location: String,
    pub uninstall_string: Option<String>,
    pub quiet_uninstall_string: Option<String>,
    pub windows_installer: bool,
}
/// Read and cleanup seam for native registry state. Implementations must reject
/// changed/foreign registrations and remove only the exact owned Uninstall key.
pub trait NativeRegistry: Send + Sync {
    fn available(&self) -> bool {
        true
    }
    fn uninstall_entries(&self) -> io::Result<Vec<UninstallRegistration>>;
    fn remove_uninstall_entry(&self, entry: &UninstallRegistration) -> io::Result<()>;
    fn undo_effect(&self, effect: &RegistryEffect) -> io::Result<()>;
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeUninstallReport {
    pub family: Option<NativeFamily>,
    pub program: Option<Program>,
    pub attempted: bool,
    pub succeeded: bool,
    pub manifest_fallback: bool,
    pub diagnostics: Vec<String>,
}
pub(crate) struct PreparedNative {
    pub steps: Vec<Step>,
    pub recognized: bool,
    pub registrations: Vec<UninstallRegistration>,
    pub report: NativeUninstallReport,
}
#[derive(Debug, Clone)]
pub struct NativeUninstaller {
    pub family: NativeFamily,
    pub program: Program,
    pub runtime: RuntimeSpec,
    pub product_code: Option<String>,
}
impl NativeUninstaller {
    pub fn step(&self) -> io::Result<Step> {
        let args = crate::plan::presets::build_uninstall_args(
            self.family.preset(),
            self.product_code.as_deref(),
        )
        .map_err(io::Error::other)?;
        Ok(Step::Run {
            program: self.program.clone(),
            args,
            env: Default::default(),
            runtime: self.runtime.clone(),
            timeout_secs: 120,
            expect: vec![],
        })
    }
}

pub(super) fn uninstall_location(location: &ExternalLocation) -> bool {
    registry_address(location).is_ok()
        && location
            .path
            .as_str()
            .to_ascii_lowercase()
            .strip_prefix("software/microsoft/windows/currentversion/uninstall/")
            .is_some_and(|key| !key.is_empty() && !key.contains('/'))
}
pub(super) fn registration_fingerprint(entry: &UninstallRegistration) -> Sha256Digest {
    use sha2::{Digest, Sha256};
    Sha256Digest::new(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(entry).expect("registry snapshot serializes"))
    ))
    .expect("SHA256 is valid")
}

impl UninstallRegistration {
    pub fn fingerprint(&self) -> Sha256Digest {
        registration_fingerprint(self)
    }
}
