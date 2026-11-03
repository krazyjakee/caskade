//! Installed application deployments from files, including extra installations.
//! Runtime refs and exports without a matching active deployment do not count.
use super::*;
use crate::sources::{flatpak, read};
use std::collections::BTreeSet;
/// Installed applications, including launchers and emulators, without game filters.
/// User installation wins when the same app is installed in several roots.
pub fn applications(env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<Application>, SourceError> {
    let mut output = vec![];
    let mut seen = BTreeSet::new();
    let mut remaining = limit(ctx);
    ctx.check()?;
    for root in flatpak::installation_paths(env) {
        let entries = match children(env, ctx, &root.join("app"), &mut remaining) {
            Ok(entries) => entries,
            Err(error @ SourceError::Cancelled) => return Err(error),
            Err(error) => {
                ctx.warn(error.to_string());
                continue;
            }
        };
        for entry in entries {
            ctx.check()?;
            let Some(id) = entry
                .file_name()
                .to_str()
                .filter(|id| flatpak::valid_id(id))
                .map(str::to_owned)
            else {
                continue;
            };
            if seen.contains(&id) {
                continue;
            }
            let Ok(app) = env
                .resolve_path(&entry.path())
                .and_then(dunce::canonicalize)
            else {
                continue;
            };
            let Ok(root) = env.resolve_path(&root).and_then(dunce::canonicalize) else {
                continue;
            };
            if !app.starts_with(&root) {
                ctx.warn("Flatpak app escapes installation");
                continue;
            }
            let Ok(active) = env
                .resolve_path(&entry.path().join("current/active"))
                .and_then(dunce::canonicalize)
            else {
                continue;
            };
            if !active.starts_with(&app) || !active.join("files").is_dir() {
                ctx.warn("Flatpak deployment escapes app or has no files");
                continue;
            }
            let metadata = active.join("metadata");
            let bytes = match read(env, &metadata, 1 << 20) {
                Ok(bytes) => bytes,
                Err(error) => {
                    ctx.warn(format!("{}: {error}", metadata.display()));
                    continue;
                }
            };
            if flatpak::application_name(&bytes) != Some(id.as_str()) {
                ctx.warn(format!(
                    "{}: missing/mismatched Application/name",
                    metadata.display()
                ));
                continue;
            }
            seen.insert(id.clone());
            output.push(Application {
                title: id.clone(),
                location: active.join("files"),
                identifier: Some(id.clone()),
                executable: None,
                launch: LaunchTarget::Flatpak {
                    app_id: id,
                    args: vec![],
                },
                icon: None,
                categories: vec![],
                version: None,
            });
        }
    }
    ctx.check()?;
    Ok(output)
}
/// Query an app ID using the same file inventory; never runs `flatpak list`.
pub fn find(env: &HostEnv, ctx: &ScanCtx, id: &str) -> Result<Option<Application>, SourceError> {
    if !flatpak::valid_id(id) {
        return Err(SourceError::Other {
            message: "invalid Flatpak application ID".into(),
        });
    }
    Ok(applications(env, ctx)?
        .into_iter()
        .find(|app| app.identifier.as_deref() == Some(id)))
}
