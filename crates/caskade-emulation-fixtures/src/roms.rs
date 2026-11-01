use crate::{images, write};
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Playnite-compatible exclusions deliberately exercised by the base tree.
pub const SCANNER_EXCLUSIONS: &[&str] =
    &["bios/", "excluded/", ">*.txt", "?^beta/.*", ">?\\(Beta\\)"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpectedGame {
    pub title: String,
    /// Relative to RomTree::root. Playlists claim their referenced tracks.
    pub roms: Vec<PathBuf>,
}

/// Expected grouping with all visible extensions accepted, archives accepted
/// directly, hidden files skipped and SCANNER_EXCLUSIONS applied. The table
/// is independent of any scanner implementation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RomTree {
    pub root: PathBuf,
    pub files: Vec<PathBuf>,
    pub expected_games: Vec<ExpectedGame>,
    pub claimed: Vec<PathBuf>,
    pub excluded: Vec<PathBuf>,
}

pub struct RomFixture {
    _temp: tempfile::TempDir,
    pub tree: RomTree,
}
impl RomFixture {
    pub fn thousand() -> io::Result<Self> {
        let temp = tempfile::tempdir()?;
        let tree = RomTree::generate(&temp.path().join("ROM collection"), 1000)?;
        Ok(Self { _temp: temp, tree })
    }
}

