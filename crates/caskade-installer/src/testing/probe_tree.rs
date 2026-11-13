use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    ArtifactNode,
    probe::{ProbeEngine, ProbeLimits},
};

/// Every regular file and macOS app bundle under `root` (what an extraction
/// left in staging), identified by the built-in probes, in path order. Paths
/// are relative to `root`; symlinks are skipped, and a bundle is a node of
/// its own as well as the files inside it.
pub fn probe_tree(root: &Path) -> Vec<ArtifactNode> {
    fn walk(root: &Path, dir: &Path, found: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let kind = fs::symlink_metadata(&path).unwrap().file_type();
            let bundle = path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("app"));
            if kind.is_file() || (kind.is_dir() && bundle) {
                found.push(path.strip_prefix(root).unwrap().to_path_buf());
            }
            if kind.is_dir() {
                walk(root, &path, found);
            }
        }
    }
    let mut found = vec![];
    walk(root, root, &mut found);
    found.sort();
    let engine = ProbeEngine::with_builtin_formats(ProbeLimits::default());
    found
        .into_iter()
        .map(|path| {
            let full = root.join(&path);
            if full.is_dir() {
                let node = engine.probe_path(&full).unwrap().root;
                return ArtifactNode { path, ..node };
            }
            let mut file = fs::File::open(full).unwrap();
            engine.probe_reader(&mut file, &path).unwrap().root
        })
        .collect()
}
