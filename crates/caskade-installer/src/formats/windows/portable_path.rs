//! Paths inside a Windows installer's metadata are relative to its install
//! folder. They serialize with `/` on every host, so facts and plans read the
//! same everywhere; deserializing takes either separator.
use serde::Serializer;
use std::path::{Path, PathBuf};

fn portable(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

pub fn serialize<S: Serializer>(path: &Path, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&portable(path))
}

pub fn serialize_option<S: Serializer>(
    path: &Option<PathBuf>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match path {
        Some(path) => serializer.serialize_some(&portable(path)),
        None => serializer.serialize_none(),
    }
}

pub fn serialize_vec<S: Serializer>(paths: &[PathBuf], serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(paths.iter().map(|path| portable(path)))
}
