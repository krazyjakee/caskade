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
        // core.streaming
        msgp!("settings", "Let other devices stream games from this one"),
        msgp!(
            "settings",
            "Other devices on the local network can play this device's games."
        ),
        msgp!("settings", "Devices added by address"),
        msgp!(
            "settings",
            "Devices to stream from that aren't found on the network by themselves."
        ),
        msgp!(
            "settings",
            "Use the streaming already set up on this device"
        ),
        msgp!(
            "settings",
            "Host through the streaming program someone already installed here, changing only Caskade's games in it."
        ),
        msgp!("settings", "Stream quality"),
        msgp!(
            "settings",
            "How games streamed from other devices look; each part left empty is chosen automatically."
        ),
        msgp!("settings", "Resolution"),
        msgp!("settings", "Width and height in pixels, like 1920x1080."),
        msgp!("settings", "Frame rate"),
        msgp!("settings", "Frames per second."),
        msgp!("settings", "Bitrate"),
        msgp!("settings", "Kilobits per second."),
        // core.saves
        msgp!("settings", "Save manifest URL"),
        msgp!(
            "settings",
            "Download save-location definitions from this URL."
        ),
        msgp!("settings", "Save versions to keep"),
        msgp!("settings", "Versions kept per game; the newest stay."),
        // core.ports
        msgp!("settings", "Show ports of games"),
        msgp!(
            "settings",
            "List fan decompilations, recompilations and source ports on game pages, from Decomp Buddy's install plans."
        ),
        msgp!("settings", "Port catalogue URL"),
        msgp!(
            "settings",
            "Download the install plans of ports from this URL."
        ),
        // core.sync
        msgp!("settings", "What syncs"),
        msgp!(
            "settings",
            "Choose Sync or This device only for each category."
        ),
        msgp!("settings", "Sync"),
        msgp!("settings", "This device only"),
        msgp!("settings", "Never use relays"),
        msgp!(
            "settings",
            "Connect to other devices only directly. A device only a relay could reach shows as offline."
        ),
        msgp!("settings", "Downloads from my devices"),
        msgp!(
            "settings",
            "Get a download from another of your devices on this network when it has the same file, before the internet. Game files never go through a relay."
        ),
        msgp!("settings", "Listening port"),
        msgp!(
            "settings",
            "A fixed UDP port for firewall rules; empty lets the system choose."
        ),
        msgp!("settings", "Play requests from my devices"),
        msgp!(
            "settings",
            "Your other devices can start a game here, as Play here would."
        ),
        msgp!("settings", "Accept copies from my devices"),
        msgp!(
            "settings",
            "A game another of your devices sends here is copied into the default storage without asking."
        ),
        msgp!("settings", "Settings kept on this device"),
        msgp!(
            "settings",
            "Settings sections that stay on this device while their category syncs."
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
        msgp!("settings", "Running installers"),
        msgp!("settings", "Never run installers"),
        msgp!("settings", "Ask first"),
        msgp!("settings", "Allow"),
        msgp!(
            "settings",
            "Let installers run, ask before each one, or only install games that need no installer program."
        ),
        msgp!("settings", "Running installers from downloads"),
        msgp!(
            "settings",
            "For games downloaded from a link (deep links, pasted or dropped addresses, magnet links and torrents, updates). The stricter of this and Running installers applies."
        ),
        msgp!("settings", "Ask when less sure than"),
        msgp!(
            "settings",
            "Ask before installing when Caskade is less sure than this of how a game installs."
        ),
        msgp!("settings", "Installer language"),
        msgp!(
            "settings",
            "Leave empty for the installer's default. Caskade says when an installer can't switch language."
        ),
        msgp!("settings", "Let installers create shortcuts"),
        msgp!(
            "settings",
            "Let installers add their own shortcuts, where they offer the choice."
        ),
        msgp!("settings", "Compatibility runner"),
        msgp!(
            "settings",
            "Proton uses umu. GE-Proton and Steam Proton use detected builds; system Wine uses the detected Wine executable. Applies to installs started after a change."
        ),
        msgp!("settings", "Proton via umu"),
        msgp!("settings", "GE-Proton via umu"),
        msgp!("settings", "Steam Proton via umu"),
        msgp!("settings", "System Wine"),
        msgp!("settings", "After installing from a file"),
        msgp!(
            "settings",
            "Keep the file you installed from, move it to the system trash, or delete it. Downloads have their own setting."
        ),
        msgp!("settings", "Keep the file"),
        msgp!("settings", "Move to trash"),
        msgp!("settings", "Delete the file"),
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
