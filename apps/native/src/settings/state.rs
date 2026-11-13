//! A settings section as a view reads and changes it (the webview's
//! `useSettings.tsx`): its value, loaded again when a SettingsChanged event
//! names it; changes that go through the core from the latest value, so a
//! quick second change never writes back an older snapshot; and what the
//! section says while it loads, saves or fails ([`SettingsFeedback`],
//! [`SettingsLoading`], [`SettingsStatus`]).

use std::sync::Arc;

use caskade_core::settings::{SettingsSection, SettingsSnapshot};
use caskade_ui_data::{ErrorReport, Key};
use caskade_ui_i18n::{t, tp};
use dioxus_native::prelude::*;
use serde_json::Value;

use crate::data::{Query, change, use_settings, use_settings_query};
use crate::ui::{FailureNotice, NoticeVariant};

/// A change to a section's value, from what's saved now.
pub type Change = Arc<dyn Fn(Value) -> Value + Send + Sync>;

/// One section's value and writes.
#[derive(Clone, Copy, PartialEq)]
pub struct SectionState {
    namespace: Signal<String>,
    settings: CopyValue<caskade_ui_data::Settings>,
    query: Query<SettingsSnapshot>,
    saving: Signal<bool>,
    save_error: Signal<Option<ErrorReport>>,
    /// The last change, for Retry.
    last: Signal<Option<Change>>,
}

/// The section `namespace`.
pub fn use_settings_section(namespace: &str) -> SectionState {
    let name = namespace.to_owned();
    let query = use_settings_query(Key::Settings(name.clone()), move |settings| {
        let name = name.clone();
        async move { settings.get(&name).await }
    });
    let settings = use_settings();
    SectionState {
        namespace: use_signal(|| namespace.to_owned()),
        settings: use_hook(|| CopyValue::new(settings)),
        query,
        saving: use_signal(|| false),
        save_error: use_signal(|| None),
        last: use_signal(|| None),
    }
}

impl SectionState {
    /// What's saved; `None` until it loads.
    pub fn value(&self) -> Option<Value> {
        match &*self.query.read() {
            Some(Ok(snapshot)) => Some(snapshot.value.clone()),
            _ => None,
        }
    }

    /// The section didn't load (or reload).
    pub fn load_error(&self) -> Option<ErrorReport> {
        match &*self.query.read() {
            Some(Err(error)) => Some(error.clone()),
            _ => None,
        }
    }

    /// The last write didn't save; the controls show what is saved.
    pub fn save_error(&self) -> Option<ErrorReport> {
        self.save_error.read().clone()
    }

    pub fn saving(&self) -> bool {
        (self.saving)()
    }

    /// Either failure, the load's first.
    pub fn error(&self) -> Option<ErrorReport> {
        self.load_error().or_else(|| self.save_error())
    }

    pub fn reload(&self) {
        let mut query = self.query;
        query.restart();
    }

    /// Changes the section from its latest value.
    pub fn set(&self, change: impl Fn(Value) -> Value + Send + Sync + 'static) {
        let change: Change = Arc::new(change);
        let mut last = self.last;
        last.set(Some(change.clone()));
        self.write(Some(change));
    }

    /// One field of an object section.
    pub fn set_field(&self, key: &str, value: Value) {
        let key = key.to_owned();
        self.set(move |mut current| {
            if let Some(object) = current.as_object_mut() {
                object.insert(key.clone(), value.clone());
            }
            current
        });
    }

    /// Puts the whole section back to its defaults.
    #[allow(dead_code, reason = "the panels of #104 and #105 call it")]
    pub fn reset(&self) {
        self.write(None);
    }

    /// Sends the last change again, after a failed write.
    #[allow(dead_code, reason = "the panels of #104 and #105 call it")]
    pub fn retry(&self) {
        let last = self.last.peek().clone();
        if last.is_some() {
            self.write(last);
        }
    }

