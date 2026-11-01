//! Type 2 AppImages. Installation copies the image and marks it executable;
//! the `.desktop` entry, icon and AppStream metadata are read statically from
//! the embedded squashfs. The AppImage runtime is never executed.
use crate::{
    ArtifactNode, Candidate, Fidelity, Format, HandlerId, Requirements, SourcePath, Step, Strategy,
    plan::{Handler, PlanContext, PlanError, Proposal},
};
use backhand::{
    BackhandError, FilesystemCompressor, FilesystemReader, InnerNode, Node, SquashfsFileReader,
    SuperBlock,
    compression::{CompressionAction, Compressor, DefaultCompressor},
    kind::Kind,
};
use quick_xml::{Reader, events::Event};
use std::{
    collections::BTreeMap,
    io::{self, BufReader, Read, Seek, SeekFrom},
    path::{Component, Path, PathBuf},
};

/// Largest squashfs block (1 MiB); no decompressed block may exceed it.
const MAX_BLOCK_BYTES: usize = 1024 * 1024;
/// Fallback squashfs search window when the ELF section table is unusable.
const MAX_OFFSET_SCAN: u64 = 16 * 1024 * 1024;
const MAX_LINK_HOPS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppImageLimits {
    pub max_entries: usize,
    pub max_metadata_bytes: u64,
    pub max_icon_bytes: u64,
}
impl Default for AppImageLimits {
    fn default() -> Self {
        Self {
            max_entries: 100_000,
            max_metadata_bytes: 1024 * 1024,
            max_icon_bytes: 4 * 1024 * 1024,
        }
    }
}