impl RomTree {
    /// Catch stale/mutated expected tables and unexpected generated payloads.
    pub fn validate(&self) -> io::Result<()> {
        use std::collections::BTreeSet;
        let mut actual = BTreeSet::new();
        let mut pending = vec![self.root.clone()];
        while let Some(dir) = pending.pop() {
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let kind = entry.file_type()?;
                if kind.is_symlink() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "ROM fixtures cannot contain symlinks",
                    ));
                }
                if kind.is_dir() {
                    pending.push(entry.path());
                } else {
                    actual.insert(
                        entry
                            .path()
                            .strip_prefix(&self.root)
                            .map_err(io::Error::other)?
                            .to_path_buf(),
                    );
                }
            }
        }
        let listed: BTreeSet<_> = self.files.iter().cloned().collect();
        let partition: Vec<_> = self
            .expected_games
            .iter()
            .flat_map(|g| g.roms.iter())
            .chain(self.claimed.iter())
            .chain(self.excluded.iter())
            .cloned()
            .collect();
        let expected: BTreeSet<_> = partition.iter().cloned().collect();
        if actual != listed
            || listed != expected
            || self.files.len() != listed.len()
            || partition.len() != expected.len()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "ROM tree differs from its expected partition",
            ));
        }
        Ok(())
    }
    /// Creates exactly `count` physical files (including playlists, tracks,
    /// archives and excluded files). Refuses nonempty destinations. The
    /// 1,000-file fixture has twenty extra two-disc sets, plus playlist sets.
    pub fn generate(root: &Path, count: usize) -> io::Result<Self> {
        if !(100..=100_000).contains(&count) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "ROM fixture count must be 100..=100000",
            ));
        }
        fs::create_dir_all(root)?;
        if fs::read_dir(root)?.next().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "ROM fixture destination must be empty",
            ));
        }
        let mut tree = Self {
            root: root.canonicalize()?,
            files: vec![],
            expected_games: vec![],
            claimed: vec![],
            excluded: vec![],
        };
        for (name, title) in [
            ("SNES/Chrono Trigger (USA) [!].sfc", "Chrono Trigger"),
            (
                "SNES/Legend of Zelda, The - A Link to the Past (Europe) (Rev 1).SFC",
                "The Legend of Zelda - A Link to the Past",
            ),
            (
                "SNES/Super_Mario™ World (Japan) (Rev 1).smc",
                "Super Mario World",
            ),
            ("Cartridges/Fixture Cartridge", "Fixture Cartridge"),
        ] {
            tree.file(name, b"")?;
            tree.game(title, &[name]);
        }
        let disc1 = "PlayStation/Final Fantasy VII (USA) (Disc 1).cue";
        let disc2 = "PlayStation/Final Fantasy VII (USA) (Disc 2).cue";
        let playlist = "PlayStation/Final Fantasy VII (USA).m3u";
        tree.file(disc1, b"FILE \"Final Fantasy VII (USA) (Disc 1) (Track 1).bin\" BINARY\n  TRACK 01 MODE2/2352\n    INDEX 01 00:00:00\nFILE \"Final Fantasy VII (USA) (Disc 1) (Track 2).bin\" BINARY\n  TRACK 02 AUDIO\n    INDEX 01 00:00:00\n")?;
        tree.file(disc2, b"FILE \"Final Fantasy VII (USA) (Disc 2).bin\" BINARY\n  TRACK 01 MODE2/2352\n    INDEX 01 00:00:00\n")?;
        tree.file(playlist, b"#EXTM3U\nFinal Fantasy VII (USA) (Disc 1).cue\nFinal Fantasy VII (USA) (Disc 2).cue\n")?;
        for track in [
            "PlayStation/Final Fantasy VII (USA) (Disc 1) (Track 1).bin",
            "PlayStation/Final Fantasy VII (USA) (Disc 1) (Track 2).bin",
            "PlayStation/Final Fantasy VII (USA) (Disc 2).bin",
        ] {
            tree.file(track, b"")?;
            tree.claimed.push(track.into());
        }
        tree.claimed.extend([disc1.into(), disc2.into()]);
        tree.game("Final Fantasy VII", &[playlist]);

        let cue = "PlayStation/Fixture Rally (Europe).cue";
        tree.file(cue, b"FILE \"Fixture Rally (Europe).bin\" BINARY\n TRACK 01 MODE2/2352\n INDEX 01 00:00:00\n")?;
        tree.file("PlayStation/Fixture Rally (Europe).bin", b"")?;
        tree.claimed
            .push("PlayStation/Fixture Rally (Europe).bin".into());
        tree.game("Fixture Rally", &[cue]);

        let gdi = "Dreamcast/Sonic Adventure (USA).gdi";
        tree.file(gdi, b"3\n1 0 4 2352 \"Sonic Adventure (USA) (Track 1).bin\" 0\n2 450 0 2352 \"Sonic Adventure (USA) (Track 2).raw\" 0\n3 45000 4 2352 \"Sonic Adventure (USA) (Track 3).bin\" 0\n")?;
        for track in [
            "Dreamcast/Sonic Adventure (USA) (Track 1).bin",
            "Dreamcast/Sonic Adventure (USA) (Track 2).raw",
            "Dreamcast/Sonic Adventure (USA) (Track 3).bin",
        ] {
            tree.file(track, b"")?;
            tree.claimed.push(track.into());
        }
        tree.game("Sonic Adventure", &[gdi]);

        for (name, title) in [
            ("Archives/Fixture Zip (USA).zip", "Fixture Zip"),
            ("Archives/Fixture Seven (Japan).7z", "Fixture Seven"),
        ] {
            if name.ends_with("zip") {
                images::zip(
                    &tree.root.join(name),
                    &[("Fixture Zip (USA).sfc", b"original synthetic cartridge")],
                )?;
            } else {
                images::seven_zip(&tree.root.join(name), "Fixture Seven (Japan).sfc")?;
            }
            tree.files.push(name.into());
            tree.game(title, &[name]);
        }
        for excluded in [
            "bios/scph5501.bin",
            "excluded/Excluded Game.chd",
            "notes.txt",
            "beta/Prototype.chd",
            "Other/Fixture Prototype (Beta).chd",
            ".hidden/Hidden Game.chd",
            ".Secret Game.chd",
            "SNES/._AppleDouble.sfc",
        ] {
            tree.file(excluded, b"")?;
            tree.excluded.push(excluded.into());
        }
        for set in 0..20 {
            let paths: Vec<_> = (1..=2)
                .map(|disc| {
                    format!("Multi Disc/Fixture Saga {set:02} (USA) (Disc {disc} of 2).chd")
                })
                .collect();
            for path in &paths {
                tree.file(path, b"")?;
            }
            tree.game(&format!("Fixture Saga {set:02}"), &[&paths[0], &paths[1]]);
        }
        let mut index = 0;
        while tree.files.len() < count {
            let path = format!(
                "Library {:02}/Fixture Adventure {index:04} (USA) [!].chd",
                index % 10
            );
            tree.file(&path, b"")?;
            tree.game(&format!("Fixture Adventure {index:04}"), &[&path]);
            index += 1;
        }
        tree.files.sort();
        tree.claimed.sort();
        tree.excluded.sort();
        tree.expected_games.sort_by(|a, b| a.title.cmp(&b.title));
        Ok(tree)
    }
    fn file(&mut self, name: &str, bytes: &[u8]) -> io::Result<()> {
        write(&self.root.join(name), bytes)?;
        self.files.push(name.into());
        Ok(())
    }
    fn game(&mut self, title: &str, paths: &[&str]) {
        self.expected_games.push(ExpectedGame {
            title: title.into(),
            roms: paths.iter().map(PathBuf::from).collect(),
        });
    }
    pub fn expected_discs(&self) -> usize {
        self.expected_games.iter().map(|g| g.roms.len()).sum()
    }
}
