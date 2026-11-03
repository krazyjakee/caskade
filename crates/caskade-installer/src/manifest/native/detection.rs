use super::model::*;
use crate::manifest::{InstallManifest, ManifestPath, RelocationVerdict};
use crate::{Program, RuntimeSpec, SourcePath, Step, Strategy};
use sha2::{Digest, Sha256};
use std::{
    io::{self, Read},
    path::Path,
};

pub(super) fn family_name(path: &Path) -> Option<NativeFamily> {
    let name = path.file_name()?.to_str()?.to_ascii_lowercase();
    if name.starts_with("uninstall") && name.ends_with(".exe") {
        Some(NativeFamily::Nsis)
    } else if name.starts_with("unins") && name.ends_with(".exe") {
        Some(NativeFamily::Inno)
    } else {
        None
    }
}
fn product_code(args: &[String]) -> Option<String> {
    let index = args.iter().position(|arg| arg.eq_ignore_ascii_case("/x"))?;
    let code = args.get(index + 1)?;
    crate::plan::presets::build_uninstall_args(
        crate::plan::presets::InstallerFamily::Msi,
        Some(code),
    )
    .ok()?;
    Some(code.to_ascii_uppercase())
}
fn from_program(
    program: &Program,
    args: &[String],
    runtime: &RuntimeSpec,
) -> Option<NativeUninstaller> {
    match program {
        Program::Source(SourcePath::Game(path)) => Some(NativeUninstaller {
            family: family_name(path)?,
            program: program.clone(),
            runtime: runtime.clone(),
            product_code: None,
        }),
        Program::Tool(tool)
            if tool.eq_ignore_ascii_case("msiexec") || tool.eq_ignore_ascii_case("msiexec.exe") =>
        {
            Some(NativeUninstaller {
                family: NativeFamily::Msi,
                program: Program::Tool("msiexec".into()),
                runtime: runtime.clone(),
                product_code: Some(product_code(args)?),
            })
        }
        _ => None,
    }
}
pub(super) fn recorded(manifest: &InstallManifest) -> Option<NativeUninstaller> {
    let mut found = manifest
        .record()
        .uninstall_steps
        .iter()
        .filter_map(|step| match step {
            Step::Run {
                program,
                args,
                runtime,
                ..
            }
            | Step::RunInteractive {
                program,
                args,
                runtime,
                ..
            } => from_program(program, args, runtime),
            _ => None,
        });
    let result = found.next()?;
    if found.next().is_some() {
        None
    } else {
        Some(result)
    }
}
pub(super) fn current_file(
    game: &Path,
    manifest: &InstallManifest,
    path: &Path,
) -> io::Result<bool> {
    let path = ManifestPath::new(
        path.to_str()
            .ok_or_else(|| io::Error::other("non Unicode uninstaller"))?,
    )
    .map_err(io::Error::other)?;
    let Some(file) = manifest
        .record()
        .files
        .iter()
        .find(|file| file.path == path)
    else {
        return Ok(false);
    };
    if file.size > 64 * 1024 * 1024 {
        return Ok(false);
    }
    let directory = match cap_std::fs::Dir::open_ambient_dir(game, cap_std::ambient_authority()) {
        Ok(directory) => directory,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    let mut options = cap_std::fs::OpenOptions::new();
    options.read(true);
    // Reject a leaf symlink as well as ancestors leaving the directory capability.
    if directory
        .symlink_metadata(&path)
        .map_or(true, |meta| !meta.is_file() || meta.is_symlink())
    {
        return Ok(false);
    }
    let mut input = directory.open_with(&path, &options)?;
    if input.metadata()?.len() != file.size {
        return Ok(false);
    }
    let mut bytes = Vec::new();
    input
        .by_ref()
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 != file.size || !bytes.starts_with(b"MZ") {
        return Ok(false);
    }
    Ok(file
        .sha256
        .as_ref()
        .is_none_or(|hash| format!("{:x}", Sha256::digest(&bytes)) == hash.as_str()))
}
fn normalize(text: &str) -> String {
    text.trim()
        .trim_matches('"')
        .trim_end_matches(['/', '\\'])
        .replace('\\', "/")
        .to_ascii_lowercase()
}
pub(super) fn owned(game: &Path, entry: &UninstallRegistration) -> bool {
    if registry_address(&entry.location).is_err() {
        return false;
    }
    let canonical_match = dunce::canonicalize(&entry.install_location)
        .ok()
        .zip(dunce::canonicalize(game).ok())
        .is_some_and(|(location, game)| location == game);
    let prefix = "software/microsoft/windows/currentversion/uninstall/";
    entry
        .location
        .path
        .as_str()
        .to_ascii_lowercase()
        .strip_prefix(prefix)
        .is_some_and(|key| !key.is_empty() && !key.contains('/'))
        && !entry.install_location.is_empty()
        && (canonical_match
            || normalize(&entry.install_location) == normalize(&game.to_string_lossy()))
}
/// A registry uninstall command split by Windows (CommandLineToArgvW) rules.
fn command_words(text: &str) -> Option<Vec<String>> {
    if text.len() > 65_536 || text.contains('\0') {
        return None;
    }
    crate::discover::split_args(text, true)
}
fn registration_program(
    entry: &UninstallRegistration,
    game: &Path,
    runtime: &RuntimeSpec,
) -> Option<NativeUninstaller> {
    if entry.windows_installer {
        let code = entry.location.path.as_str().rsplit('/').next()?;
        if crate::plan::presets::build_uninstall_args(
            crate::plan::presets::InstallerFamily::Msi,
            Some(code),
        )
        .is_ok()
        {
            return Some(NativeUninstaller {
                family: NativeFamily::Msi,
                program: Program::Tool("msiexec".into()),
                runtime: runtime.clone(),
                product_code: Some(code.to_ascii_uppercase()),
            });
        }
    }
    for text in [
        entry.quiet_uninstall_string.as_ref(),
        entry.uninstall_string.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        // A malformed quiet string still leaves the plain one to try.
        let Some(args) = command_words(text) else {
            continue;
        };
        let (program, tail) = args.split_first()?;
        if program.eq_ignore_ascii_case("msiexec") || program.eq_ignore_ascii_case("msiexec.exe") {
            let candidate = from_program(&Program::Tool("msiexec".into()), tail, runtime);
            if candidate.is_some() {
                return candidate;
            }
        }
        let program = program.replace('\\', "/");
        let root = game.to_string_lossy().replace('\\', "/");
        if program.len() > root.len()
            && program
                .get(..root.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(&root))
            && program.as_bytes().get(root.len()) == Some(&b'/')
        {
            let path = ManifestPath::new(&program[root.len() + 1..]).ok()?;
            if let Some(candidate) = from_program(
                &Program::Source(SourcePath::Game(path.as_path().into())),
                tail,
                runtime,
            ) {
                return Some(candidate);
            }
        }
    }
    None
}
/// Inspect only validated manifest inventory and strictly matched registry rows.
/// Relocated Rewritable/Fragile records deliberately use manifest-only cleanup.
pub fn detect_native_uninstaller(
    game: &Path,
    manifest: &InstallManifest,
    entries: &[UninstallRegistration],
) -> io::Result<Option<NativeUninstaller>> {
    if matches!(
        manifest.record().relocation.verdict,
        RelocationVerdict::Rewritable | RelocationVerdict::Fragile
    ) {
        return Ok(None);
    }
    let native_install = matches!(
        manifest.record().plan.candidate.strategy,
        Strategy::RunSilent | Strategy::RunInteractive
    );
    let recorded = recorded(manifest);
    if !native_install && recorded.is_none() {
        return Ok(None);
    }
    let runtime = &manifest.record().runtime;
    let candidate = recorded
        .or_else(|| {
            entries
                .iter()
                .filter(|entry| owned(game, entry))
                .find_map(|entry| registration_program(entry, game, runtime))
        })
        .or_else(|| {
            let mut candidates = manifest.record().files.iter().filter_map(|file| {
                let family = family_name(file.path.as_path())?;
                Some(NativeUninstaller {
                    family,
                    program: Program::Source(SourcePath::Game(file.path.as_path().into())),
                    runtime: runtime.clone(),
                    product_code: None,
                })
            });
            let result = candidates.next()?;
            if candidates.next().is_some() {
                None
            } else {
                Some(result)
            }
        });
    let Some(candidate) = candidate else {
        return Ok(None);
    };
    if let Program::Source(SourcePath::Game(path)) = &candidate.program
        && !current_file(game, manifest, path)?
    {
        return Ok(None);
    }
    Ok(Some(candidate))
}

#[cfg(test)]
mod tests {
    use super::command_words;

    #[test]
    fn command_words_keep_quoted_arguments_whole() {
        assert_eq!(
            command_words(r#""C:\Games\X\unins000.exe" /SILENT /LOG="C:\Games\X\un log.txt""#)
                .unwrap(),
            [
                r"C:\Games\X\unins000.exe",
                "/SILENT",
                r"/LOG=C:\Games\X\un log.txt"
            ]
        );
        assert_eq!(
            command_words(r"MsiExec.exe /X{F78B4949-77EC-441F-B1E9-9673F41E1A18}").unwrap(),
            ["MsiExec.exe", "/X{F78B4949-77EC-441F-B1E9-9673F41E1A18}"]
        );
        assert!(command_words(r#""C:\Games\X\unins000.exe /S"#).is_none());
    }
}
