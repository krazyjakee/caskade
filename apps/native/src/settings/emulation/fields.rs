//! Emulation's shared fields, as the webview's `EmulationFields` and
//! `StackedTable`: a path field with Browse…, a platform's BIOS files, the
//! command line a profile resolves to, and rows that are cards with their
//! cells labelled (#61), so a narrow window doesn't scroll sideways.

use caskade_core::emulation::{ArgvPreview, BiosEntry, BiosState};
use caskade_ui_i18n::t;
use dioxus_native::prelude::*;

use crate::flows::paths::{PathRequest, PickKind, format_path, use_path_picker};
use crate::ui::TextField;
use crate::ui::icons::FOLDER_OPEN;
use crate::ui::{Button, ButtonSize};

/// A path typed or browsed for. `on_commit` saves it where the field saves
/// itself (on Enter or a chosen path); without it a form's Save does.
#[component]
pub fn PathField(
    label: String,
    value: String,
    on_change: EventHandler<String>,
    #[props(default)] on_commit: Option<EventHandler<String>>,
    /// A file rather than a folder.
    #[props(default)]
    file: bool,
    #[props(default)] disabled: bool,
    /// The input's id.
    #[props(default)]
    id: Option<String>,
) -> Element {
    let picker = use_path_picker();
    let browse_label = t!(
        note = "A path field's browse button; {label} is the field's own label",
        "Browse for {label}",
        label = label.clone()
    );
    let browse = {
        let label = label.clone();
        let value = value.clone();
        move |_| {
            let kind = if file {
                PickKind::File
            } else {
                PickKind::Folder
            };
            let mut request = PathRequest::new(kind, label.clone());
            if !file && !value.trim().is_empty() {
                request.start_at = Some(value.trim().to_owned());
            }
            spawn(async move {
                if let Some(path) = picker.pick(request).await {
                    on_change.call(path.clone());
                    if let Some(commit) = on_commit {
                        commit.call(path);
                    }
                }
            });
        }
    };
    rsx! {
        TextField {
            id,
            label,
            value: format_path(&value),
            mono: true,
            disabled,
            on_input: move |next: String| on_change.call(next),
            on_submit: move |next: String| {
                if let Some(commit) = on_commit {
                    commit.call(next);
                }
            },
            action: rsx! {
                Button {
                    size: ButtonSize::Sm,
                    icon: FOLDER_OPEN,
                    "aria-label": browse_label,
                    disabled,
                    on_press: browse,
                }
            },
        }
    }
}

/// A platform's BIOS files: how many were found, and each one's state.
#[component]
pub fn BiosSummary(files: Vec<BiosEntry>) -> Element {
    if files.is_empty() {
        return rsx! {
            span { class: "text-xs text-fg-muted", {t!(note = "No BIOS files are needed", "Not needed")} }
        };
    }
    let found = files.iter().filter(|file| file.found == Some(true)).count();
    let unknown = files.iter().filter(|file| file.found.is_none()).count();
    let total = files.len();
    let missing_required = files
        .iter()
        .any(|file| file.required && file.found == Some(false));
    let status = if missing_required {
        "missing"
    } else if unknown > 0 {
        "unknown"
    } else {
        "checked"
    };
    let summary = if unknown == total {
        t!(note = "A BIOS file's state", "Not checked")
    } else if unknown > 0 {
        t!(
            note = "BIOS files found out of all the platform needs",
            "{found}/{total} found · {unknown} not checked",
            found = found,
            total = total,
            unknown = unknown
        )
    } else {
        t!(
            note = "BIOS files found out of all the platform needs",
            "{found}/{total} found",
            found = found,
            total = total
        )
    };
    rsx! {
        div { "data-status": status,
            p { class: if missing_required { "text-xs text-danger" } else { "text-xs text-fg-muted" }, "{summary}" }
            ul { class: "mt-1 space-y-1 font-mono text-[11px] text-fg-subtle",
                for file in files {
                    li { key: "{file.file}",
                        {
                            let need = if file.required {
                                t!(note = "A BIOS file", "Required")
                            } else {
                                t!(note = "A BIOS file", "Optional")
                            };
                            let state = if file.state == Some(BiosState::WrongHash) {
                                t!(note = "A BIOS file's state", "Wrong file")
                            } else {
                                match file.found {
                                    None => t!(note = "A BIOS file's state", "Not checked"),
                                    Some(true) => t!(note = "A BIOS file's state", "Found"),
                                    Some(false) => t!(note = "A BIOS file's state", "Missing"),
                                }
                            };
                            format!("{} · {need} · {state}", file.file)
                        }
                    }
                }
            }
        }
    }
}

