use super::*;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmulatorOrigin {
    DesktopEntry,
    Shortcut,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmulatorCandidate {
    pub origin: EmulatorOrigin,
    pub application: Application,
}
/// Emulator-only Desktop Entries are handed off by category. Shortcuts have no
/// category: the emulator consumer supplies executable leaf names from
/// its definitions, rather than this module inventing emulator identity rules.
pub fn emulator_candidates(
    env: &HostEnv,
    ctx: &ScanCtx,
    shortcut_executables: &[&str],
) -> Result<Vec<EmulatorCandidate>, SourceError> {
    ctx.check()?;
    #[cfg(not(any(feature = "linux-sources", feature = "windows-sources")))]
    let _ = env;
    if shortcut_executables.len() > 256
        || shortcut_executables
            .iter()
            .any(|name| !safe_text(name) || name.contains(['/', '\\', ':']))
    {
        return Err(SourceError::Other {
            message: "emulator shortcut matcher requires bounded executable leaf names".into(),
        });
    }
    let mut result: Vec<EmulatorCandidate> = vec![];
    #[cfg(feature = "linux-sources")]
    if env.os() == crate::OperatingSystem::Linux {
        for app in crate::sources::desktop::emulator_candidates(env, ctx)? {
            result.push(EmulatorCandidate {
                origin: EmulatorOrigin::DesktopEntry,
                application: Application {
                    title: app.title,
                    location: app.desktop_file,
                    identifier: app.flatpak_id,
                    executable: match &app.launch {
                        LaunchTarget::Exec { program, .. } => Some(program.clone()),
                        _ => None,
                    },
                    launch: app.launch,
                    icon: app.icon,
                    categories: app.categories,
                    version: None,
                },
            });
        }
    }
    #[cfg(feature = "windows-sources")]
    if env.os() == crate::OperatingSystem::Windows {
        for app in crate::sources::startmenu::applications(env, ctx)? {
            let LaunchTarget::Exec { program, .. } = &app.launch else {
                continue;
            };
            let Some(name) = program.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if shortcut_executables
                .iter()
                .any(|known| name.eq_ignore_ascii_case(known))
            {
                result.push(EmulatorCandidate {
                    origin: EmulatorOrigin::Shortcut,
                    application: Application {
                        title: app.title,
                        location: app.path,
                        identifier: None,
                        executable: Some(program.clone()),
                        launch: app.launch,
                        icon: app.icon,
                        categories: vec!["Emulator".into()],
                        version: None,
                    },
                });
            }
        }
    }
    ctx.check()?;
    result.sort_by(|a, b| a.application.location.cmp(&b.application.location));
    Ok(result)
}
