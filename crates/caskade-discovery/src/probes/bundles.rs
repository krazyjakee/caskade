//! Neutral Info.plist application metadata; unlike the game source this does
//! not drop launchers/utilities/emulators or require a Games category.
use super::*;
use crate::{
    env::KnownFolder,
    parsers::{self, Limits},
};
use std::collections::BTreeSet;
pub fn inspect(env: &HostEnv, ctx: &ScanCtx, bundle: &Path) -> Result<Application, SourceError> {
    ctx.check()?;
    let bundle = env
        .resolve_path(bundle)
        .and_then(fs::canonicalize)
        .map_err(|e| SourceError::io(bundle, e))?;
    let bundle = env
        .resolve_path(&bundle)
        .map_err(|e| SourceError::io(&bundle, e))?;
    if !bundle.is_dir()
        || bundle
            .extension()
            .is_none_or(|ext| !ext.eq_ignore_ascii_case("app"))
    {
        return Err(SourceError::parse(
            &bundle,
            "expected .app bundle directory",
        ));
    }
    let info = bundle.join("Contents/Info.plist");
    let info = env
        .resolve_path(&info)
        .and_then(fs::canonicalize)
        .map_err(|e| SourceError::io(&info, e))?;
    if !info.starts_with(&bundle)
        || !fs::metadata(&info)
            .map_err(|e| SourceError::io(&info, e))?
            .is_file()
    {
        return Err(SourceError::parse(
            &info,
            "Info.plist must be a regular file inside the bundle",
        ));
    }
    let bytes =
        crate::sources::read(env, &info, 1024 * 1024).map_err(|e| SourceError::io(&info, e))?;
    let value = parsers::plist::parse(
        &bytes,
        &Limits {
            max_input_bytes: 1024 * 1024,
            max_string_bytes: 65536,
            max_nodes: 20000,
            max_depth: 32,
        },
    )
    .map_err(|e| SourceError::parse(&info, e))?;
    let dict = value
        .as_dictionary()
        .ok_or_else(|| SourceError::parse(&info, "Info.plist is not a dictionary"))?;
    let text = |key| {
        dict.get(key)
            .and_then(plist::Value::as_string)
            .filter(|s| safe_text(s))
    };
    let exe = text("CFBundleExecutable")
        .filter(|s| crate::sources::plain_name(s))
        .ok_or_else(|| SourceError::parse(&info, "unsafe/missing CFBundleExecutable"))?;
    let executable = env
        .resolve_path(&bundle.join("Contents/MacOS").join(exe))
        .and_then(fs::canonicalize)
        .map_err(|e| SourceError::io(&bundle, e))?;
    if !executable.starts_with(&bundle) || !executable.is_file() {
        return Err(SourceError::parse(
            &info,
            "bundle executable escapes bundle or is missing",
        ));
    }
    let icon = text("CFBundleIconFile")
        .filter(|s| crate::sources::plain_name(s))
        .and_then(|name| {
            let mut path = bundle.join("Contents/Resources").join(name);
            if path.extension().is_none() {
                path.set_extension("icns");
            }
            env.resolve_path(&path).and_then(fs::canonicalize).ok()
        })
        .filter(|p| p.is_file() && p.starts_with(&bundle));
    let title = text("CFBundleDisplayName")
        .or_else(|| text("CFBundleName"))
        .map(str::to_owned)
        .or_else(|| bundle.file_stem()?.to_str().map(str::to_owned))
        .unwrap_or_else(|| "Application".into());
    ctx.check()?;
    Ok(Application {
        title,
        location: bundle.clone(),
        identifier: text("CFBundleIdentifier").map(str::to_owned),
        executable: Some(executable.clone()),
        launch: exec(executable, Some(bundle)),
        icon,
        categories: text("LSApplicationCategoryType")
            .map(str::to_owned)
            .into_iter()
            .collect(),
        version: text("CFBundleShortVersionString")
            .or_else(|| text("CFBundleVersion"))
            .map(str::to_owned),
    })
}
pub fn applications(env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<Application>, SourceError> {
    let mut output = vec![];
    let mut seen = BTreeSet::new();
    let mut remaining = limit(ctx);
    ctx.check()?;
    for root in [KnownFolder::Applications, KnownFolder::UserApplications]
        .into_iter()
        .filter_map(|key| env.known_folder(key))
    {
        let entries = match children(env, ctx, root, &mut remaining) {
            Ok(entries) => entries,
            Err(error @ SourceError::Cancelled) => return Err(error),
            Err(error) => {
                ctx.warn(error.to_string());
                continue;
            }
        };
        for entry in entries {
            ctx.check()?;
            if !entry.file_type().is_ok_and(|kind| kind.is_dir())
                || entry
                    .path()
                    .extension()
                    .is_none_or(|ext| !ext.eq_ignore_ascii_case("app"))
            {
                continue;
            }
            match inspect(env, ctx, &entry.path()) {
                Ok(app) => {
                    if seen.insert(app.location.clone()) {
                        output.push(app);
                    }
                }
                Err(error) => ctx.warn(error.to_string()),
            }
        }
    }
    ctx.check()?;
    Ok(output)
}
