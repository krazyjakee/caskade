use super::{
    ArchiveMember, CancellationToken, Exclusions, Result, RomFile, ScanError, ScanPhase,
    ScanProgress, ScanReport, ScanRequest, check, extension, path_text, scan_files,
};
use caskade_installer::{
    Format,
    formats::archive::{self, ArchiveOptions, EntryKind},
};
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
};
/// IO boundary: no symlink traversal, bounded reads, no archive extraction or
/// placeholders opened. File classification itself remains pure/testable.
pub fn enumerate(
    request: &ScanRequest<'_>,
    cancel: &CancellationToken,
    progress: &mut dyn FnMut(ScanProgress),
) -> Result<Vec<RomFile>> {
    check(cancel)?;
    let root = fs::canonicalize(&request.scanner.folder)?;
    let cap = cap_std::fs::Dir::open_ambient_dir(&root, cap_std::ambient_authority())?;
    let exclusions = Exclusions::compile(
        request
            .global_exclusions
            .iter()
            .chain(&request.scanner.exclusions),
    )?;
    let mut pending = vec![(root.clone(), 0)];
    let mut files = Vec::new();
    let mut visited = 0;
    let mut total_text = 0usize;
    let mut total_entries = 0usize;
    let mut archive_read_bytes = 0u64;
    while let Some((dir, depth)) = pending.pop() {
        check(cancel)?;
        if depth > request.limits.max_depth {
            return Err(ScanError::Limit("directory depth"));
        }
        for entry in fs::read_dir(&dir)? {
            check(cancel)?;
            let entry = entry?;
            visited += 1;
            if visited > request.limits.max_files {
                return Err(ScanError::Limit("directory entries"));
            }
            let path = entry.path();
            let relative = path
                .strip_prefix(&root)
                .map_err(|_| ScanError::Invalid(crate::tr!("file escaped scanner folder")))?;
            let text = path_text(relative)?;
            let metadata = entry.metadata()?;
            // file_type does not follow symlinks. Dir capability confines all
            // content opens even if a path is replaced after enumeration.
            if entry.file_type()?.is_symlink() {
                continue;
            }
            let (hidden, cloud_placeholder) = flags(&metadata);
            if hidden
                || text.split('/').any(|p| p.starts_with('.'))
                || exclusions.matches(&text)
                || (request.scanner.skip_cloud_placeholders && cloud_placeholder)
            {
                continue;
            }
            if metadata.is_dir() {
                if request.scanner.recursive {
                    pending.push((path, depth + 1));
                }
                continue;
            }
            if !metadata.is_file() {
                continue;
            }
            let mut file = RomFile {
                path: relative.to_path_buf(),
                size: metadata.len(),
                hidden,
                cloud_placeholder,
                ..Default::default()
            };
            match extension(relative).as_str() {
                "m3u" | "m3u8" | "cue" | "gdi" => {
                    let mut bytes = Vec::new();
                    cap.open(relative)?
                        .take(request.limits.max_playlist_bytes as u64 + 1)
                        .read_to_end(&mut bytes)?;
                    if bytes.len() > request.limits.max_playlist_bytes {
                        return Err(ScanError::Limit("playlist bytes"));
                    }
                    total_text = total_text.saturating_add(bytes.len());
                    if total_text > request.limits.max_total_text_bytes {
                        return Err(ScanError::Limit("total playlist text"));
                    }
                    file.playlist = Some(
                        caskade_installer::safety::decode_name(
                            &bytes,
                            caskade_installer::safety::NameEncoding::Detect,
                        )
                        .map_err(|e| ScanError::Invalid(e.to_string()))?,
                    );
                }
                "zip" | "7z"
                    if request.scanner.scan_archives
                        && !request
                            .extensions
                            .iter()
                            .any(|e| e.eq_ignore_ascii_case(&extension(relative))) =>
                {
                    let budget = request.limits.max_archive_read_bytes.min(
                        request
                            .limits
                            .max_total_archive_read_bytes
                            .saturating_sub(archive_read_bytes),
                    );
                    if budget == 0 {
                        return Err(ScanError::Limit("total archive reads"));
                    }
                    let mut reader = Bounded {
                        inner: cap.open(relative)?,
                        remaining: budget,
                        cancel,
                    };
                    let options = ArchiveOptions {
                        max_entries: request.limits.max_archive_entries,
                        max_metadata_bytes: request
                            .limits
                            .max_archive_read_bytes
                            .min(16 * 1024 * 1024)
                            as usize,
                        ..Default::default()
                    };
                    let format = if extension(relative) == "zip" {
                        Format::Zip
                    } else {
                        Format::SevenZip
                    };
                    // Invalid archives remain unmatched rather than aborting a
                    // whole collection. The list-only parser never runs tools.
                    let listed = archive::list(&mut reader, format, &options);
                    archive_read_bytes += budget - reader.remaining;
                    check(cancel)?;
                    if let Ok(entries) = listed {
                        total_entries = total_entries.saturating_add(entries.len());
                        if total_entries > request.limits.max_total_archive_entries {
                            return Err(ScanError::Limit("total archive entries"));
                        }
                        file.archive_entries = entries
                            .into_iter()
                            .filter(|e| e.kind == EntryKind::File)
                            .map(|e| ArchiveMember {
                                path: e.name.into(),
                                size: e.size,
                            })
                            .collect();
                    } else if let Err(error) = listed {
                        file.warnings.push(crate::tr!(
                            "Could not inspect archive: {error}",
                            error = error
                        ));
                    }
                }
                _ => {}
            }
            files.push(file);
            if files.len() % 64 == 0 {
                progress(ScanProgress {
                    phase: ScanPhase::Enumerate,
                    files: files.len() as u32,
                    games: 0,
                });
            }
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    progress(ScanProgress {
        phase: ScanPhase::Enumerate,
        files: files.len() as u32,
        games: 0,
    });
    Ok(files)
}
pub fn scan_folder(request: &ScanRequest<'_>, cancel: &CancellationToken) -> Result<ScanReport> {
    scan_folder_with_progress(request, cancel, &mut |_| {})
}
pub fn scan_folder_with_progress(
    request: &ScanRequest<'_>,
    cancel: &CancellationToken,
    progress: &mut dyn FnMut(ScanProgress),
) -> Result<ScanReport> {
    let files = enumerate(request, cancel, progress)?;
    progress(ScanProgress {
        phase: ScanPhase::Match,
        files: files.len() as u32,
        games: 0,
    });
    let report = scan_files(&files, request, cancel)?;
    progress(ScanProgress {
        phase: ScanPhase::Group,
        files: files.len() as u32,
        games: report.games.len() as u32,
    });
    Ok(report)
}
struct Bounded<'a, R> {
    inner: R,
    remaining: u64,
    cancel: &'a CancellationToken,
}
impl<R: Read> Read for Bounded<'_, R> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        if self.cancel.is_cancelled() {
            return Err(std::io::Error::other("ROM scan cancelled"));
        }
        if bytes.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            return Err(std::io::Error::other("ROM archive read limit exceeded"));
        }
        let len = bytes.len().min(self.remaining as usize).min(64 * 1024);
        let n = self.inner.read(&mut bytes[..len])?;
        self.remaining -= n as u64;
        Ok(n)
    }
}
impl<R: Seek> Seek for Bounded<'_, R> {
    fn seek(&mut self, p: SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(p)
    }
}
fn flags(meta: &fs::Metadata) -> (bool, bool) {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        let attrs = meta.file_attributes();
        (attrs & 0x2 != 0, attrs & (0x0040_0000 | 0x0004_0000) != 0)
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::macos::fs::MetadataExt;
        (
            meta.st_flags() & 0x0000_8000 != 0,
            meta.st_flags() & 0x4000_0000 != 0,
        )
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = meta;
        (false, false)
    }
}
