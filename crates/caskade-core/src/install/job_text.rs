//! The installer's text as people read it. `caskade-installer` has no
//! gettext (its messages are for developers), yet its questions, warnings
//! and progress reach Activity, so the core says them again from its own
//! catalog each time a job is read: a language switch changes jobs already
//! listed. Text it doesn't know, such as an error from the OS, stays as it
//! came.

use super::JobSnapshot;
use crate::{tr, trn};
use caskade_installer::Question;

type Known = (&'static str, fn() -> String);

/// Why a file might be the game, why a question is asked, and warnings
/// with no details of their own, all by the installer's English.
const TEXTS: &[Known] = &[
    ("Top-level macOS app bundle", || {
        tr!("Top-level macOS app bundle")
    }),
    ("Unity executable with matching _Data directory", || {
        tr!("Unity executable with matching _Data directory")
    }),
    ("Unreal root bootstrapper", || {
        tr!("Unreal root bootstrapper")
    }),
    ("Unreal shipping executable fallback", || {
        tr!("Unreal shipping executable fallback")
    }),
    ("Windows GUI executable", || tr!("Windows GUI executable")),
    ("Windows executable", || tr!("Windows executable")),
    ("ELF executable matching host architecture", || {
        tr!("ELF executable matching host architecture")
    }),
    ("Conventional game start script", || {
        tr!("Conventional game start script")
    }),
    ("Mach-O executable", || tr!("Mach-O executable")),
    ("GOG playTasks", || tr!("GOG playTasks")),
    ("itch.io manifest action", || tr!("itch.io manifest action")),
    ("Desktop Entry Exec", || tr!("Desktop Entry Exec")),
    ("Info.plist CFBundleExecutable", || {
        tr!("Info.plist CFBundleExecutable")
    }),
    ("AppImage desktop entry", || tr!("AppImage desktop entry")),
    (
        "Installer produced no identifiable payload in the requested folder; choose its installed folder.",
        || {
            tr!(
                "Installer produced no identifiable payload in the requested folder; choose its installed folder."
            )
        },
    ),
    (
        "Expected payload was not installed in the requested folder; observed matching folders are retained for an explicit choice.",
        || {
            tr!(
                "Expected payload was not installed in the requested folder; observed matching folders are retained for an explicit choice."
            )
        },
    ),
    (
        "The moved Fragile installation has no previous-path anchor. Select its kept source or URL to reinstall; no paths were inferred or changed.",
        || {
            tr!(
                "The moved Fragile installation has no previous-path anchor. Select its kept source or URL to reinstall; no paths were inferred or changed."
            )
        },
    ),
    (
        "The legacy manifest has no previous real-path anchor. Supply its old location explicitly; it cannot be inferred safely.",
        || {
            tr!(
                "The legacy manifest has no previous real-path anchor. Supply its old location explicitly; it cannot be inferred safely."
            )
        },
    ),
    (
        "Both the previous and observed locations exist. Choose the authoritative installation before retargeting its alias or references.",
        || {
            tr!(
                "Both the previous and observed locations exist. Choose the authoritative installation before retargeting its alias or references."
            )
        },
    ),
    (
        "The alias already names another existing installation.",
        || tr!("The alias already names another existing installation."),
    ),
    (
        "Some recorded references cannot be rewritten safely. Known text/registry references were repaired; manifest-based uninstall remains required.",
        || {
            tr!(
                "Some recorded references cannot be rewritten safely. Known text/registry references were repaired; manifest-based uninstall remains required."
            )
        },
    ),
    (
        "A ROM scanner keeps these ROMs with the rest of the platform's games.",
        || tr!("A ROM scanner keeps these ROMs with the rest of the platform's games."),
    ),
    (
        "Unreferenced package script archive is skipped without execution",
        || tr!("Unreferenced package script archive is skipped without execution"),
    ),
    (
        "Uses macOS installer; receipts are recorded, installed files require native uninstall tracking",
        || {
            tr!(
                "Uses macOS installer; receipts are recorded, installed files require native uninstall tracking"
            )
        },
    ),
    (
        "The disk image licence agreement is accepted while mounting read-only",
        || tr!("The disk image licence agreement is accepted while mounting read-only"),
    ),
    (
        "Static extraction suppresses shortcuts and omits registry, INI, plugin and launch actions; unresolved external destinations are omitted",
        || {
            tr!(
                "Static extraction suppresses shortcuts and omits registry, INI, plugin and launch actions; unresolved external destinations are omitted"
            )
        },
    ),
    (
        "Embedded MSI payload extraction skips bundle prerequisites, custom actions and registration.",
        || {
            tr!(
                "Embedded MSI payload extraction skips bundle prerequisites, custom actions and registration."
            )
        },
    ),
    (
        "Best-effort 7-Zip overlay extraction; authored installer actions are skipped.",
        || tr!("Best-effort 7-Zip overlay extraction; authored installer actions are skipped."),
    ),
    (
        "No verified silent contract for this installer variant; run the wizard with supported presets only.",
        || {
            tr!(
                "No verified silent contract for this installer variant; run the wizard with supported presets only."
            )
        },
    ),
    (
        "Static extraction does not execute Code, Registry, INI or custom install actions",
        || tr!("Static extraction does not execute Code, Registry, INI or custom install actions"),
    ),
    (
        "Table extraction omits custom actions, external shortcuts and files outside the application directory",
        || {
            tr!(
                "Table extraction omits custom actions, external shortcuts and files outside the application directory"
            )
        },
    ),
    (
        "Flatpak manages this user installation outside the storage root",
        || tr!("Flatpak manages this user installation outside the storage root"),
    ),
    (
        "All discs are retained together. Insert-next-disc requests use the interactive installer fallback; automatic mounting is not enabled.",
        || {
            tr!(
                "All discs are retained together. Insert-next-disc requests use the interactive installer fallback; automatic mounting is not enabled."
            )
        },
    ),
    (
        "Delegated installations cannot be repaired by moving their manifest folder.",
        || tr!("Delegated installations cannot be repaired by moving their manifest folder."),
    ),
    (
        "Metadata launch target is missing or outside the game directory",
        || tr!("Metadata launch target is missing or outside the game directory"),
    ),
    (
        "The discovered target needs a runner that is not currently available",
        || tr!("The discovered target needs a runner that is not currently available"),
    ),
    (
        "Presets have not yet been hand-checked against a native installer version.",
        || tr!("Presets have not yet been hand-checked against a native installer version."),
    ),
    (
        "Install directory cannot be preset without a package-specific recipe.",
        || tr!("Install directory cannot be preset without a package-specific recipe."),
    ),
    (
        "Installer language cannot be preset without a package-specific recipe.",
        || tr!("Installer language cannot be preset without a package-specific recipe."),
    ),
    (
        "Shortcut suppression needs a package-specific recipe.",
        || tr!("Shortcut suppression needs a package-specific recipe."),
    ),
    (
        "Launch-at-finish suppression needs a package-specific recipe.",
        || tr!("Launch-at-finish suppression needs a package-specific recipe."),
    ),
];

/// What the installer is doing. Its markers for steps inside a job mean
/// nothing to anyone, so they say nothing: Activity then shows its own
/// "Working…".
const PROGRESS: &[Known] = &[
    ("started", || tr!("Starting…")),
    ("resumed", || tr!("Resuming…")),
    ("retrying", || tr!("Trying again…")),
    ("verifying", || tr!("Verifying…")),
    ("extracting", || tr!("Extracting…")),
    ("moving", || tr!("Moving…")),
    ("copying", || tr!("Copying…")),
    ("copied", || tr!("Copied")),
    ("moved", || tr!("Moved")),
    ("rewriting", || tr!("Updating paths…")),
    ("candidate_failed", || tr!("Trying another way…")),
    ("committed", || tr!("Finishing…")),
    ("completed", || tr!("Done")),
    ("nearby", || tr!("Downloading from a device nearby…")),
    ("Reading the torrent", || tr!("Reading the torrent")),
    ("Connecting to peers", || tr!("Connecting to peers")),
    ("Verifying downloaded pieces", || {
        tr!("Verifying downloaded pieces")
    }),
    ("Checking downloaded files", || {
        tr!("Checking downloaded files")
    }),
    ("Finding peers for the torrent's metadata", || {
        tr!("Finding peers for the torrent's metadata")
    }),
    ("Download complete", || tr!("Download complete")),
];

/// The installer's markers with nothing to say to people.
const SILENT: &[&str] = &[
    "progress",
    "step_started",
    "step_completed",
    "candidate_started",
    "finalize_started",
    "finalize_completed",
    "commit_started",
    "process_started",
    "process_completed",
];

fn known(table: &[Known], english: &str) -> Option<String> {
    table
        .iter()
        .find(|(text, _)| *text == english)
        .map(|(_, translated)| translated())
}

/// A sentence from the installer that carries a detail of its own.
fn with_detail(text: &str) -> Option<String> {
    if let Some(error) = text.strip_prefix("Installed successfully, but download cleanup failed: ")
    {
        return Some(tr!(
            "Installed successfully, but download cleanup failed: {error}",
            error = error
        ));
    }
    if let Some(scripts) = text.strip_prefix("Extraction skips package scripts: ") {
        return Some(tr!(
            "Extraction skips package scripts: {scripts}",
            scripts = scripts
        ));
    }
    if let Some(files) = text
        .strip_suffix(" files exceeded the reference scan limit; relocation remains Fragile.")
        .and_then(|count| count.parse::<u64>().ok())
    {
        return Some(trn!(
            "One file exceeded the reference scan limit; relocation remains Fragile.",
            "{n} files exceeded the reference scan limit; relocation remains Fragile.",
            files
        ));
    }
    if let Some(folder) = text.strip_prefix("Did not descend into ") {
        return Some(tr!("Did not descend into {folder}", folder = folder));
    }
    if let Some(folder) = text.strip_prefix("Ignored non-file manifest in ") {
        return Some(tr!(
            "Ignored non-file manifest in {folder}",
            folder = folder
        ));
    }
    None
}

fn text(english: &str) -> String {
    known(TEXTS, english)
        .or_else(|| with_detail(english))
        .unwrap_or_else(|| english.to_owned())
}

/// `Downloading from 3 peers at 1.2 MB/s`, the one progress line with
/// numbers in it.
fn torrent_speed(english: &str) -> Option<String> {
    let rest = english.strip_prefix("Downloading from ")?;
    let (peers, rate) = rest.split_once(" at ")?;
    let rate = rate.strip_suffix("/s")?;
    let count = peers
        .strip_suffix(" peers")
        .or_else(|| peers.strip_suffix(" peer"))?
        .parse::<u64>()
        .ok()?;
    Some(trn!(
        "Downloading from one peer at {rate}/s",
        "Downloading from {n} peers at {rate}/s",
        count,
        rate = rate
    ))
}

fn progress(english: &str) -> String {
    if SILENT.contains(&english) {
        return String::new();
    }
    known(PROGRESS, english)
        .or_else(|| torrent_speed(english))
        .unwrap_or_else(|| english.to_owned())
}

/// Writes `job`'s question, warnings and progress in the language in use.
pub(super) fn translate(job: &mut JobSnapshot) {
    for warning in &mut job.warnings {
        warning.message = text(&warning.message);
    }
    if let Some(shown) = &mut job.progress {
        shown.message = progress(&shown.message);
    }
    match &mut job.question {
        Some(Question::ChooseLaunchTarget { targets }) => {
            for target in targets {
                target.reason = text(&target.reason);
            }
        }
        Some(
            Question::ChooseInstallDirectory { reason, .. }
            | Question::ReinstallAfterMove { reason, .. }
            | Question::ChooseDestination { reason, .. },
        ) => *reason = text(reason),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::scope_catalog;
    use std::path::Path;

    /// The installer's sources and the core's own ROM question: where the
    /// English these tables match on comes from.
    fn sources() -> String {
        fn read(dir: &Path, into: &mut String) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    read(&path, into);
                } else if path.extension().is_some_and(|extension| extension == "rs") {
                    into.push_str(&std::fs::read_to_string(path).unwrap());
                }
            }
        }
        let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let mut text =
            std::fs::read_to_string(crates.join("caskade-core/src/install/roms.rs")).unwrap();
        read(&crates.join("caskade-installer/src"), &mut text);
        // rustfmt breaks long literals over lines.
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn every_known_text_is_still_what_the_installer_says() {
        let sources = sources();
        for (english, _) in TEXTS.iter().chain(PROGRESS) {
            assert!(
                sources.contains(english),
                "the installer no longer says {english:?}"
            );
        }
        for marker in SILENT {
            assert!(sources.contains(marker), "{marker}");
        }
    }

    const SV: &str = r#"msgid ""
msgstr ""
"Language: sv\n"
"Plural-Forms: nplurals=2; plural=(n != 1);\n"

msgid "Conventional game start script"
msgstr "Vanligt startskript för spel"

msgid "The discovered target needs a runner that is not currently available"
msgstr "Det hittade programmet behöver en körmiljö som inte finns just nu"

msgid "Extracting…"
msgstr "Packar upp…"

msgid "Installed successfully, but download cleanup failed: {error}"
msgstr "Installerat, men nedladdningen kunde inte städas bort: {error}"

msgid "Downloading from one peer at {rate}/s"
msgid_plural "Downloading from {n} peers at {rate}/s"
msgstr[0] "Laddar ner från en källa med {rate}/s"
msgstr[1] "Laddar ner från {n} källor med {rate}/s"
"#;

    fn job() -> JobSnapshot {
        let mut job = JobSnapshot::queued(
            super::super::JobKind::Install,
            "Testspel".into(),
            "/tmp/testspel".into(),
            None,
        );
        job.warnings.push(caskade_installer::Warning {
            code: "missing_runtime".into(),
            message: "The discovered target needs a runner that is not currently available".into(),
            path: None,
        });
        job.warnings.push(caskade_installer::Warning {
            code: "download_retained".into(),
            message: "Installed successfully, but download cleanup failed: disk full".into(),
            path: None,
        });
        job.warnings.push(caskade_installer::Warning {
            code: "source_retained".into(),
            message: "Permission denied (os error 13)".into(),
            path: None,
        });
        job.question = Some(Question::ChooseLaunchTarget {
            targets: vec![caskade_installer::LaunchTarget {
                path: "start.sh".into(),
                args: vec![],
                working_dir: ".".into(),
                env: Default::default(),
                runtime: caskade_installer::RuntimeSpec::Native,
                score: 125.0,
                reason: "Conventional game start script".into(),
            }],
        });
        job
    }

    #[test]
    fn a_job_reads_in_the_language_shown_with_details_as_they_came() {
        let _sv = scope_catalog("sv", SV);
        let mut job = job();
        job.progress = Some(super::super::JobProgress {
            bytes: 0,
            total_bytes: None,
            files: 0,
            eta_secs: None,
            message: "extracting".into(),
        });
        translate(&mut job);
        assert_eq!(
            job.warnings[0].message,
            "Det hittade programmet behöver en körmiljö som inte finns just nu"
        );
        assert_eq!(
            job.warnings[1].message,
            "Installerat, men nedladdningen kunde inte städas bort: disk full"
        );
        assert_eq!(job.warnings[2].message, "Permission denied (os error 13)");
        assert_eq!(job.progress.unwrap().message, "Packar upp…");
        let Some(Question::ChooseLaunchTarget { targets }) = job.question else {
            panic!("the question stays");
        };
        assert_eq!(targets[0].reason, "Vanligt startskript för spel");
    }

    #[test]
    fn the_torrent_line_keeps_its_numbers() {
        let _sv = scope_catalog("sv", SV);
        assert_eq!(
            progress("Downloading from 3 peers at 1.2 MB/s"),
            "Laddar ner från 3 källor med 1.2 MB/s"
        );
        assert_eq!(
            progress("Downloading from 1 peer at 10 KB/s"),
            "Laddar ner från en källa med 10 KB/s"
        );
    }

    #[test]
    fn the_installer_s_step_markers_say_nothing() {
        assert_eq!(progress("step_started"), "");
        assert_eq!(progress("progress"), "");
        assert_eq!(progress("some file.pak"), "some file.pak");
    }

    #[test]
    fn english_keeps_the_installer_s_sentence() {
        let mut job = job();
        translate(&mut job);
        assert_eq!(
            job.warnings[1].message,
            "Installed successfully, but download cleanup failed: disk full"
        );
    }
}
