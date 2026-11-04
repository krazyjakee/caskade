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
        /// The data right after the install. When recorded, a value the user
        /// has changed since is preserved rather than restored or deleted.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        installed: Option<RegistryValue>,
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
    /// Whether Windows Installer still holds its own product record for this
    /// ProductCode (`Installer\Products\<packed code>`), which the manifest
    /// route never touches. Registries without one answer false.
    fn windows_installer_product(&self, product_code: &str) -> io::Result<bool> {
        let _ = product_code;
        Ok(false)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeUninstallReport {
    pub family: Option<NativeFamily>,
    pub program: Option<Program>,
    pub attempted: bool,
    pub succeeded: bool,
    pub manifest_fallback: bool,
    pub diagnostics: Vec<String>,
    /// After the manifest route removed an MSI game, `msiexec /x` dropped
    /// Windows Installer's own record of the product (#40).
    #[serde(default)]
    pub windows_installer_removed: bool,
}
pub(crate) struct PreparedNative {
    pub steps: Vec<Step>,
    pub recognized: bool,
    pub registrations: Vec<UninstallRegistration>,
    pub report: NativeUninstallReport,
    /// The ProductCode whose Windows Installer records `msiexec /x` drops
    /// once the manifest route has removed the game.
    pub forget_product: Option<String>,
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

/// Windows Installer's packed form of a ProductCode, as its registry keys
/// name products (`Installer\Products\<packed>`): the first three groups
/// reversed, then each byte of the last two with its hex digits swapped.
pub fn packed_product_code(code: &str) -> Option<String> {
    crate::plan::presets::build_uninstall_args(InstallerFamily::Msi, Some(code)).ok()?;
    let hex: Vec<char> = code
        .chars()
        .filter(char::is_ascii_hexdigit)
        .map(|digit| digit.to_ascii_uppercase())
        .collect();
    let mut packed = String::with_capacity(32);
    for group in [0..8, 8..12, 12..16] {
        packed.extend(hex[group].iter().rev());
    }
    for pair in hex[16..].chunks(2) {
        packed.push(pair[1]);
        packed.push(pair[0]);
    }
    Some(packed)
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

#[cfg(test)]
mod tests {
    use super::packed_product_code;

    #[test]
    fn product_codes_pack_as_windows_installer_names_them() {
        assert_eq!(
            packed_product_code("{F78B4949-77EC-441F-B1E9-9673F41E1A18}").as_deref(),
            Some("9494B87FCE77F1441B9E69374FE1A181")
        );
        assert_eq!(
            packed_product_code("{90140000-0011-0000-0000-0000000ff1ce}").as_deref(),
            Some("00004109110000000000000000F01FEC")
        );
        assert_eq!(
            packed_product_code("90140000-0011-0000-0000-0000000FF1CE"),
            None
        );
        assert_eq!(packed_product_code("{not-a-product-code}"), None);
    }
}
