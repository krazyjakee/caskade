//! The native UI's messages: the `app` and `ui` domains.

use super::{
    ExtractError, each_source,
    scan::{Messages, NATIVE_KEYWORDS},
};
use crate::po::Message;
use std::path::Path;

/// The native UI's sources. Their `t!`, `tn!`, `tp!`, `tnp!`, `trans!`,
/// `transp!` and `t_noop!` messages make the `app` and `ui` domains
/// ([`APP`](super::APP), [`UI`](super::UI)), which keep the retired
/// webview's English, contexts and translations. (Their `tr!` family stays
/// in [`CORE`](super::CORE).) `caskade-ui-data` marks with `t_noop!` the
/// English it hands the UI to translate (the recovery hints' labels).
pub const NATIVE_SOURCES: &[&str] = &[
    "apps/native/src",
    "crates/caskade-ui-i18n/src",
    "crates/caskade-ui-data/src",
];

/// The shared components' folder: a message used in a file under it is the
/// `ui` domain's, the rest are `app`'s.
pub const UI_KIT: &str = "apps/native/src/ui/";

/// Every message of the native UI's sources (both domains), by English and
/// context. Test files aren't scanned: their text is test data.
pub fn native_messages(root: &Path) -> Result<Vec<Message>, ExtractError> {
    scan_sorted(root, NATIVE_SOURCES)
}

/// The `t!` family's messages in `sources`, by English and context.
pub(super) fn scan_sorted(root: &Path, sources: &[&str]) -> Result<Vec<Message>, ExtractError> {
    let mut messages = Messages::with(NATIVE_KEYWORDS);
    messages.skip_tests = true;
    each_source(root, sources, true, |reference, text| {
        messages.scan(text, reference)
    })?;
    let mut list = messages.list;
    list.sort_by(|a, b| a.id.cmp(&b.id).then_with(|| a.context.cmp(&b.context)));
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_native_uis_messages_and_notes() {
        let mut messages = Messages::with(NATIVE_KEYWORDS);
        messages
            .scan(
                r##"
                macro_rules! t { ($($rest:tt)+) => { $crate::t!($($rest)+) }; }
                fn view(n: u64) {
                    let _ = t!("Quick menu");
                    let _ = t!(note = "A game's playtime, before any", "Not played");
                    let _ = tp!("size", "Unknown");
                    let _ = tn!("# game", "# games", n);
                    let _ = tnp!("library", "# file", "# files", n, extra = 1);
                    let _ = trans!("<0>{guide}</0> opens the quick menu", guide = g; 0 => |x| x);
                    let _ = tr!("A core message");
                }
                "##,
                "apps/native/src/a.rs",
            )
            .unwrap();
        let keys: Vec<_> = messages
            .list
            .iter()
            .map(|m| (m.context.as_deref(), m.id.as_str(), m.plural.as_deref()))
            .collect();
        assert_eq!(
            keys,
            [
                (None, "Quick menu", None),
                (None, "Not played", None),
                (Some("size"), "Unknown", None),
                (None, "# game", Some("# games")),
                (Some("library"), "# file", Some("# files")),
                (None, "<0>{guide}</0> opens the quick menu", None),
            ]
        );
        assert_eq!(
            messages.list[1].extracted_comments,
            ["A game's playtime, before any"]
        );
    }
}
