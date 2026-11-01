//! Static AppImage metadata through the installer's bounded squashfs reader.
use super::*;
use crate::env::KnownFolder;
use caskade_installer::formats::linux::appimage::{self, AppImageLimits};
pub fn inspect(env: &HostEnv, ctx: &ScanCtx, image: &Path) -> Result<Application, SourceError> {
    ctx.check()?;
    let image = env
        .resolve_path(image)
        .map_err(|e| SourceError::io(image, e))?;
    if !fs::metadata(&image)
        .map_err(|e| SourceError::io(&image, e))?
        .is_file()
    {
        return Err(SourceError::parse(
            &image,
            "AppImage must be a regular file",
        ));
    }
    let file = fs::File::open(&image).map_err(|e| SourceError::io(&image, e))?;
    let metadata = appimage::read_metadata(
        file,
        AppImageLimits {
            max_entries: limit(ctx),
            max_metadata_bytes: 1024 * 1024,
            max_icon_bytes: 1024 * 1024,
        },
    )
    .map_err(|e| SourceError::parse(&image, e))?;
    ctx.check()?;
    let title = metadata
        .name
        .or(metadata.appstream_name)
        .unwrap_or_else(|| {
            image
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("AppImage")
                .to_owned()
        });
    Ok(Application {
        title,
        location: image.clone(),
        identifier: metadata.appstream_id,
        executable: Some(image.clone()),
        launch: exec(image.clone(), image.parent().map(Path::to_path_buf)),
        icon: None,
        categories: metadata
            .categories
            .unwrap_or_default()
            .split(';')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect(),
        version: None,
    })
}
pub fn applications(env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<Application>, SourceError> {
    let mut output = vec![];
    let mut remaining = limit(ctx);
    ctx.check()?;
    let mut roots = vec![
        env.home().join("Applications"),
        env.home().join(".local/bin"),
    ];
    if let Some(root) = env.known_folder(KnownFolder::Applications) {
        roots.push(root.to_owned());
    }
    roots.sort();
    roots.dedup();
    for root in roots {
        let entries = match children(env, ctx, &root, &mut remaining) {
            Ok(entries) => entries,
            Err(error @ SourceError::Cancelled) => return Err(error),
            Err(error) => {
                ctx.warn(error.to_string());
                continue;
            }
        };
        for entry in entries {
            ctx.check()?;
            if !entry.file_type().is_ok_and(|kind| kind.is_file())
                || entry
                    .path()
                    .extension()
                    .is_none_or(|ext| !ext.eq_ignore_ascii_case("appimage"))
            {
                continue;
            }
            match inspect(env, ctx, &entry.path()) {
                Ok(app) => output.push(app),
                Err(error) => ctx.warn(error.to_string()),
            }
        }
    }
    ctx.check()?;
    Ok(output)
}
