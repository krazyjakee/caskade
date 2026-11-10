//! The words a failure notice shows besides the core's own summary: each
//! recovery hint's label, as the webview has them (`hintLabels` in
//! `apps/desktop/src/api/reply.ts`). They are English message ids; the UI
//! translates them where it shows them (#91).
//!
//! An [`ErrorReport`](caskade_core::error::ErrorReport)'s code is for
//! matching, never shown: the summary is the sentence for people.

use caskade_core::error::RecoveryHint;

/// What a recovery hint asks of the user. Exhaustive: a new hint doesn't
/// build until it has a label.
pub fn hint(hint: RecoveryHint) -> &'static str {
    match hint {
        RecoveryHint::Retry => "Trying again may work.",
        RecoveryHint::RetryLater => "The other side has trouble; try again in a while.",
        RecoveryHint::CheckConnection => "Check the network connection.",
        RecoveryHint::ReconnectStorage => "Reconnect the drive or network share.",
        RecoveryHint::FreeSpace => "Free some disk space.",
        RecoveryHint::CheckPermissions => "Check that Caskade may read and write the folder.",
        RecoveryHint::CheckCredentials => "Check the server address and sign-in details.",
        RecoveryHint::SignIn => "Sign in to an account that may do this.",
        RecoveryHint::Reload => "Reload to see the current state.",
        RecoveryHint::Restart => "Restart Caskade.",
        RecoveryHint::UpdateCaskade => "Update Caskade to the newest version.",
    }
}

/// The summary when a failure has none of its own.
pub const SOMETHING_WENT_WRONG: &str = "Something went wrong.";

#[cfg(test)]
mod tests {
    use super::*;

    const HINTS: [RecoveryHint; 11] = [
        RecoveryHint::Retry,
        RecoveryHint::RetryLater,
        RecoveryHint::CheckConnection,
        RecoveryHint::ReconnectStorage,
        RecoveryHint::FreeSpace,
        RecoveryHint::CheckPermissions,
        RecoveryHint::CheckCredentials,
        RecoveryHint::SignIn,
        RecoveryHint::Reload,
        RecoveryHint::Restart,
        RecoveryHint::UpdateCaskade,
    ];

    /// Every hint the core sends has the webview's label, word for word, so
    /// the native UI says what the webview says and keeps its translations.
    #[test]
    fn every_hint_has_the_webviews_label() {
        let reply = include_str!("../../../apps/desktop/src/api/reply.ts");
        for each in HINTS {
            let name = serde_json_name(each);
            let line = format!("  {name}: msg`{}`,", hint(each));
            assert!(reply.contains(&line), "reply.ts lacks `{line}`");
        }
        assert!(reply.contains(&format!("t`{SOMETHING_WENT_WRONG}`")));
    }

    fn serde_json_name(hint: RecoveryHint) -> String {
        format!("{hint:?}")
            .char_indices()
            .map(|(i, c)| if i == 0 { c.to_ascii_lowercase() } else { c })
            .collect()
    }
}
