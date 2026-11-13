//! The words a failure notice shows besides the core's own summary: each
//! recovery hint's label. This is their source (#139): the core's tests
//! check every hint is labelled here. They are English message ids, the
//! webview's word for word so their translations carry over; the UI
//! translates them where it shows them (#91).
//!
//! An [`ErrorReport`](caskade_core::error::ErrorReport)'s code is for
//! matching, never shown: the summary is the sentence for people.

use caskade_core::error::RecoveryHint;

/// English for the `app` domain, marked here and translated where it's
/// shown (`caskade_ui_i18n::text`): `just i18n` extracts it as it does
/// `t!`, so its translations stay in the catalogs.
macro_rules! t_noop {
    ($id:literal) => {
        $id
    };
}

/// What a recovery hint asks of the user. Exhaustive: a new hint doesn't
/// build until it has a label.
pub fn hint(hint: RecoveryHint) -> &'static str {
    match hint {
        RecoveryHint::Retry => t_noop!("Trying again may work."),
        RecoveryHint::RetryLater => t_noop!("The other side has trouble; try again in a while."),
        RecoveryHint::CheckConnection => t_noop!("Check the network connection."),
        RecoveryHint::ReconnectStorage => t_noop!("Reconnect the drive or network share."),
        RecoveryHint::FreeSpace => t_noop!("Free some disk space."),
        RecoveryHint::CheckPermissions => {
            t_noop!("Check that Caskade may read and write the folder.")
        }
        RecoveryHint::CheckCredentials => t_noop!("Check the server address and sign-in details."),
        RecoveryHint::SignIn => t_noop!("Sign in to an account that may do this."),
        RecoveryHint::Reload => t_noop!("Reload to see the current state."),
        RecoveryHint::Restart => t_noop!("Restart Caskade."),
        RecoveryHint::UpdateCaskade => t_noop!("Update Caskade to the newest version."),
    }
}

/// The summary when a failure has none of its own.
pub const SOMETHING_WENT_WRONG: &str = t_noop!("Something went wrong.");

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

    /// Every hint has its own label, a sentence, as the webview had them
    /// (`hintLabels` in its `reply.ts`) so their translations carry over.
    #[test]
    fn every_hint_has_its_own_sentence() {
        let labels: Vec<&str> = HINTS.into_iter().map(hint).collect();
        assert_eq!(
            labels,
            [
                "Trying again may work.",
                "The other side has trouble; try again in a while.",
                "Check the network connection.",
                "Reconnect the drive or network share.",
                "Free some disk space.",
                "Check that Caskade may read and write the folder.",
                "Check the server address and sign-in details.",
                "Sign in to an account that may do this.",
                "Reload to see the current state.",
                "Restart Caskade.",
                "Update Caskade to the newest version.",
            ]
        );
        for label in labels.iter().chain([&SOMETHING_WENT_WRONG]) {
            assert!(label.ends_with('.'), "{label}");
        }
        let unique: std::collections::BTreeSet<_> = labels.iter().collect();
        assert_eq!(unique.len(), labels.len());
    }
}
