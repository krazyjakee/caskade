use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// A managed root inside a disposable parent, with a sentinel outside the root.
#[derive(Debug)]
pub struct TestRoot {
    parent: tempfile::TempDir,
    root: PathBuf,
    sentinel: PathBuf,
}

impl TestRoot {
    pub fn new() -> io::Result<Self> {
        let parent = tempfile::tempdir()?;
        let root = parent.path().join("root");
        let sentinel = parent.path().join("sentinel");
        fs::create_dir(&root)?;
        fs::write(&sentinel, b"caskade-test-sentinel\n")?;
        Ok(Self {
            parent,
            root,
            sentinel,
        })
    }
    pub fn path(&self) -> &Path {
        &self.root
    }
    pub fn parent_path(&self) -> &Path {
        self.parent.path()
    }
    pub fn sentinel_path(&self) -> &Path {
        &self.sentinel
    }
    pub fn assert_sentinel_untouched(&self) {
        let metadata = fs::symlink_metadata(&self.sentinel).expect("sentinel removed");
        assert!(
            metadata.file_type().is_file(),
            "sentinel replaced by non-file"
        );
        assert_eq!(
            fs::read(&self.sentinel).expect("read sentinel"),
            b"caskade-test-sentinel\n"
        );
    }
    /// Assert that the root is empty and no unexpected siblings were created.
    pub fn assert_clean(&self) {
        self.assert_sentinel_untouched();
        let root_meta = fs::symlink_metadata(&self.root).expect("root removed");
        assert!(
            root_meta.file_type().is_dir(),
            "root replaced by non-directory"
        );
        assert!(
            fs::read_dir(&self.root)
                .expect("read root")
                .next()
                .is_none(),
            "root contains leftovers"
        );
        let mut siblings: Vec<_> = fs::read_dir(self.parent.path())
            .expect("read parent")
            .map(|entry| entry.expect("read sibling").file_name())
            .collect();
        siblings.sort();
        assert_eq!(
            siblings,
            ["root", "sentinel"],
            "unexpected out-of-root writes"
        );
    }
}
