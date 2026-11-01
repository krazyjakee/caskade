//! A failed install's summary for people (#214). The installer classifies
//! failures and writes an English summary; the core says it again from the
//! failure's kind, stage and file, in the language in use, each time a job
//! is read, so a language switch changes failures already listed.

use crate::i18n::{Message, Text};
use crate::{msg, msgp};
use caskade_installer::failure::{Failure, FailureKind, FailureStage, SecurityNote};
use std::path::Path;

/// `failure`'s summary, translated when it is shown.
pub fn failure_message(failure: &Failure) -> Message {
    let file = failure
        .path
        .as_deref()
        .and_then(Path::file_name)
        .map(|name| format!("\u{201c}{}\u{201d}", name.to_string_lossy()));
    // A file the failure names, else what it was.
    let file_or =
        |fallback: Message| -> Text { file.clone().map_or_else(|| fallback.into(), Text::Plain) };
    let stage = match failure.stage {
        FailureStage::Download => msgp!("install failure stage", "while downloading"),
        FailureStage::Inspect => msgp!("install failure stage", "while reading the installer"),
        FailureStage::Install => msgp!("install failure stage", "while installing"),
        FailureStage::Finalize => msgp!("install failure stage", "while finishing the install"),
        FailureStage::Commit => msgp!("install failure stage", "after installing"),
    };
    let text = match failure.kind {
        FailureKind::MissingSource => msg!(
            "{file} is no longer there ({stage}). It may have been moved, deleted or on a drive that was removed.",
            file = file_or(msgp!("install failure file", "The installer")),
            stage = stage
        ),
        FailureKind::MissingStaged => {
            msg!(
                "A file Caskade had just written disappeared {stage}.",
                stage = stage
            )
        }
        FailureKind::MissingOutput => msg!(
            "{file} is missing from the install, so the game was not added to the library.",
            file = file_or(msgp!("install failure file", "A file the game needs"))
        ),
        FailureKind::AccessDenied => msg!(
            "Access to {file} was refused {stage}.",
            file = file_or(msgp!("install failure file", "a file Caskade needs")),
            stage = stage
        ),
        FailureKind::Locked => msg!(
            "{file} is in use by another program {stage}.",
            file = file_or(msgp!("install failure file", "A file Caskade needs")),
            stage = stage
        ),
        FailureKind::Integrity => msg!(
            "{file} isn't what was expected: its checksum doesn't match. It was discarded.",
            file = file_or(msgp!("install failure file", "The download"))
        ),
        FailureKind::StorageDisconnected => match &failure.path {
            Some(path) => msg!(
                "The storage location {path} can't be reached. Reconnect the drive or choose another location.",
                path = format!("\u{201c}{}\u{201d}", path.display())
            ),
            None => msg!(
                "The storage location can't be reached. Reconnect the drive or choose another location."
            ),
        },
        FailureKind::DiskFull => msg!(
            "There isn't enough free space to finish. Free some space or choose another location."
        ),
        // Torrents (#159): nobody to download from, or metadata Caskade
        // can't use. The installer's detail is English and stable.
        FailureKind::Download if failure.detail.contains("no BitTorrent peers") => msg!(
            "No one sharing this torrent sent anything, so the download can't go on. It may have no active seeders right now; try again later."
        ),
        FailureKind::Download if failure.detail.contains("unusable torrent") => msg!(
            "This torrent can't be used: its metadata is damaged or of a kind Caskade doesn't support."
        ),
        FailureKind::Download if failure.transient => {
            msg!("The download failed because the network or the server had trouble.")
        }
        FailureKind::Download => msg!("The download failed."),
        FailureKind::Other => msg!("The install stopped {stage}.", stage = stage),
    };
    let note = match failure.security {
        Some(SecurityNote::Confirmed) => msg!(
            "Your security software reported a threat in this file and blocked or removed it. Caskade won't work around it; check the security software's history if you trust the file."
        ),
        Some(SecurityNote::Possible) if failure.occurrences > 1 => msg!(
            "This happened again. Security software may be removing or blocking the file; check its history before trying again."
        ),
        Some(SecurityNote::Possible) => {
            msg!("Security software can cause this, but Caskade can't tell for sure.")
        }
        None => return text,
    };
    msgp!(
        "install failure and a note about security software",
        "{failure} {note}",
        failure = text,
        note = note
    )
}

/// Writes `failure`'s summary in the language in use.
pub(crate) fn translate(failure: &mut Failure) {
    failure.summary = failure_message(failure).render();
}

#[cfg(test)]
mod tests {
    use super::*;

    const SV: &str = r#"msgid ""
msgstr ""
"Language: sv\n"
"Plural-Forms: nplurals=2; plural=(n != 1);\n"

msgctxt "install failure stage"
msgid "while downloading"
msgstr "under nedladdningen"

msgid "{file} is no longer there ({stage}). It may have been moved, deleted or on a drive that was removed."
msgstr "{file} finns inte längre ({stage}). Den kan ha flyttats, raderats eller legat på en enhet som togs bort."

msgctxt "install failure and a note about security software"
msgid "{failure} {note}"
msgstr "{failure} {note}"

msgid "This happened again. Security software may be removing or blocking the file; check its history before trying again."
msgstr "Det hände igen. Säkerhetsprogram kan ta bort eller blockera filen; kontrollera dess historik innan du försöker igen."
"#;

    fn missing() -> Failure {
        let mut failure = Failure::new(
            FailureKind::MissingSource,
            FailureStage::Download,
            Some("/downloads/setup.exe".into()),
            false,
            "No such file or directory",
        );
        failure.security = None;
        failure.resummarize();
        failure
    }

    #[test]
    fn the_core_says_what_the_installer_says_in_english() {
        let mut failure = missing();
        assert_eq!(failure_message(&failure).render(), failure.summary);
        failure.security = Some(SecurityNote::Possible);
        failure.occurrences = 2;
        failure.resummarize();
        assert_eq!(failure_message(&failure).source(), failure.summary);
        for kind in [
            FailureKind::DiskFull,
            FailureKind::Locked,
            FailureKind::Other,
        ] {
            let failure = Failure::new(kind, FailureStage::Install, None, false, "");
            assert_eq!(
                failure_message(&failure).render(),
                failure.summary,
                "{kind:?}"
            );
        }
    }

    #[test]
    fn a_failed_install_reads_in_the_language_shown() {
        let mut failure = missing();
        failure.security = Some(SecurityNote::Possible);
        failure.occurrences = 2;
        translate(&mut failure);
        assert!(
            failure
                .summary
                .starts_with("\u{201c}setup.exe\u{201d} is no longer there")
        );
        let _sv = crate::i18n::scope_catalog("sv", SV);
        translate(&mut failure);
        assert_eq!(
            failure.summary,
            "\u{201c}setup.exe\u{201d} finns inte längre (under nedladdningen). Den kan ha \
             flyttats, raderats eller legat på en enhet som togs bort. Det hände igen. \
             Säkerhetsprogram kan ta bort eller blockera filen; kontrollera dess historik \
             innan du försöker igen."
        );
    }
}
