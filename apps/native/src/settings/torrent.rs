//! Settings → Installer's magnet links and torrent files (#159), as the
//! webview's `TorrentLinksSettings`: how to add one, and whether the OS
//! opens them with Caskade. Linux makes Caskade their default app; Windows
//! only offers it, and the user chooses it in Default apps.

use caskade_app::torrent_links::{Support, TorrentLinks};
use caskade_ui_data::ErrorReport;
use caskade_ui_i18n::t;
use dioxus_native::prelude::*;

use crate::data::change;
use crate::ui::icons::{EXTERNAL_LINK, MAGNET};
use crate::ui::{Button, FailureNotice, NoticeVariant, SettingRow, SettingsSection, Switch};

async fn status() -> Result<TorrentLinks, ErrorReport> {
    #[cfg(test)]
    if let Some(status) = fake::status() {
        return Ok(status);
    }
    caskade_app::commands::torrent_links::torrent_links_status().await
}

async fn set(open: bool) -> Result<TorrentLinks, ErrorReport> {
    #[cfg(test)]
    if let Some(outcome) = fake::set(open) {
        return outcome;
    }
    caskade_app::commands::torrent_links::torrent_links_set(open).await
}

async fn choose() -> Result<(), ErrorReport> {
    #[cfg(test)]
    if fake::choose() {
        return Ok(());
    }
    caskade_app::commands::torrent_links::torrent_links_choose().await
}

#[component]
pub fn TorrentLinksSettings() -> Element {
    let mut query = use_resource(|| change(status()));
    let mut pending = use_signal(|| false);
    let mut failed = use_signal(|| None::<String>);
    let mut apply = move |open: bool| {
        pending.set(true);
        failed.set(None);
        spawn(async move {
            let outcome = change(set(open)).await;
            pending.set(false);
            match outcome {
                Ok(_) => query.restart(),
                Err(error) => failed.set(Some(error.summary)),
            }
        });
    };
    let mut choosing = use_signal(|| false);
    let open_default_apps = move |_| {
        choosing.set(true);
        failed.set(None);
        spawn(async move {
            let outcome = change(choose()).await;
            choosing.set(false);
            if let Err(error) = outcome {
                failed.set(Some(error.summary));
            }
        });
    };
    let read = query.read().clone();
    rsx! {
        SettingsSection {
            icon: Some(MAGNET),
            title: t!("Magnet links and torrent files"),
            description: t!("Paste a magnet link anywhere in Caskade, drop a .torrent file on it, or use Add game. Nothing downloads until you confirm, and nothing is seeded afterwards."),
            "data-part": "torrent-links",
            match read {
                None => rsx! {},
                // Not an empty card: say what couldn't be read (#55).
                Some(Err(error)) => rsx! {
                    FailureNotice {
                        error,
                        variant: NoticeVariant::Inline,
                        title: t!("Caskade couldn't check how this system opens them"),
                        on_retry: move |_| query.restart(),
                    }
                },
                Some(Ok(status)) => match status.support {
                    Support::Direct => rsx! {
                        SettingRow {
                            icon: Some(MAGNET),
                            label: t!("Open them with Caskade"),
                            description: t!("Your browser and file manager send magnet links and .torrent files to Caskade instead of your torrent client. Turning this off gives them back."),
                            Switch {
                                checked: status.magnet || status.torrent_files,
                                disabled: pending(),
                                on_change: move |open: bool| apply(open),
                            }
                        }
                    },
                    Support::Chooser => {
                        let chosen = status.magnet && status.torrent_files;
                        let description = if !status.registered {
                            t!("Windows lists Caskade in Default apps, where you choose it for magnet links and .torrent files.")
                        } else if chosen {
                            t!("Windows opens magnet links and .torrent files with Caskade.")
                        } else {
                            t!("Choose Caskade for magnet links and .torrent files in Windows' Default apps.")
                        };
                        rsx! {
                            SettingRow {
                                icon: Some(MAGNET),
                                label: t!("Offer Caskade for them"),
                                description,
                                Switch {
                                    checked: status.registered,
                                    disabled: pending(),
                                    on_change: move |open: bool| apply(open),
                                }
                            }
                            if status.registered && !chosen {
                                div { class: "mt-2 flex justify-end",
                                    Button {
                                        icon: Some(EXTERNAL_LINK),
                                        disabled: choosing(),
                                        on_press: open_default_apps,
                                        {t!("Open Default apps")}
                                    }
                                }
                            }
                        }
                    }
                    Support::Unsupported => rsx! {
                        p { class: "text-sm text-fg-muted",
                            {t!("This system can't send magnet links and .torrent files to Caskade yet.")}
                        }
                    },
                },
            }
            if let Some(error) = failed() {
                p { role: "alert", class: "mt-3 text-sm text-danger", "{error}" }
            }
        }
    }
}

/// A test's stand-in for the OS, as the webview's tests fake
/// `TorrentLinksApi`: tests never change this machine's default apps.
#[cfg(test)]
pub(crate) mod fake {
    use super::*;
    use std::sync::Mutex;

    struct Fake {
        status: TorrentLinks,
        calls: Vec<String>,
        fail: Option<String>,
    }

    static FAKE: Mutex<Option<Fake>> = Mutex::new(None);

    fn fake() -> std::sync::MutexGuard<'static, Option<Fake>> {
        FAKE.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) struct Faked;

    impl Drop for Faked {
        fn drop(&mut self) {
            *fake() = None;
        }
    }

    /// Fakes the OS from `status` until the guard drops; `fail` makes
    /// every change fail with it.
    pub(crate) fn start(status: TorrentLinks, fail: Option<&str>) -> Faked {
        *fake() = Some(Fake {
            status,
            calls: Vec::new(),
            fail: fail.map(str::to_owned),
        });
        Faked
    }

    pub(crate) fn calls() -> Vec<String> {
        fake()
            .as_ref()
            .map(|fake| fake.calls.clone())
            .unwrap_or_default()
    }

    pub(super) fn status() -> Option<TorrentLinks> {
        fake().as_ref().map(|fake| fake.status.clone())
    }

    pub(super) fn set(open: bool) -> Option<Result<TorrentLinks, ErrorReport>> {
        let mut fake = fake();
        let fake = fake.as_mut()?;
        fake.calls.push(format!("set {open}"));
        if let Some(fail) = &fake.fail {
            return Some(Err(ErrorReport::new(
                caskade_ui_data::ErrorCode::Other,
                fail.clone(),
            )));
        }
        match fake.status.support {
            Support::Direct => {
                fake.status.magnet = open;
                fake.status.torrent_files = open;
            }
            _ => fake.status.registered = open,
        }
        Some(Ok(fake.status.clone()))
    }

    pub(super) fn choose() -> bool {
        let mut fake = fake();
        match fake.as_mut() {
            Some(fake) => {
                fake.calls.push("choose".into());
                true
            }
            None => false,
        }
    }
}
