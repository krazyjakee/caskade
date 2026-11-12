//! Asks before deleting a collection (the webview's
//! `components/collections/DeleteCollectionDialog.tsx`); its games stay in
//! the library.

use crate::data::{change, use_library};
use crate::ui::ConfirmDialog;
use caskade_core::model::CollectionSummary;
use caskade_ui_i18n::t;
use dioxus_native::prelude::*;

#[component]
pub fn DeleteCollectionDialog(
    collection: CollectionSummary,
    on_close: EventHandler<()>,
    /// Deleted: the collection's page goes back to the list.
    #[props(default)]
    on_deleted: Option<EventHandler<()>>,
) -> Element {
    let library = use_library();
    let mut pending = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let id = collection.id.clone();
    let confirm = move |_| {
        if *pending.peek() {
            return;
        }
        let library = library.clone();
        let id = id.clone();
        pending.set(true);
        error.set(None);
        spawn(async move {
            match change(async move { library.delete_collection(&id).await }).await {
                Ok(_) => {
                    super::changed();
                    on_close.call(());
                    if let Some(on_deleted) = on_deleted {
                        on_deleted.call(());
                    }
                }
                Err(report) => {
                    error.set(Some(report.summary));
                    pending.set(false);
                }
            }
        });
    };
    rsx! {
        ConfirmDialog {
            title: t!("Delete {name}?", name = collection.name.clone()),
            confirm_label: t!("Delete"),
            pending_label: t!("Deleting…"),
            pending: pending(),
            on_confirm: confirm,
            on_close: move |_| on_close.call(()),
            p { {t!("The collection goes. Its games stay in your library.")} }
            if let Some(error) = error() {
                p { role: "alert", class: "text-sm text-danger", "{error}" }
            }
        }
    }
}