/// A file read from inside the image; `path` is relative to the image root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedFile {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppImageMetadata {
    pub squashfs_offset: u64,
    pub desktop_file: Option<PathBuf>,
    /// Desktop Entry `Name`.
    pub name: Option<String>,
    /// Desktop Entry `Exec`, unparsed; its program is the AppImage itself.
    pub exec: Option<String>,
    /// Desktop Entry `Categories`, used by read-only game/emulator probes.
    pub categories: Option<String>,
    /// Desktop Entry `Icon`.
    pub icon_name: Option<String>,
    /// Icon candidates in preference order: `.DirIcon`, then `Icon=` files.
    pub icons: Vec<EmbeddedFile>,
    pub appstream_id: Option<String>,
    pub appstream_name: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum AppImageError {
    #[error("not a type 2 AppImage")]
    NotAppImage,
    #[error("AppImage has no squashfs after its ELF runtime")]
    MissingSquashfs,
    #[error("AppImage squashfs is unreadable: {0}")]
    Squashfs(String),
    #[error("AppImage squashfs has more than {0} entries")]
    TooManyEntries(usize),
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// ELF magic plus the AppImage type 2 marker in `e_ident` padding.
pub fn is_appimage(header: &[u8]) -> bool {
    header.starts_with(b"\x7fELF") && header.get(8..11) == Some(b"AI\x02")
}

/// End of the ELF runtime (section header table end), which is where the
/// AppImage runtime itself looks for its squashfs.
pub fn elf_end(header: &[u8]) -> Option<u64> {
    let little = match header.get(5)? {
        1 => true,
        2 => false,
        _ => return None,
    };
    let int = |at: usize, width: usize| -> Option<u64> {
        let bytes = header.get(at..at + width)?;
        let mut value = 0u64;
        for i in 0..width {
            let byte = if little {
                bytes[width - 1 - i]
            } else {
                bytes[i]
            };
            value = (value << 8) | u64::from(byte);
        }
        Some(value)
    };
    let (shoff, shentsize, shnum) = match header.get(4)? {
        1 => (int(0x20, 4)?, int(0x2e, 2)?, int(0x30, 2)?),
        2 => (int(0x28, 8)?, int(0x3a, 2)?, int(0x3c, 2)?),
        _ => return None,
    };
    shoff.checked_add(shentsize.checked_mul(shnum)?)
}

/// Squashfs start: the ELF end when it holds `hsqs`, otherwise the first
/// `hsqs` within a bounded window (images with stripped section tables).
pub fn squashfs_offset<R: Read + Seek>(reader: &mut R) -> Result<u64, AppImageError> {
    let mut header = [0u8; 64];
    reader.seek(SeekFrom::Start(0))?;
    reader
        .read_exact(&mut header)
        .map_err(|_| AppImageError::NotAppImage)?;
    if !is_appimage(&header) {
        return Err(AppImageError::NotAppImage);
    }
    let size = reader.seek(SeekFrom::End(0))?;
    if let Some(offset) = elf_end(&header).filter(|offset| offset.saturating_add(4) <= size) {
        let mut magic = [0u8; 4];
        reader.seek(SeekFrom::Start(offset))?;
        reader.read_exact(&mut magic)?;
        if &magic == b"hsqs" {
            return Ok(offset);
        }
    }
    reader.seek(SeekFrom::Start(0))?;
    let mut window = vec![];
    reader
        .by_ref()
        .take(MAX_OFFSET_SCAN)
        .read_to_end(&mut window)?;
    window
        .windows(4)
        .skip(64)
        .position(|bytes| bytes == b"hsqs")
        .map(|position| position as u64 + 64)
        .ok_or(AppImageError::MissingSquashfs)
}

/// Read the desktop entry, icon and AppStream metadata without running or
/// mounting the image. Symlinks are followed only inside the image.
pub fn read_metadata<R: Read + Seek + Send>(
    mut reader: R,
    limits: AppImageLimits,
) -> Result<AppImageMetadata, AppImageError> {
    let offset = squashfs_offset(&mut reader)?;
    reader.seek(SeekFrom::Start(offset))?;
    // Offset 0 makes backhand use the window as-is instead of its own offset
    // reader, whose unchecked `offset + start` overflows on hostile tables.
    let filesystem = FilesystemReader::from_reader_with_offset_and_kind(
        BufReader::new(Window {
            inner: reader,
            offset,
        }),
        0,
        Kind::new_v4(&DECOMPRESSOR),
    )
    .map_err(|error| AppImageError::Squashfs(error.to_string()))?;
    if filesystem.root.nodes.len() > limits.max_entries {
        return Err(AppImageError::TooManyEntries(limits.max_entries));
    }
    let image = Image {
        filesystem: &filesystem,
        nodes: filesystem
            .files()
            .filter_map(|node| Some((relative(&node.fullpath)?, node)))
            .collect(),
    };
    let mut metadata = AppImageMetadata {
        squashfs_offset: offset,
        ..Default::default()
    };
    let mut desktop_files: Vec<_> = image
        .nodes
        .keys()
        .filter(|path| {
            path.components().count() == 1 && path.extension().is_some_and(|ext| ext == "desktop")
        })
        .cloned()
        .collect();
    desktop_files.sort();
    if let Some(path) = desktop_files.first() {
        let bytes = image.read(path, limits.max_metadata_bytes)?;
        let entry = desktop_entry(&String::from_utf8_lossy(&bytes));
        metadata.name = entry.get("Name").cloned();
        metadata.exec = entry.get("Exec").cloned();
        metadata.categories = entry.get("Categories").cloned();
        metadata.icon_name = entry.get("Icon").cloned();
        metadata.desktop_file = Some(path.clone());
    }
    let mut icon_paths = vec![PathBuf::from(".DirIcon")];
    if let Some(icon) = metadata
        .icon_name
        .as_deref()
        .filter(|icon| !icon.contains(['/', '\\']) && !icon.is_empty())
    {
        for extension in ["png", "svg", "xpm"] {
            icon_paths.push(format!("{icon}.{extension}").into());
        }
        let mut themed: Vec<_> = image
            .nodes
            .keys()
            .filter(|path| {
                path.starts_with("usr/share/icons")
                    && path.file_stem().is_some_and(|stem| stem == icon)
            })
            .cloned()
            .collect();
        themed.sort();
        icon_paths.extend(themed);
    }
    for path in icon_paths {
        if image.nodes.contains_key(&path)
            && let Ok(bytes) = image.read(&path, limits.max_icon_bytes)
        {
            metadata.icons.push(EmbeddedFile { path, bytes });
        }
    }
    let stem = metadata
        .desktop_file
        .as_ref()
        .and_then(|path| path.file_stem())
        .map(|stem| stem.to_string_lossy().into_owned());
    let mut appstream: Vec<_> = image
        .nodes
        .keys()
        .filter(|path| {
            (path.parent() == Some(Path::new("usr/share/metainfo"))
                || path.parent() == Some(Path::new("usr/share/appdata")))
                && path.file_name().is_some_and(|name| {
                    let name = name.to_string_lossy();
                    name.ends_with(".appdata.xml") || name.ends_with(".metainfo.xml")
                })
        })
        .cloned()
        .collect();
    // Prefer the file named after the desktop entry, then path order.
    appstream.sort_by_key(|path| {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let matches = stem.as_ref().is_some_and(|stem| {
            name.split_once('.')
                .is_some_and(|_| name.starts_with(stem.as_str()))
        });
        (!matches, path.clone())
    });
    if let Some(path) = appstream.first() {
        let bytes = image.read(path, limits.max_metadata_bytes)?;
        (metadata.appstream_id, metadata.appstream_name) = appstream_fields(&bytes);
    }
    Ok(metadata)
}

/// The squashfs as a stream starting at 0, with checked position arithmetic.
struct Window<R> {
    inner: R,
    offset: u64,
}
impl<R: Read> Read for Window<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buffer)
    }
}
impl<R: Seek> Seek for Window<R> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "squashfs seek out of range");
        let position = match position {
            SeekFrom::Start(start) => {
                SeekFrom::Start(self.offset.checked_add(start).ok_or_else(invalid)?)
            }
            other => other,
        };
        let absolute = self.inner.seek(position)?;
        match absolute.checked_sub(self.offset) {
            Some(relative) => Ok(relative),
            None => {
                self.inner.seek(SeekFrom::Start(self.offset))?;
                Err(invalid())
            }
        }
    }
}

