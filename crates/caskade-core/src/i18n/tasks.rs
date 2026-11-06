//! The status bar's background work in the language in use. The queue's
//! labels come from crates without gettext (the installer, discovery's
//! adapter, the plugin host) as well as the core, so a snapshot is said
//! again from the core's catalog as it leaves for the webview. A label it
//! doesn't know, one the core already translated, stays as it came.

use crate::{jobs::Snapshot, tr};

type Known = (&'static str, fn() -> String);

const LABELS: &[Known] = &[
    ("Importing library", || tr!("Importing library")),
    ("Updating metadata", || tr!("Updating metadata")),
    ("Scanning library", || tr!("Scanning library")),
    ("Scanning library source", || tr!("Scanning library source")),
    ("Merging library sources", || tr!("Merging library sources")),
    ("Starting library watch", || tr!("Starting library watch")),
    ("Detecting launchers", || tr!("Detecting launchers")),
    ("Checking storage", || tr!("Checking storage")),
    ("Finding storage", || tr!("Finding storage")),
    ("Remembering storage", || tr!("Remembering storage")),
    ("Backing up", || tr!("Backing up")),
    ("Planning a backup", || tr!("Planning a backup")),
    ("Checking a backup", || tr!("Checking a backup")),
    ("Checking the backup", || tr!("Checking the backup")),
    ("Restoring a backup", || tr!("Restoring a backup")),
    ("Removing old backups", || tr!("Removing old backups")),
    ("Starting plugin", || tr!("Starting plugin")),
    ("Running plugin", || tr!("Running plugin")),
    ("Reloading plugin", || tr!("Reloading plugin")),
    ("Preparing plugin", || tr!("Preparing plugin")),
    ("Installing plugin", || tr!("Installing plugin")),
    ("Reading plugin files", || tr!("Reading plugin files")),
    ("Reading installer", || tr!("Reading installer")),
    ("Reading game manifest", || tr!("Reading game manifest")),
    ("Reading disc image", || tr!("Reading disc image")),
    ("Preparing portable game", || tr!("Preparing portable game")),
    ("Preparing compatibility tools", || {
        tr!("Preparing compatibility tools")
    }),
    ("Installing game", || tr!("Installing game")),
    ("Updating game files", || tr!("Updating game files")),
    ("Checking game files", || tr!("Checking game files")),
    ("Moving game files", || tr!("Moving game files")),
    ("Cleaning install files", || tr!("Cleaning install files")),
    ("Recovering installs", || tr!("Recovering installs")),
    ("Verifying download", || tr!("Verifying download")),
];

/// `snapshot` with its labels in the language in use.
pub fn translated_tasks(mut snapshot: Snapshot) -> Snapshot {
    for task in &mut snapshot.tasks {
        if let Some((_, label)) = LABELS.iter().find(|(english, _)| *english == task.label) {
            task.label = label();
        }
    }
    snapshot
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::Task;
    use std::path::Path;

    #[test]
    fn labels_read_in_the_language_shown_and_others_stay() {
        let _sv = crate::i18n::scope_catalog(
            "sv",
            r#"msgid ""
msgstr ""
"Language: sv\n"
"Plural-Forms: nplurals=2; plural=(n != 1);\n"

msgid "Reading installer"
msgstr "Läser installationsprogrammet"
"#,
        );
        let snapshot = translated_tasks(Snapshot {
            revision: 3,
            tasks: vec![
                Task {
                    id: 1,
                    label: "Reading installer".into(),
                    queued: true,
                },
                Task {
                    id: 2,
                    label: "Matchar spel med IGDB".into(),
                    queued: false,
                },
            ],
        });
        assert_eq!(snapshot.revision, 3);
        assert_eq!(snapshot.tasks[0].label, "Läser installationsprogrammet");
        assert!(snapshot.tasks[0].queued);
        assert_eq!(snapshot.tasks[1].label, "Matchar spel med IGDB");
    }

    fn sources(dir: &Path, into: &mut String) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                sources(&path, into);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                into.push_str(&std::fs::read_to_string(path).unwrap());
            }
        }
    }

    /// Fails when a label changes or a new one appears, so the status bar
    /// can't drift back into English.
    #[test]
    fn the_table_matches_every_label_the_crates_use() {
        let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let mut text = String::new();
        for name in [
            "caskade-installer/src",
            "caskade-core/src",
            "caskade-sources/src",
            "caskade-plugin-host/src",
        ] {
            sources(&crates.join(name), &mut text);
        }
        sources(&crates.join("../apps/desktop/src-tauri/src"), &mut text);
        let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
        for (english, _) in LABELS {
            assert!(flat.contains(english), "nothing says {english:?} any more");
        }
        // The queue's calls with a literal label, past test code's own.
        for call in ["blocking(\"", "activity(\"", ".run(\""] {
            for piece in flat.split(call).skip(1) {
                let Some(label) = piece.split('"').next() else {
                    continue;
                };
                let test_label = matches!(
                    label,
                    "Parent"
                        | "Child"
                        | "Recovered"
                        | "Matching"
                        | "Slow worker"
                        | "Panic"
                        | "Source scan"
                        | "Other background work"
                );
                let looks_like_a_label = label.chars().next().is_some_and(char::is_uppercase)
                    && label.chars().all(|c| c.is_alphabetic() || c == ' ');
                assert!(
                    test_label
                        || !looks_like_a_label
                        || LABELS.iter().any(|(english, _)| *english == label),
                    "{label:?} isn't in the table"
                );
            }
        }
    }
}
