use super::*;
use cap_std::fs::{Dir, OpenOptions};
use std::{
    io::{Read, Write},
    path::Path,
};
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalogue {
    schema_version: u32,
    roots: Vec<StorageRoot>,
}
pub(super) fn write(config: &Path, roots: &[StorageRoot]) -> Result<()> {
    if roots.len() > 128 {
        return Err(StorageError::Invalid(crate::tr!(
            "storage catalogue supports at most 128 locations"
        )));
    }
    let bytes = serde_json::to_vec(&Catalogue {
        schema_version: 1,
        roots: roots.to_vec(),
    })?;
    if bytes.len() + 1 > 1024 * 1024 {
        return Err(StorageError::Invalid(crate::tr!(
            "root catalogue exceeds its recovery budget"
        )));
    }
    std::fs::create_dir_all(config)?;
    let dir = Dir::open_ambient_dir(config, cap_std::ambient_authority())?;
    let temporary = format!(".storage-roots-{}.tmp", crate::model::new_id());
    let mut file = dir.open_with(&temporary, OpenOptions::new().write(true).create_new(true))?;
    let result = (|| {
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        drop(file);
        dir.rename(&temporary, &dir, "storage-roots.json")?;
        #[cfg(unix)]
        dir.open(".")?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = dir.remove_file(&temporary);
    }
    result
}
pub(super) fn read(config: &Path) -> Result<Vec<StorageRoot>> {
    if !config.exists() {
        return Ok(vec![]);
    }
    let dir = Dir::open_ambient_dir(config, cap_std::ambient_authority())?;
    let file = match dir.open("storage-roots.json") {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.into()),
    };
    if !dir.symlink_metadata("storage-roots.json")?.is_file() {
        return Err(StorageError::Invalid(crate::tr!(
            "root catalogue must be a regular file"
        )));
    }
    let mut bytes = vec![];
    file.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err(StorageError::Invalid(crate::tr!(
            "root catalogue exceeds 1MiB"
        )));
    }
    let catalogue: Catalogue = serde_json::from_slice(&bytes)?;
    if catalogue.schema_version != 1 || catalogue.roots.len() > 128 {
        return Err(StorageError::Invalid(crate::tr!(
            "unsupported root catalogue"
        )));
    }
    Ok(catalogue.roots)
}