struct Image<'a, 'b> {
    filesystem: &'a FilesystemReader<'b>,
    nodes: BTreeMap<PathBuf, &'a Node<SquashfsFileReader>>,
}
impl Image<'_, '_> {
    fn read(&self, path: &Path, limit: u64) -> Result<Vec<u8>, AppImageError> {
        let mut path = path.to_path_buf();
        for _ in 0..=MAX_LINK_HOPS {
            let node = self
                .nodes
                .get(&path)
                .ok_or_else(|| io::Error::other("AppImage entry is missing"))?;
            match &node.inner {
                InnerNode::File(file) => {
                    if file.file_len() as u64 > limit {
                        return Err(io::Error::other("AppImage entry exceeds read limit").into());
                    }
                    let mut bytes = Vec::with_capacity(file.file_len());
                    self.filesystem
                        .file(file)
                        .reader_checked()
                        .map_err(|error| AppImageError::Squashfs(error.to_string()))?
                        .take(limit + 1)
                        .read_to_end(&mut bytes)?;
                    if bytes.len() as u64 > limit {
                        return Err(io::Error::other("AppImage entry exceeds read limit").into());
                    }
                    return Ok(bytes);
                }
                InnerNode::Symlink(link) if link.link.is_relative() => {
                    path = path
                        .parent()
                        .map(|parent| parent.join(&link.link))
                        .and_then(|joined| relative(&joined))
                        .ok_or_else(|| io::Error::other("AppImage link leaves the image"))?;
                }
                _ => return Err(io::Error::other("AppImage entry is not a file").into()),
            }
        }
        Err(io::Error::other("AppImage symlink chain is too long").into())
    }
}

/// Lexically normalise an image path; `None` when it escapes the root.
fn relative(path: &Path) -> Option<PathBuf> {
    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(part) => clean.push(part),
            Component::ParentDir => {
                if !clean.pop() {
                    return None;
                }
            }
            Component::Prefix(_) => return None,
        }
    }
    (!clean.as_os_str().is_empty()).then_some(clean)
}

/// Unlocalised keys of the `[Desktop Entry]` group.
fn desktop_entry(text: &str) -> BTreeMap<String, String> {
    let mut in_entry = false;
    let mut values = BTreeMap::new();
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry
            && !line.starts_with('#')
            && let Some((key, value)) = line.split_once('=')
            && !key.contains('[')
        {
            values
                .entry(key.trim().to_owned())
                .or_insert_with(|| value.trim().replace("\\s", " "));
        }
    }
    values
}

