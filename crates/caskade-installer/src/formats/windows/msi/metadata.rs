use super::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Seek},
    path::{Path, PathBuf},
};
type Row = BTreeMap<String, serde_json::Value>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MsiFile {
    pub id: String,
    pub path: PathBuf,
    pub size: u64,
    pub sequence: u32,
    pub cabinet: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MsiMedia {
    pub disk_id: u32,
    pub last_sequence: u32,
    pub cabinet: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MsiMetadata {
    pub properties: BTreeMap<String, String>,
    pub files: Vec<MsiFile>,
    pub media: Vec<MsiMedia>,
    pub shortcuts: Vec<Row>,
    pub custom_actions: Vec<Row>,
    /// Files mapped outside the declared application directory are omitted.
    pub omitted_files: Vec<String>,
}
impl MsiMetadata {
    pub fn unpacked_bytes(&self) -> Option<u64> {
        self.files
            .iter()
            .try_fold(0_u64, |sum, file| sum.checked_add(file.size))
    }
    pub fn is_partial(&self) -> bool {
        !self.custom_actions.is_empty()
            || !self.shortcuts.is_empty()
            || !self.omitted_files.is_empty()
    }
}
fn invalid(message: impl Into<String>) -> MsiError {
    MsiError::Invalid(message.into())
}
fn text<'a>(row: &'a Row, column: &str) -> Result<&'a str> {
    row.get(column)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| invalid(format!("missing string column {column}")))
}
fn optional<'a>(row: &'a Row, column: &str) -> Option<&'a str> {
    row.get(column).and_then(serde_json::Value::as_str)
}
fn integer(row: &Row, column: &str) -> Result<u32> {
    row.get(column)
        .and_then(serde_json::Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| invalid(format!("missing/nonnegative integer column {column}")))
}

