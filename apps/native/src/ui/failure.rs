//! Any failure as an [`ErrorNotice`] (#160), as the webview's
//! `FailureNotice`: the core's summary, its recovery hints in words, and
//! its redacted details and log reference behind Show details. `on_retry`
//! adds Try again, the notice's first stop for a pad.

use caskade_core::error::{ErrorReport, RecoveryHint};
use caskade_ui_i18n::t;
use dioxus_native::prelude::*;

use super::button::{Button, ButtonSize};
use super::error_notice::{ErrorNotice, NoticeVariant};
use super::icons::ROTATE_CCW;

#[component]
pub fn FailureNotice(
    error: ErrorReport,
    /// What couldn't be done ("Settings couldn't be loaded").
    #[props(default)]
    title: Option<String>,
    #[props(default)] on_retry: Option<EventHandler<()>>,
    /// In place of "Try again".
    #[props(default)]
    retry_label: Option<String>,
    #[props(default)] variant: NoticeVariant,
    #[props(default)] class: String,
) -> Element {
    let mut hints: Vec<String> = error
        .hints
        .iter()
        // Try again says it already.
        .filter(|hint| !(on_retry.is_some() && **hint == RecoveryHint::Retry))
        .map(|hint| caskade_ui_i18n::text(None, caskade_ui_data::labels::hint(*hint), &[]))
        .collect();
    if cfg!(target_os = "macos") && error.hints.contains(&RecoveryHint::CheckPermissions) {
        hints.push(t!("If macOS is protecting this location, open System Settings, choose Privacy & Security, then Files and Folders, and allow Caskade to access it. For locations that require Full Disk Access, add Caskade there instead. Quit and reopen Caskade if macOS asks, then try again. Privacy access does not fix a read-only drive or folder ownership."));
    }
    let summary = if error.summary.is_empty() {
        caskade_ui_i18n::text(None, caskade_ui_data::labels::SOMETHING_WENT_WRONG, &[])
    } else {
        error.summary.clone()
    };
    let actions = on_retry.map(|retry| {
        let label = retry_label.clone().unwrap_or_else(|| t!("Try again"));
        rsx! {
            Button {
                size: ButtonSize::Sm,
                icon: Some(ROTATE_CCW),
                on_press: move |_| retry.call(()),
                "{label}"
            }
        }
    });
    rsx! {
        ErrorNotice {
            summary,
            title,
            hints,
            detail: error.detail.clone(),
            reference: Some(error.reference.clone()).filter(|reference| !reference.is_empty()),
            variant,
            class,
            actions,
        }
    }
}
