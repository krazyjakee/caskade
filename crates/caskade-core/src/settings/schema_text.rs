//! The settings schemas' titles and descriptions in the language in use
//! (#214). The core serves them translated (`SettingsService::sections`)
//! and the webview shows them as they come.
//!
//! schemars writes them from `#[schemars(title)]` attributes and doc
//! comments, which extraction can't read, so each is marked again below
//! (`marked`, in the `settings` context). A test fails when a titled
//! setting's title or description isn't. Lookups ignore how the text is
//! wrapped. A plugin's schema is shown as it is.

use serde_json::Value;

/// The context the schemas' texts are translated in.
const CONTEXT: &str = "settings";

/// Translates `schema`'s titles and descriptions in place.
pub(super) fn translate(schema: &mut Value) {
    match schema {
        Value::Object(map) => {
            for (key, value) in map.iter_mut() {
                match (key.as_str(), value) {
                    ("title" | "description", Value::String(text)) => *text = translated(text),
                    // Values, not schemas.
                    ("default" | "const" | "enum" | "examples", _) => {}
                    (_, value) => translate(value),
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(translate),
        _ => {}
    }
}

fn translated(text: &str) -> String {
    let id = normalized(text);
    let shown = crate::i18n::pgettext(CONTEXT, &id);
    if shown == id { text.to_owned() } else { shown }
}

/// The text on one line, as it is marked.
fn normalized(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Every title and description a Settings panel shows from a core schema,
/// for extraction; the test below keeps the list complete.
#[cfg(test)]
fn marked() -> Vec<crate::i18n::Message> {
    use crate::msgp;
    vec![
        // core.emulation
        msgp!("settings", "Desktop full screen"),
        msgp!(
            "settings",
            "Play emulated games fullscreen in desktop mode too."
        ),
        // core.updates
        msgp!("settings", "Check for game updates"),
        msgp!(
            "settings",
            "Look for new releases of games installed from GitHub in the background."
        ),
        msgp!("settings", "Check every (hours)"),
        msgp!("settings", "How long to wait between checks of a game."),
        msgp!("settings", "Offer updates before playing"),
        msgp!(
            "settings",
            "When a newer release is out, ask before starting the game whether to update it first."
        ),
        // core.appUpdates
        msgp!("settings", "Check for Caskade updates"),
        msgp!(
            "settings",
            "Look for a new version of Caskade at startup and every few hours."
        ),
        // core.metadata
        msgp!("settings", "Fetch artwork and game info online"),
        msgp!(
            "settings",
            "Download Hasheous' game databases and match games to IGDB in the background, for covers, descriptions and genres."
        ),
        // core.saves
        msgp!("settings", "Save manifest URL"),
        msgp!(
            "settings",
            "Download save-location definitions from this URL."
        ),
        // core.accounts
        msgp!("settings", "Rating board"),
        // core.installer
        msgp!("settings", "Windows install method"),
        msgp!(
            "settings",
            "Run supported installers on Windows, or prefer extracting their files."
        ),
        msgp!("settings", "Run the installer"),
        msgp!("settings", "Extract the files"),
        msgp!("settings", "Execution policy"),
        msgp!("settings", "Never run installers"),
        msgp!("settings", "Ask first"),
        msgp!("settings", "Allow"),
        msgp!(
            "settings",
            "Allow installer processes, ask before running them, or only use plans that do not execute code."
        ),
        msgp!("settings", "Execution policy for downloads"),
        msgp!(
            "settings",
            "For games downloaded from a link (deep links, pasted or dropped URLs, magnet links and torrents, updates). The stricter of this and the execution policy applies."
        ),
        msgp!("settings", "Ask below confidence"),
        msgp!(
            "settings",
            "Pause a job when its best plan scores below this probability."
        ),
        msgp!("settings", "Installer language"),
        msgp!(
            "settings",
            "Leave empty for the installer's default. Unsupported language switches are reported by its handler."
        ),
        msgp!("settings", "Let installers create shortcuts"),
        msgp!(
            "settings",
            "Allow installer-created shortcuts where the package supports this preference."
        ),
        msgp!("settings", "Compatibility runner"),
        msgp!(
            "settings",
            "Proton uses umu. GE-Proton and Steam Proton use detected builds; system Wine uses the detected Wine executable. Applies to new jobs."
        ),
        msgp!("settings", "Proton via umu"),
        msgp!("settings", "GE-Proton via umu"),
        msgp!("settings", "Steam Proton via umu"),
        msgp!("settings", "System Wine"),
        msgp!("settings", "Source handling after install"),
        msgp!(
            "settings",
            "Keep, move to the system trash, or delete the selected local source after a successful managed install. Downloads have their own setting."
        ),
        msgp!("settings", "Keep source"),
        msgp!("settings", "Move to trash"),
        msgp!("settings", "Delete source"),
        msgp!("settings", "Keep downloads"),
        msgp!(
            "settings",
            "Retain downloaded installers after successful installation."
        ),
        msgp!("settings", "Concurrent downloads"),
        msgp!(
            "settings",
            "Maximum simultaneous downloads. Active downloads finish when this limit is lowered."
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppPaths, db::LibraryStore, settings::SettingsService};
    use std::collections::BTreeSet;

    /// The titles below the schema's root, and the descriptions beside
    /// them; a description with code in it is for developers.
    fn shown(schema: &Value, root: bool, found: &mut BTreeSet<String>) {
        match schema {
            Value::Object(map) => {
                if !root && let Some(Value::String(title)) = map.get("title") {
                    found.insert(normalized(title));
                    if let Some(Value::String(description)) = map.get("description")
                        && !description.contains('`')
                    {
                        found.insert(normalized(description));
                    }
                }
                for (key, value) in map {
                    if !matches!(key.as_str(), "default" | "const" | "enum" | "examples") {
                        shown(value, false, found);
                    }
                }
            }
            Value::Array(items) => items.iter().for_each(|item| shown(item, false, found)),
            _ => {}
        }
    }

    async fn settings() -> SettingsService {
        let dir = tempfile::tempdir().unwrap();
        let settings = SettingsService::new(LibraryStore::in_memory().await.unwrap());
        settings
            .register_core(&AppPaths::under(dir.path()))
            .unwrap();
        settings
    }

    #[tokio::test]
    async fn every_titled_setting_is_marked_for_translation() {
        let marked: BTreeSet<String> = marked().iter().map(|message| message.id.clone()).collect();
        let mut found = BTreeSet::new();
        for section in settings().await.sections().unwrap() {
            shown(&section.schema, true, &mut found);
        }
        let missing: Vec<_> = found.difference(&marked).collect();
        assert!(
            missing.is_empty(),
            "not in settings::schema_text::marked: {missing:#?}"
        );
    }

    #[tokio::test]
    async fn sections_are_served_in_the_language_shown() {
        const SV: &str = r#"msgid ""
msgstr ""
"Language: sv\n"
"Plural-Forms: nplurals=2; plural=(n != 1);\n"

msgctxt "settings"
msgid "Keep downloads"
msgstr "Behåll nedladdningar"

msgctxt "settings"
msgid "Retain downloaded installers after successful installation."
msgstr "Behåll hämtade installationsprogram efter en lyckad installation."
"#;
        let settings = settings().await;
        let keep = |settings: &SettingsService| {
            let installer = settings
                .sections()
                .unwrap()
                .into_iter()
                .find(|section| section.namespace == "core.installer")
                .unwrap();
            let property = &installer.schema["properties"]["keepDownloads"];
            (
                property["title"].as_str().unwrap().to_owned(),
                property["description"].as_str().unwrap().to_owned(),
            )
        };
        assert_eq!(keep(&settings).0, "Keep downloads");
        let _sv = crate::i18n::scope_catalog("sv", SV);
        assert_eq!(
            keep(&settings),
            (
                "Behåll nedladdningar".to_owned(),
                "Behåll hämtade installationsprogram efter en lyckad installation.".to_owned()
            )
        );
    }
}