    /// A change (`None`: a reset), saved in the background.
    fn write(&self, change_to: Option<Change>) {
        let settings = self.settings.read().clone();
        let namespace = self.namespace.peek().clone();
        let mut saving = self.saving;
        let mut save_error = self.save_error;
        let mut query = self.query;
        saving.set(true);
        spawn(async move {
            let result = change(async move {
                match change_to {
                    Some(change_to) => settings.update(&namespace, |value| change_to(value)).await,
                    None => settings.reset(&namespace).await,
                }
            })
            .await;
            saving.set(false);
            match result {
                Ok(_) => save_error.set(None),
                Err(error) => {
                    save_error.set(Some(error));
                    // The controls show what the core has saved.
                    query.restart();
                }
            }
        });
    }
}

/// A section's definition: its schema and defaults, from the same schemas
/// the core validates every save against.
pub fn use_settings_definition(namespace: &str) -> Query<SettingsSection> {
    let name = namespace.to_owned();
    use_settings_query(Key::Settings(name.clone()), move |settings| {
        let name = name.clone();
        async move {
            settings
                .sections()?
                .into_iter()
                .find(|section| section.namespace == name)
                .ok_or_else(|| {
                    ErrorReport::new(
                        caskade_ui_data::ErrorCode::Unavailable,
                        t!("This settings section is unavailable."),
                    )
                })
        }
    })
}

/// Every section's definition, for a reset that spans several.
pub fn use_settings_sections() -> Query<Vec<SettingsSection>> {
    use_settings_query(Key::Settings(String::new()), |settings| async move {
        settings.sections()
    })
}

/// Saving…, or what went wrong, above a section's controls.
#[component]
pub fn SettingsFeedback(error: Option<ErrorReport>, saving: bool) -> Element {
    if let Some(error) = error {
        return rsx! {
            FailureNotice { error, variant: NoticeVariant::Inline, class: "mb-3" }
        };
    }
    if saving {
        return rsx! {
            p { role: "status", class: "mb-3 text-sm text-fg-muted", {t!("Saving…")} }
        };
    }
    rsx! {}
}

/// While a section loads, or why it didn't, with Try again.
#[component]
pub fn SettingsLoading(error: Option<ErrorReport>, on_reload: EventHandler<()>) -> Element {
    match error {
        Some(error) => rsx! {
            FailureNotice {
                error,
                title: t!("Settings couldn't be loaded"),
                on_retry: move |_| on_reload.call(()),
            }
        },
        None => rsx! {
            p { role: "status", class: "p-4 text-sm text-fg-muted", {t!("Loading settings…")} }
        },
    }
}

/// One section's load and writes, shown beside its controls (#42): the
/// load failure with Try again, Saving…, and a write that didn't save with
/// Retry, which sends the same change again. Controls keep showing the
/// saved value meanwhile. `what` names the settings ("Automatic backups").
/// Nothing while the section loads or once everything saved.
#[component]
pub fn SettingsStatus(
    section: SectionState,
    what: String,
    #[props(default = "mt-3".to_owned())] class: String,
) -> Element {
    if section.value().is_none() {
        return match section.load_error() {
            Some(error) => rsx! {
                FailureNotice {
                    error,
                    variant: NoticeVariant::Inline,
                    class,
                    title: t!(
                        note = "{what} names a group of settings, such as Automatic backups",
                        "{what} couldn't be loaded",
                        what = what.clone()
                    ),
                    on_retry: move |_| section.reload(),
                }
            },
            None => rsx! {},
        };
    }
    if section.saving() {
        return rsx! {
            p { role: "status", class: "text-sm text-fg-muted {class}", {t!("Saving…")} }
        };
    }
    let Some(error) = section.save_error() else {
        return rsx! {};
    };
    rsx! {
        div { class: "flex flex-col gap-1 {class}",
            FailureNotice {
                error,
                variant: NoticeVariant::Inline,
                title: t!(
                    note = "{what} names a group of settings, such as Automatic backups",
                    "{what}: your change didn't save",
                    what = what.clone()
                ),
                on_retry: move |_| section.retry(),
                retry_label: tp!("settings write", "Retry"),
            }
            span { class: "text-xs text-fg-muted",
                {t!("The settings here show what's saved. Retry sends your change again.")}
            }
        }
    }
}

/// Whether `key` of a section's `value` is on; off when unset.
pub fn flag(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}