pub(super) fn read<R: Read + Seek>(
    package: &mut msi::Package<R>,
    limits: MsiLimits,
) -> Result<MsiMetadata> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| parse(package, limits)))
        .map_err(|_| invalid("upstream MSI table reader rejected malformed metadata"))?
}
fn parse<R: Read + Seek>(package: &mut msi::Package<R>, limits: MsiLimits) -> Result<MsiMetadata> {
    let mut bytes = 0_u64;
    let mut rows = 0_usize;
    let mut table = |name: &str| -> Result<Vec<Row>> {
        if !package.has_table(name) {
            return Ok(vec![]);
        }
        let mut result = vec![];
        for row in package.select_rows(msi::Select::table(name))? {
            rows += 1;
            if rows > limits.max_rows {
                return Err(MsiError::Limit);
            }
            let mut record = BTreeMap::new();
            for (index, column) in row.columns().iter().enumerate() {
                let value = if let Some(value) = row[index].as_str() {
                    bytes = bytes
                        .checked_add(value.len() as u64)
                        .ok_or(MsiError::Limit)?;
                    if bytes > limits.max_metadata_bytes {
                        return Err(MsiError::Limit);
                    }
                    value.into()
                } else if let Some(value) = row[index].as_int() {
                    value.into()
                } else {
                    serde_json::Value::Null
                };
                record.insert(column.name().to_owned(), value);
            }
            result.push(record);
        }
        Ok(result)
    };
    let mut directories = BTreeMap::new();
    for row in table("Directory")? {
        if directories
            .insert(
                text(&row, "Directory")?.to_owned(),
                (
                    optional(&row, "Directory_Parent").map(str::to_owned),
                    text(&row, "DefaultDir")?.to_owned(),
                ),
            )
            .is_some()
        {
            return Err(invalid("duplicate Directory identifier"));
        }
    }
    let anchor = [
        "INSTALLDIR",
        "INSTALLFOLDER",
        "APPLICATIONFOLDER",
        "APPDIR",
        "TARGETDIR",
    ]
    .into_iter()
    .find(|id| directories.contains_key(*id))
    .ok_or_else(|| invalid("no application directory"))?;
    let mut components = BTreeMap::new();
    for row in table("Component")? {
        if components
            .insert(
                text(&row, "Component")?.to_owned(),
                text(&row, "Directory_")?.to_owned(),
            )
            .is_some()
        {
            return Err(invalid("duplicate Component identifier"));
        }
    }
    let mut media = vec![];
    for row in table("Media")? {
        let cabinet = optional(&row, "Cabinet")
            .ok_or_else(|| MsiError::Unsupported("uncompressed Media payload".into()))?
            .to_owned();
        let name = cabinet.strip_prefix('#').unwrap_or(&cabinet);
        let safe = crate::safety::sanitize_path(name)?;
        if safe.components().count() != 1 || safe != Path::new(name) {
            return Err(invalid("cabinet must be one literal safe filename"));
        }
        media.push(MsiMedia {
            disk_id: integer(&row, "DiskId")?,
            last_sequence: integer(&row, "LastSequence")?,
            cabinet,
        });
    }
    media.sort_by_key(|item| item.disk_id);
    let mut last = 0;
    let mut disk = 0;
    for item in &media {
        if item.disk_id <= disk || item.last_sequence <= last {
            return Err(invalid("overlapping Media ranges"));
        }
        disk = item.disk_id;
        last = item.last_sequence;
    }
    let mut files = vec![];
    let mut omitted_files = vec![];
    let mut names = BTreeSet::new();
    let mut sequences = BTreeSet::new();
    for row in table("File")? {
        let id = text(&row, "File")?.to_owned();
        let component_id = text(&row, "Component_")?;
        let directory = components
            .get(component_id)
            .ok_or_else(|| invalid("unknown file component"))?;
        let Some(base) = directory_path(directory, anchor, &directories)? else {
            omitted_files.push(id);
            continue;
        };
        let path = base.join(component(text(&row, "FileName")?)?);
        let sequence = integer(&row, "Sequence")?;
        if sequence == 0
            || !sequences.insert(sequence)
            || !names.insert(path.to_string_lossy().to_lowercase())
        {
            return Err(invalid("duplicate file path or sequence"));
        }
        let cabinet = media
            .iter()
            .find(|item| sequence <= item.last_sequence)
            .ok_or_else(|| invalid("file sequence has no Media"))?
            .cabinet
            .clone();
        if row
            .get("Attributes")
            .and_then(serde_json::Value::as_i64)
            .is_some_and(|a| a & 1024 != 0)
        {
            return Err(MsiError::Unsupported("non-cabinet file".into()));
        }
        files.push(MsiFile {
            id,
            path,
            size: u64::from(integer(&row, "FileSize")?),
            sequence,
            cabinet,
        });
    }
    files.sort_by_key(|file| file.sequence);
    let mut properties = BTreeMap::new();
    for row in table("Property")? {
        properties.insert(
            text(&row, "Property")?.to_owned(),
            text(&row, "Value")?.to_owned(),
        );
    }
    if let Some(code) = properties.get("ProductCode")
        && !product_code(code)
    {
        return Err(invalid("invalid ProductCode"));
    }
    Ok(MsiMetadata {
        properties,
        files,
        media,
        shortcuts: table("Shortcut")?,
        custom_actions: table("CustomAction")?,
        omitted_files,
    })
}
/// MSI target names use short|long and target:source notation.
fn component(name: &str) -> Result<PathBuf> {
    let target = name.split_once('|').map_or(name, |(_, long)| long);
    let path = crate::safety::sanitize_path(target)?;
    if path.components().count() != 1 {
        return Err(invalid("MSI directory/file name contains separators"));
    }
    Ok(path)
}
fn directory_path(
    id: &str,
    anchor: &str,
    directories: &BTreeMap<String, (Option<String>, String)>,
) -> Result<Option<PathBuf>> {
    let mut current = id;
    let mut seen = BTreeSet::new();
    let mut parts = vec![];
    loop {
        if current == anchor {
            break;
        }
        if seen.len() >= 128 || !seen.insert(current) {
            return Err(invalid("Directory cycle/depth"));
        }
        let (parent, name) = directories
            .get(current)
            .ok_or_else(|| invalid("missing Directory row"))?;
        let target = name.split(':').next().unwrap_or(name);
        if target != "." {
            parts.push(component(target)?);
        }
        let Some(parent) = parent else {
            return Ok(None);
        };
        current = parent;
    }
    let mut path = PathBuf::new();
    for part in parts.into_iter().rev() {
        path.push(part);
    }
    if path == Path::new(".") {
        path.clear();
    }
    Ok(Some(path))
}
pub(super) fn product_code(code: &str) -> bool {
    let bytes = code.as_bytes();
    bytes.len() == 38
        && bytes[0] == b'{'
        && bytes[37] == b'}'
        && bytes[1..37].iter().enumerate().all(|(index, b)| {
            if [8, 13, 18, 23].contains(&index) {
                *b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}