/// Top-level `<id>` and unlocalised `<name>` of an AppStream component.
fn appstream_fields(bytes: &[u8]) -> (Option<String>, Option<String>) {
    let mut reader = Reader::from_reader(bytes);
    let mut stack: Vec<String> = vec![];
    let (mut id, mut name) = (None, None);
    let mut buffer = vec![];
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(start)) => {
                let localised = start
                    .attributes()
                    .flatten()
                    .any(|attribute| attribute.key.as_ref() == "xml:lang");
                let mut tag = start.name().as_ref().to_owned();
                if localised {
                    tag.push('@');
                }
                stack.push(tag);
            }
            Ok(Event::End(_)) => {
                stack.pop();
            }
            Ok(Event::Text(text)) if stack.len() == 2 && stack[0] == "component" => {
                let value = text.xml10_content().trim().to_owned();
                if value.is_empty() {
                    continue;
                }
                match stack[1].as_str() {
                    "id" => {
                        id.get_or_insert(value);
                    }
                    "name" => {
                        name.get_or_insert(value);
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buffer.clear();
    }
    (id, name)
}

/// Read-only decompressor: backhand's gzip plus bounded pure-Rust zstd, xz
/// and lz4, so no C library is needed on any host.
struct Decompressor;
static DECOMPRESSOR: Decompressor = Decompressor;

fn bounded(reader: impl Read, out: &mut Vec<u8>) -> Result<(), BackhandError> {
    let start = out.len();
    reader
        .take(MAX_BLOCK_BYTES as u64 + 1)
        .read_to_end(out)
        .map_err(|_| BackhandError::CorruptedOrInvalidSquashfs)?;
    if out.len() - start > MAX_BLOCK_BYTES {
        return Err(BackhandError::CorruptedOrInvalidSquashfs);
    }
    Ok(())
}

impl CompressionAction for Decompressor {
    type Error = BackhandError;
    type Compressor = Compressor;
    type FilesystemCompressor = FilesystemCompressor;
    type SuperBlock = SuperBlock;

    fn decompress(
        &self,
        bytes: &[u8],
        out: &mut Vec<u8>,
        compressor: Self::Compressor,
    ) -> Result<(), Self::Error> {
        match compressor {
            Compressor::Uncompressed | Compressor::Gzip => {
                DefaultCompressor.decompress(bytes, out, compressor)
            }
            Compressor::Zstd => bounded(
                ruzstd::decoding::StreamingDecoder::new(bytes)
                    .map_err(|_| BackhandError::CorruptedOrInvalidSquashfs)?,
                out,
            ),
            Compressor::Xz => bounded(
                lzma_rust2::XzReader::new_mem_limit(bytes, false, 64 * 1024),
                out,
            ),
            Compressor::Lz4 => {
                // backhand reserves the block or metadata size before calling.
                let start = out.len();
                let room = out.capacity().saturating_sub(start).min(MAX_BLOCK_BYTES);
                out.resize(start + room, 0);
                let written = lz4_flex::block::decompress_into(bytes, &mut out[start..])
                    .map_err(|_| BackhandError::CorruptedOrInvalidSquashfs)?;
                out.truncate(start + written);
                Ok(())
            }
            other => Err(BackhandError::UnsupportedCompression(format!("{other:?}"))),
        }
    }

    fn compress(
        &self,
        _: &[u8],
        _: Self::FilesystemCompressor,
        _: u32,
    ) -> Result<Vec<u8>, Self::Error> {
        Err(BackhandError::UnsupportedCompression(
            "AppImage squashfs access is read-only".into(),
        ))
    }
}

/// Copies an AppImage into the game directory under its own name and marks
/// it executable. Ranking drops it on non-Linux hosts (Linux payload).
#[derive(Debug, Default)]
pub struct AppImageHandler;
impl Handler for AppImageHandler {
    fn id(&self) -> HandlerId {
        HandlerId("appimage".into())
    }
    fn claims(&self, artifact: &ArtifactNode) -> bool {
        artifact.format == Format::AppImage
    }
    fn propose(&self, artifact: &ArtifactNode, _: &PlanContext<'_>) -> Result<Proposal, PlanError> {
        let Some(name) = artifact
            .path
            .file_name()
            .filter(|_| self.claims(artifact))
            .map(PathBuf::from)
        else {
            return Ok(Proposal::default());
        };
        Ok(Proposal {
            candidates: vec![Candidate {
                handler: self.id(),
                strategy: Strategy::Copy,
                confidence: 1.0,
                fidelity: Fidelity::Exact,
                requires: Requirements {
                    est_disk_bytes: artifact.size,
                    ..Default::default()
                },
                steps: vec![
                    Step::Copy {
                        from: SourcePath::Artifact {
                            artifact: artifact.id.clone(),
                            path: ".".into(),
                        },
                        to: name.clone(),
                    },
                    Step::SetExecutable { path: name },
                ],
                warnings: vec![],
            }],
            question: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn elf64(shoff: u64, shentsize: u16, shnum: u16) -> Vec<u8> {
        let mut header = vec![0u8; 64];
        header[..4].copy_from_slice(b"\x7fELF");
        header[4] = 2;
        header[5] = 1;
        header[8..11].copy_from_slice(b"AI\x02");
        header[0x28..0x30].copy_from_slice(&shoff.to_le_bytes());
        header[0x3a..0x3c].copy_from_slice(&shentsize.to_le_bytes());
        header[0x3c..0x3e].copy_from_slice(&shnum.to_le_bytes());
        header
    }

    #[test]
    fn elf_end_handles_both_classes_and_overflow() {
        assert_eq!(elf_end(&elf64(1000, 64, 3)), Some(1192));
        let mut elf32 = vec![0u8; 52];
        elf32[..4].copy_from_slice(b"\x7fELF");
        elf32[4] = 1;
        elf32[5] = 2;
        elf32[0x20..0x24].copy_from_slice(&500u32.to_be_bytes());
        elf32[0x2e..0x30].copy_from_slice(&40u16.to_be_bytes());
        elf32[0x30..0x32].copy_from_slice(&2u16.to_be_bytes());
        assert_eq!(elf_end(&elf32), Some(580));
        assert_eq!(elf_end(&elf64(u64::MAX, 64, 3)), None);
        assert_eq!(elf_end(b"\x7fELF"), None);
    }

    #[test]
    fn squashfs_offset_uses_elf_end_then_bounded_scan() {
        let mut image = elf64(100, 0, 0);
        image.resize(100, 0);
        image.extend_from_slice(b"hsqs....");
        assert_eq!(squashfs_offset(&mut Cursor::new(&image)).unwrap(), 100);
        let mut stripped = elf64(0, 0, 0);
        stripped.resize(300, 0);
        stripped.extend_from_slice(b"hsqs");
        assert_eq!(squashfs_offset(&mut Cursor::new(&stripped)).unwrap(), 300);
        let plain_elf = {
            let mut header = elf64(0, 0, 0);
            header[8..11].copy_from_slice(&[0; 3]);
            header
        };
        assert!(matches!(
            squashfs_offset(&mut Cursor::new(plain_elf)),
            Err(AppImageError::NotAppImage)
        ));
        assert!(matches!(
            squashfs_offset(&mut Cursor::new(elf64(0, 0, 0))),
            Err(AppImageError::MissingSquashfs)
        ));
    }

    #[test]
    fn invalid_squashfs_is_an_error_not_a_panic() {
        let mut image = elf64(64, 0, 0);
        image.extend_from_slice(b"hsqs");
        image.resize(4096, 0xff);
        assert!(matches!(
            read_metadata(Cursor::new(image), AppImageLimits::default()),
            Err(AppImageError::Squashfs(_))
        ));
    }

    #[test]
    fn paths_never_escape_the_image_root() {
        assert_eq!(relative(Path::new("/usr/../a.png")), Some("a.png".into()));
        assert_eq!(relative(Path::new("../a.png")), None);
        assert_eq!(relative(Path::new("/")), None);
    }

    #[test]
    fn desktop_and_appstream_fields_ignore_localised_values() {
        let entry = desktop_entry(
            "[Desktop Action x]\nName=Action\n[Desktop Entry]\nName[de]=Spiel\nName=My\\sGame\nExec=game %U\n",
        );
        assert_eq!(entry["Name"], "My Game");
        assert_eq!(entry["Exec"], "game %U");
        let (id, name) = appstream_fields(
            br#"<component><id>io.example.Game</id><name xml:lang="de">Spiel</name><name>Game</name><developer><name>Dev</name></developer></component>"#,
        );
        assert_eq!(id.as_deref(), Some("io.example.Game"));
        assert_eq!(name.as_deref(), Some("Game"));
    }

    #[test]
    fn handler_copies_under_source_name_and_marks_executable() {
        let artifact = ArtifactNode {
            id: crate::ArtifactId("image".into()),
            path: "Some Game-x86_64.AppImage".into(),
            format: Format::AppImage,
            platform: crate::PayloadPlatform::Linux,
            role: crate::Role::Application,
            size: Some(1024),
            facts: BTreeMap::new(),
            children: vec![],
            children_complete: true,
        };
        let host: crate::host::HostCaps =
            toml::from_str(include_str!("../../../tests/hosts/linux.toml")).unwrap();
        let policy = crate::Policy::default();
        let proposal = AppImageHandler
            .propose(&artifact, &PlanContext::new(&host, &policy))
            .unwrap();
        assert_eq!(
            proposal.candidates[0].steps[1],
            Step::SetExecutable {
                path: "Some Game-x86_64.AppImage".into()
            }
        );
    }
}
