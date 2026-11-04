//! Native family uninstall detection, conservative registry ownership and
//! portable inverse effects. Manifest cleanup always follows a native attempt;
//! a stale/failed native program is diagnostic, never a reason to strand files.
mod detection;
mod model;
mod registry;
use crate::RuntimeSpec;
use crate::manifest::{InstallManifest, RelocationVerdict};
pub use detection::detect_native_uninstaller;
pub(super) use model::PreparedNative;
pub use model::{
    NativeFamily, NativeRegistry, NativeUninstallPolicy, NativeUninstallReport, NativeUninstaller,
    RegistryEffect, UninstallRegistration, packed_product_code,
};
pub use registry::PlatformRegistry;
#[cfg(windows)]
pub(crate) use registry::{Owner, VIEWS, created_effects, software_keys};
use std::{io, path::Path};
pub(super) fn prepare(
    game: &Path,
    manifest: &InstallManifest,
    registry: &dyn NativeRegistry,
    policy: NativeUninstallPolicy,
) -> io::Result<PreparedNative> {
    let recorded = detection::recorded(manifest);
    // Preserve the existing executor semantics for arbitrary uninstall DSLs.
    if !manifest.record().uninstall_steps.is_empty()
        && (recorded.is_none() || manifest.record().uninstall_steps.len() != 1)
    {
        return Ok(PreparedNative {
            steps: manifest.record().uninstall_steps.clone(),
            recognized: false,
            registrations: vec![],
            report: NativeUninstallReport::default(),
            forget_product: None,
        });
    }
    if recorded.is_none()
        && !matches!(
            manifest.record().plan.candidate.strategy,
            crate::Strategy::RunSilent | crate::Strategy::RunInteractive
        )
    {
        return Ok(PreparedNative {
            steps: vec![],
            recognized: false,
            registrations: vec![],
            report: NativeUninstallReport::default(),
            forget_product: None,
        });
    }
    let native_registry = matches!(
        recorded
            .as_ref()
            .map(|native| &native.runtime)
            .unwrap_or(&manifest.record().runtime),
        RuntimeSpec::Native
    );
    let entries = if native_registry && registry.available() {
        registry.uninstall_entries()?
    } else {
        vec![]
    };
    let registrations = entries
        .iter()
        .filter(|entry| detection::owned(game, entry))
        .filter(|entry| {
            !manifest.record().external_effects.iter().any(|effect| {
                matches!(effect, crate::manifest::ExternalEffect::Registry {
                    effect: RegistryEffect::UninstallKey { location, .. },
                } if *location == entry.location)
            })
        })
        .cloned()
        .collect();
    let candidate = detect_native_uninstaller(game, manifest, &entries)?;
    let recognized = recorded.is_some() || candidate.is_some();
    let mut report = NativeUninstallReport::default();
    let mut steps = vec![];
    // The recorded or detected MSI, before `candidate` moves into the report.
    let msi = candidate
        .as_ref()
        .or(recorded.as_ref())
        .filter(|native| {
            native.family == NativeFamily::Msi && native.runtime == RuntimeSpec::Native
        })
        .and_then(|native| native.product_code.clone());
    if let Some(candidate) = candidate {
        report.family = Some(candidate.family);
        report.program = Some(candidate.program.clone());
        let stale_msi = candidate.family == NativeFamily::Msi
            && native_registry
            && registry.available()
            && !entries.iter().any(|entry| {
                entry.windows_installer
                    && candidate.product_code.as_ref().is_some_and(|code| {
                        entry
                            .location
                            .path
                            .as_str()
                            .rsplit('/')
                            .next()
                            .is_some_and(|key| key.eq_ignore_ascii_case(code))
                    })
            });
        if policy == NativeUninstallPolicy::ManifestOnly || stale_msi {
            report.manifest_fallback = true;
            report.diagnostics.push(
                if stale_msi {
                    "MSI ProductCode is no longer registered"
                } else {
                    "manifest-only uninstall requested"
                }
                .into(),
            );
        } else {
            steps.push(candidate.step()?);
            report.attempted = true;
        }
    } else if recorded.is_some()
        || matches!(
            manifest.record().plan.candidate.strategy,
            crate::Strategy::RunSilent | crate::Strategy::RunInteractive
        )
        || matches!(
            manifest.record().relocation.verdict,
            RelocationVerdict::Rewritable | RelocationVerdict::Fragile
        )
    {
        report.manifest_fallback = true;
        report.diagnostics.push(
            "Native uninstaller is missing, changed, ambiguous, or unsafe after relocation".into(),
        );
    }
    // A missing game is idempotent even if its recorded native step is stale.
    if !game.exists() {
        steps.clear();
        report.attempted = false;
        report.manifest_fallback = recognized;
    }
    // When the manifest route removes an MSI game in msiexec's place (after a
    // move, #40), Windows Installer still lists the product; `msiexec /x` runs
    // afterwards to drop its records. Only on the host's own registry: other
    // hosts' `PlatformRegistry` is unavailable, and a Wine/Proton prefix is
    // not the Native runtime.
    let forget_product = match msi {
        Some(code)
            if steps.is_empty()
                && policy == NativeUninstallPolicy::PreferNative
                && registry.available() =>
        {
            let registered = entries.iter().any(|entry| {
                entry.windows_installer
                    && entry
                        .location
                        .path
                        .as_str()
                        .rsplit('/')
                        .next()
                        .is_some_and(|key| key.eq_ignore_ascii_case(&code))
            });
            let listed = registered
                || registry
                    .windows_installer_product(&code)
                    .unwrap_or_else(|error| {
                        report.diagnostics.push(format!(
                            "Could not check Windows Installer's record of {code}: {error}"
                        ));
                        false
                    });
            listed.then_some(code)
        }
        _ => None,
    };
    Ok(PreparedNative {
        steps,
        recognized,
        registrations,
        report,
        forget_product,
    })
}

/// Record owned Uninstall registrations after a native install. Matching uses
/// canonical alias targets when available. Serialized effects retain logical key
/// locations and identity hashes, never absolute install locations.
pub fn uninstall_registration_effects(
    game: &Path,
    registry: &dyn NativeRegistry,
) -> io::Result<Vec<crate::manifest::ExternalEffect>> {
    registry
        .uninstall_entries()?
        .into_iter()
        .filter(|entry| detection::owned(game, entry))
        .map(|entry| {
            let effect = RegistryEffect::UninstallKey {
                fingerprint: model::registration_fingerprint(&entry),
                location: entry.location,
            };
            effect.validate()?;
            Ok(crate::manifest::ExternalEffect::Registry { effect })
        })
        .collect()
}