/// The command line a profile resolves to, with a sample ROM.
#[component]
pub fn PreviewTokens(
    preview: Option<ArgvPreview>,
    #[props(default)] pending: bool,
    #[props(default)] error: Option<String>,
) -> Element {
    let failure = error.or_else(|| preview.as_ref().and_then(|preview| preview.error.clone()));
    rsx! {
        div { class: "rounded-control border border-border bg-surface-base/60 p-3",
            p { class: "mb-2 font-mono text-xs tracking-wide text-fg-subtle uppercase",
                {t!("Resolved command line")}
            }
            if pending {
                p { role: "status", class: "text-xs text-fg-muted", {t!("Updating preview…")} }
            }
            if let Some(preview) = preview {
                p { class: "mb-2 break-all text-xs text-fg-muted",
                    {t!("Sample ROM: {0}", 0 = preview.sample_rom.clone())}
                }
                ol { "aria-label": t!("Command line"), class: "flex flex-wrap gap-1.5 font-mono text-xs",
                    for (index, token) in std::iter::once(preview.program.clone()).chain(preview.args.iter().cloned()).enumerate() {
                        li {
                            key: "{index}",
                            "data-part": "token",
                            "data-program": (index == 0).then_some("true"),
                            class: "max-w-full break-all rounded-control border border-border bg-surface-overlay px-2 py-1 text-fg-muted data-program:border-accent-alt/40 data-program:text-accent-alt",
                            if token.is_empty() {
                                "\"\""
                            } else {
                                "{token}"
                            }
                        }
                    }
                }
                if let Some(cwd) = preview.cwd.clone() {
                    p { class: "mt-2 break-all font-mono text-xs text-fg-muted",
                        {t!("Working directory: {0}", 0 = cwd)}
                    }
                }
            }
            if let Some(failure) = failure {
                p { role: "alert", class: "mt-2 text-sm text-danger", "{failure}" }
            }
        }
    }
}

/// Rows of cards, each cell labelled with its column (the webview's
/// `StackedTable`, always in its narrow layout).
#[component]
pub fn StackedTable(
    #[props(default)] class: String,
    #[props(extends = GlobalAttributes)] attributes: Vec<Attribute>,
    children: Element,
) -> Element {
    rsx! {
        div { role: "list", class: "rounded-control border border-border {class}", ..attributes, {children} }
    }
}

#[component]
pub fn StackedRow(
    #[props(default)] class: String,
    #[props(extends = GlobalAttributes)] attributes: Vec<Attribute>,
    children: Element,
) -> Element {
    rsx! {
        div {
            role: "listitem",
            class: "grid grid-cols-2 gap-x-4 gap-y-3 border-t border-border p-4 first:border-t-0 {class}",
            ..attributes,
            {children}
        }
    }
}

#[component]
pub fn StackedCell(
    /// Its column's name.
    #[props(default)]
    label: Option<String>,
    /// Takes the row's whole width.
    #[props(default)]
    full: bool,
    #[props(default)] class: String,
    children: Element,
) -> Element {
    rsx! {
        div { class: if full { "col-span-full min-w-0 break-words {class}" } else { "min-w-0 break-words {class}" },
            if let Some(label) = label {
                div { "data-part": "column-label", class: "mb-1 text-xs text-fg-muted", "{label}" }
            }
            {children}
        }
    }
}

/// How an emulator is installed, for people.
pub fn install_kind_label(install: &caskade_core::emulation::EmulatorInstall) -> String {
    use caskade_core::emulation::EmulatorInstall as I;
    match install {
        I::Folder { .. } => t!("Folder"),
        I::Path { .. } => t!("$PATH binary"),
        I::AppBundle { .. } => t!("App bundle"),
        // Product names.
        I::Flatpak { .. } => "Flatpak".to_owned(),
        I::AppImage { .. } => "AppImage".to_owned(),
    }
}

/// Where an install is: its folder, binary, app id, file or bundle.
pub fn install_location(install: &caskade_core::emulation::EmulatorInstall) -> String {
    use caskade_core::emulation::EmulatorInstall as I;
    match install {
        I::Folder { dir, .. } => dir.to_string_lossy().into_owned(),
        I::Path { binary } => binary.clone(),
        I::Flatpak { app_id } => app_id.clone(),
        I::AppImage { file } => file.to_string_lossy().into_owned(),
        I::AppBundle { bundle } => bundle.to_string_lossy().into_owned(),
    }
}
