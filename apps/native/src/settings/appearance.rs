//! Settings → Appearance (#104), as the webview's `AppearanceSettings`,
//! `ThemePicker` and `CustomCssEditor`: the theme (every installed one with
//! its previews and problems), the look (density, interface scale, accent),
//! the library's cards, motion and effects, and the user's own CSS. A
//! change applies at once: `theme::follow` restyles the window when
//! `core.appearance` changes.

use caskade_core::themes::{ThemeInfo, ThemeProblem, ThemeSource};
use caskade_ui_i18n::{format, t, tp};
use caskade_ui_theme::user_css::{self, CssProblem, USER_CSS_MAX};
use dioxus_native::NodeHandle;
use dioxus_native::prelude::*;
use serde_json::Value;

use super::drafts::DraftSlider;
use super::reset::{ResetDefaults, ResetField, ResetPart};
use super::state::{SettingsFeedback, SettingsLoading, use_settings_section};
use super::sync::SectionSync;
use super::unsaved::{UnsavedForm, UnsavedHint, use_unsaved_changes};
use crate::data::{Query, change, use_themes_query};
use crate::router::{Route, use_router};
use crate::ui::icons::{
    ACTIVITY, BOOK_OPEN, CAPTIONS, CODE, CONTRAST, DROPLET, FOLDER_OPEN, IMAGE, LAYOUT_GRID,
    PALETTE, POWER_OFF, RECTANGLE_HORIZONTAL, REFRESH_CW, ROWS_3, SCALING, SPARKLES,
};
use crate::ui::{
    Button, ButtonSize, ButtonVariant, FailureNotice, Link, NoticeVariant, Segment,
    SegmentedControl, SettingRow, SettingsSection, Switch, TextField,
};

/// The default theme, when the setting is empty.
const DEFAULT_THEME: &str = "caskade.default";

/// The theming contract's documentation (`docs/theming/`).
const THEMING_DOCS: &str = "https://github.com/krazyjakee/caskade/blob/master/docs/theming/";

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn flag(value: &Value, key: &str, default: bool) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(default)
}

fn number(value: &Value, key: &str, default: f64) -> f64 {
    value.get(key).and_then(Value::as_f64).unwrap_or(default)
}

/// Accents a pad can choose (#61). The ids are the colours themselves, in
/// the setting's `#rrggbb` format.
fn swatches() -> [(&'static str, String); 8] {
    [
        ("#3b82f6", tp!("accent colour", "Blue")),
        ("#06b6d4", tp!("accent colour", "Cyan")),
        ("#22c55e", tp!("accent colour", "Green")),
        ("#f59e0b", tp!("accent colour", "Amber")),
        ("#f97316", tp!("accent colour", "Orange")),
        ("#ef4444", tp!("accent colour", "Red")),
        ("#ec4899", tp!("accent colour", "Pink")),
        ("#a855f7", tp!("accent colour", "Violet")),
    ]
}

#[component]
pub fn AppearanceSettings() -> Element {
    let section = use_settings_section("core.appearance");
    let Some(settings) = section.value() else {
        return rsx! {
            SettingsLoading { error: section.load_error(), on_reload: move |_| section.reload() }
        };
    };
    let set = move |key: &'static str, value: Value| section.set_field(key, value);
    let field = |key: &str, label: String| ResetField {
        key: key.to_owned(),
        label,
    };
    // The page shows the whole section, custom CSS included (#44).
    let reset_parts = vec![ResetPart {
        namespace: "core.appearance".into(),
        whole: true,
        fields: vec![
            field("theme", t!("Theme")),
            field("density", t!("Density")),
            field("uiScale", t!("Interface scale")),
            field("accent", t!("Accent colour")),
            field("cardSize", t!("Card size")),
            field("showArtTitles", t!("Show titles on artwork")),
            field("motion", t!("Animations")),
            field("reduceEffects", t!("Reduce effects")),
            field("customCss", t!("Custom CSS")),
            field("customCssEnabled", t!("Disable custom CSS")),
        ],
    }];
    let theme = Some(text(&settings, "theme"))
        .filter(|theme| !theme.is_empty())
        .unwrap_or_else(|| DEFAULT_THEME.to_owned());
    let densities = vec![
        Segment {
            icon: Some(ROWS_3),
            ..Segment::new("compact", tp!("density", "Compact"))
        },
        Segment {
            icon: Some(LAYOUT_GRID),
            ..Segment::new("comfortable", tp!("density", "Comfortable"))
        },
    ];
    let motions = vec![
        Segment::new(
            "system",
            tp!(
                note = "Follow the operating system's reduced-motion setting",
                "animations setting",
                "System"
            ),
        ),
        Segment::new("full", tp!("animations setting", "Full")),
        Segment::new("reduced", tp!("animations setting", "Reduced")),
    ];
    let accent = settings
        .get("accent")
        .and_then(Value::as_str)
        .filter(|accent| !accent.is_empty())
        .map(str::to_owned);

    // Two columns only where the panel has room beside both navigation
    // columns, as General.
    rsx! {
        div { "data-settings": "appearance",
            div { class: "grid items-start gap-4 min-[102rem]:grid-cols-2",
                div { class: "col-span-full flex items-center justify-between gap-3",
                    SettingsFeedback { error: section.error(), saving: section.saving() }
                    ResetDefaults {
                        group: tp!("settings tab", "Appearance"),
                        label: t!("Reset appearance settings"),
                        parts: reset_parts,
                        disabled: section.saving(),
                    }
                }
                SettingsSection {
                    icon: Some(PALETTE),
                    title: t!("Theme"),
                    description: t!("How Caskade looks. Themes restyle every view through design tokens."),
                    class: "col-span-full",
                    ThemePicker {
                        value: theme,
                        on_change: move |next: String| set("theme", Value::String(next)),
                    }
                    MoreThemes {}
                }
                SettingsSection {
                    icon: Some(SPARKLES),
                    title: tp!(note = "The appearance", "settings section", "Look"),
                    description: t!("Adjust the theme to taste."),
                    SettingRow { icon: Some(LAYOUT_GRID), label: t!("Density"),
                        SegmentedControl {
                            options: densities,
                            value: text(&settings, "density"),
                            on_change: move |next: String| set("density", Value::String(next)),
                        }
                    }
                    SettingRow { icon: Some(SCALING), label: t!("Interface scale"),
                        DraftSlider {
                            value: number(&settings, "uiScale", 100.0),
                            min: 80.0,
                            max: 150.0,
                            step: 5.0,
                            format: Callback::new(|value: f64| format!("{value}%")),
                            commit: move |next: f64| set("uiScale", Value::from(next as i64)),
                            class: "w-56",
                        }
                    }
                    SettingRow {
                        icon: Some(DROPLET),
                        label: t!("Accent colour"),
                        description: t!("Buttons, selection and glows; kept readable on the theme."),
                        AccentPicker {
                            value: accent,
                            on_change: move |next: Option<String>| {
                                set("accent", next.map(Value::String).unwrap_or(Value::Null))
                            },
                        }
                    }
                    // The interface scale suits each screen and always stays
                    // here.
                    SectionSync { namespace: "core.appearance" }
                }
                SettingsSection {
                    icon: Some(IMAGE),
                    title: t!("Library"),
                    description: t!("How games are shown in grids."),
                    SettingRow {
                        icon: Some(RECTANGLE_HORIZONTAL),
                        label: t!("Card size"),
                        description: t!("Minimum width of a game card."),
                        DraftSlider {
                            value: number(&settings, "cardSize", 176.0),
                            min: 128.0,
                            max: 288.0,
                            step: 8.0,
                            format: Callback::new(|value: f64| format!("{value}px")),
                            commit: move |next: f64| set("cardSize", Value::from(next as i64)),
                            class: "w-56",
                        }
                    }
                    SettingRow {
                        icon: Some(CAPTIONS),
                        label: t!("Show titles on artwork"),
                        description: t!("Overlay the game title on cover art."),
                        Switch {
                            checked: flag(&settings, "showArtTitles", true),
                            on_change: move |next: bool| set("showArtTitles", Value::Bool(next)),
                        }
                    }
                }
                SettingsSection {
                    icon: Some(ACTIVITY),
                    title: t!("Motion & Effects"),
                    description: t!("Turn these down on slower machines."),
                    SettingRow {
                        icon: Some(ACTIVITY),
                        label: t!("Animations"),
                        description: t!("System follows your OS reduced-motion setting."),
                        SegmentedControl {
                            options: motions,
                            value: text(&settings, "motion"),
                            on_change: move |next: String| set("motion", Value::String(next)),
                        }
                    }
                    SettingRow {
                        icon: Some(CONTRAST),
                        label: t!("Reduce effects"),
                        description: t!("Turn off blur, glow, scanlines and the backdrop grid."),
                        Switch {
                            checked: flag(&settings, "reduceEffects", false),
                            on_change: move |next: bool| set("reduceEffects", Value::Bool(next)),
                        }
                    }
                }
                SettingsSection {
                    icon: Some(CODE),
                    title: t!("Custom CSS"),
                    description: t!("Your own rules, applied over the theme without a reload."),
                    class: "col-span-full",
                    CustomCssEditor {
                        css: text(&settings, "customCss"),
                        enabled: flag(&settings, "customCssEnabled", true),
                        saving: section.saving(),
                        on_save: move |css: String| set("customCss", Value::String(css)),
                        on_enabled_change: move |on: bool| set("customCssEnabled", Value::Bool(on)),
                    }
                }
            }
        }
    }
}

/// Links to the catalogue's themes.
#[component]
fn MoreThemes() -> Element {
    let router = use_router();
    let href = Route::Settings {
        tab: "plugins",
        section: None,
    }
    .href();
    let to = href.clone();
    rsx! {
        Link {
            href,
            "data-part": "more-themes",
            class: "mt-3 inline-block text-sm text-accent-alt",
            on_press: move |_| router.navigate(&to),
            {t!("More themes in the plugin and theme catalogue")}
        }
    }
}

// ── Theme ──

/// A theme's previews, as the window loads them: `caskade-theme:` URLs.
pub fn preview_url(theme: &str, path: &str) -> String {
    // What a URL path segment can't hold as it is.
    const SEGMENT: &percent_encoding::AsciiSet = &percent_encoding::CONTROLS
        .add(b' ')
        .add(b'"')
        .add(b'#')
        .add(b'%')
        .add(b'/')
        .add(b'<')
        .add(b'>')
        .add(b'?')
        .add(b'`')
        .add(b'{')
        .add(b'}');
    let encode = |text: &str| percent_encoding::utf8_percent_encode(text, SEGMENT).to_string();
    let path = path.split('/').map(encode).collect::<Vec<_>>().join("/");
    format!("caskade-theme://localhost/{}/{path}", encode(theme))
}

fn problem_text(problems: &[ThemeProblem]) -> String {
    problems
        .iter()
        .map(|problem| {
            let mut at = vec![problem.file.clone()];
            at.extend(problem.line.map(|line| line.to_string()));
            at.extend(problem.column.map(|column| column.to_string()));
            format!("{}: {}", at.join(":"), problem.message)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn source_label(source: ThemeSource) -> Option<String> {
    match source {
        ThemeSource::Builtin => None,
        ThemeSource::User => Some(tp!("theme source", "Installed")),
        ThemeSource::Plugin => Some(tp!("theme source", "From a plugin")),
        ThemeSource::Dev => Some(tp!("theme source", "Dev folder")),
    }
}

/// Appearance → Theme (#140): every installed theme with its name, author
/// and previews; one that can't be used shows why and can't be picked.
/// A radio group: one tab stop, arrows move and pick, as the webview's.
#[component]
pub fn ThemePicker(value: String, on_change: EventHandler<String>) -> Element {
    let mut themes: Query<Vec<ThemeInfo>> =
        use_themes_query(|themes| async move { themes.list().await });
    let active = use_themes_query(|themes| async move { themes.active().await });
    let mut rescan_error = use_signal(|| None);
    let mut rescanning = use_signal(|| false);
    let handle = crate::data::try_use_themes();
    let rescan = use_callback(move |()| {
        let Some(handle) = handle.clone() else {
            return;
        };
        rescanning.set(true);
        rescan_error.set(None);
        spawn(async move {
            let outcome = change(async move { handle.reload().await }).await;
            rescanning.set(false);
            match outcome {
                Ok(_) => themes.restart(),
                Err(error) => rescan_error.set(Some(error)),
            }
        });
    });
    let notice = match &*active.read() {
        Some(Ok(active)) if active.safe_mode || active.requested == value => active.notice.clone(),
        _ => None,
    };
    let list = themes.read().clone();
    // The usable themes in order: where arrows go.
    let usable: Vec<String> = match &list {
        Some(Ok(list)) => list
            .iter()
            .filter(|theme| theme.problems.is_empty())
            .map(|theme| theme.id.clone())
            .collect(),
        _ => Vec::new(),
    };
    let stop = Some(value.clone())
        .filter(|value| usable.contains(value))
        .or_else(|| usable.first().cloned());
    rsx! {
        div { "data-part": "theme-picker", class: "flex flex-col gap-3 py-2",
            if let Some(notice) = notice {
                p {
                    role: "status",
                    class: "rounded-control border border-warning/40 bg-warning/10 px-3 py-2 text-sm",
                    "{notice}"
                }
            }
            match list {
                None => rsx! {
                    p { role: "status", class: "text-sm text-fg-muted", {t!("Loading…")} }
                },
                Some(Err(error)) => rsx! {
                    FailureNotice {
                        error,
                        variant: NoticeVariant::Inline,
                        title: t!("Themes couldn't be loaded"),
                        on_retry: move |_| themes.restart(),
                    }
                },
                Some(Ok(list)) if list.is_empty() => rsx! {
                    p { class: "text-sm text-fg-muted", {t!("No themes found. Rescan themes looks for them again.")} }
                },
                Some(Ok(list)) => rsx! {
                    div {
                        role: "radiogroup",
                        "aria-label": t!("Theme"),
                        class: "grid gap-3 sm:grid-cols-2 2xl:grid-cols-3",
                        for (index, theme) in list.into_iter().enumerate() {
                            ThemeOption {
                                key: "{theme.id}:{index}",
                                index,
                                checked: theme.problems.is_empty() && theme.id == value,
                                stop: theme.problems.is_empty() && Some(&theme.id) == stop.as_ref(),
                                previous: neighbour(&usable, &theme.id, false),
                                next: neighbour(&usable, &theme.id, true),
                                theme,
                                on_check: on_change,
                            }
                        }
                    }
                },
            }
            if let Some(error) = rescan_error() {
                FailureNotice {
                    error,
                    variant: NoticeVariant::Inline,
                    on_retry: move |_| rescan(()),
                }
            }
            div { class: "flex flex-wrap items-center justify-end gap-2",
                Button {
                    variant: ButtonVariant::Ghost,
                    size: ButtonSize::Sm,
                    icon: Some(REFRESH_CW),
                    disabled: rescanning(),
                    on_press: move |_| rescan(()),
                    {t!("Rescan themes")}
                }
                LoadThemeFolder { on_loaded: on_change, themes }
            }
        }
    }
}

/// The usable theme before or after `id`, wrapping.
fn neighbour(usable: &[String], id: &str, forward: bool) -> Option<String> {
    let at = usable.iter().position(|known| known == id)?;
    let next = if forward {
        (at + 1) % usable.len()
    } else {
        (at + usable.len() - 1) % usable.len()
    };
    Some(usable[next].clone())
}

/// One theme: a radio card.
#[component]
fn ThemeOption(
    theme: ThemeInfo,
    index: usize,
    checked: bool,
    stop: bool,
    previous: Option<String>,
    next: Option<String>,
    on_check: EventHandler<String>,
) -> Element {
    let interaction = crate::ui::interact::use_interaction_where(stop);
    let label_id = format!("theme-option-{index}");
    let broken = !theme.problems.is_empty();
    let id = theme.id.clone();
    let check = use_callback(move |()| {
        if !broken {
            on_check.call(id.clone());
        }
    });
    let mut node = use_signal(|| None::<NodeHandle>);
    // Arrows check the next usable theme and move focus there.
    let go = move |to: Option<String>| {
        if let Some(to) = to {
            on_check.call(to.clone());
            if let Some(handle) = node.peek().clone() {
                let doc = handle.doc();
                let mut found = None;
                doc.visit(|id, node| {
                    if node.attr(blitz_dom::LocalName::from("data-theme")) == Some(to.as_str()) {
                        found = Some(id);
                    }
                });
                drop(doc);
                if let Some(found) = found {
                    handle.doc_mut().set_focus_to(found);
                }
            }
        }
    };
    let source = source_label(theme.source);
    let problems = problem_text(&theme.problems);
    let warnings = problem_text(&theme.warnings);
    let previews: Vec<String> = theme
        .previews
        .iter()
        .map(|preview| preview_url(&theme.id, preview))
        .collect();
    rsx! {
        div {
            role: "radio",
            tabindex: if stop { "0" } else { "-1" },
            "aria-checked": if checked { "true" } else { "false" },
            "aria-disabled": crate::ui::interact::flag(broken),
            "aria-labelledby": "{label_id}",
            "data-part": "theme",
            "data-theme": "{theme.id}",
            "data-selected": crate::ui::interact::flag(checked),
            "data-disabled": crate::ui::interact::flag(broken),
            "data-hovered": interaction.hovered(),
            "data-focus-visible": interaction.ring(),
            class: "flex cursor-pointer flex-col gap-2 rounded-card border border-border bg-surface-overlay/60 p-3 transition-colors duration-(--cs-motion-duration-fast) data-disabled:cursor-not-allowed data-focus-visible:focus-ring data-hovered:border-border-strong data-selected:border-accent data-selected:bg-surface-hover",
            onmounted: move |event| node.set(event.data().downcast::<NodeHandle>().cloned()),
            onfocus: move |_| interaction.focus(),
            onblur: move |_| interaction.blur(),
            onmouseenter: move |_| interaction.enter(),
            onmouseleave: move |_| interaction.leave(),
            onclick: move |_| check(()),
            onkeydown: {
                let previous = previous.clone();
                let next = next.clone();
                move |event: KeyboardEvent| {
                    use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
                    match event.key() {
                        Key::Named(NamedKey::ArrowDown | NamedKey::ArrowRight) => {
                            event.prevent_default();
                            go(next.clone());
                        }
                        Key::Named(NamedKey::ArrowUp | NamedKey::ArrowLeft) => {
                            event.prevent_default();
                            go(previous.clone());
                        }
                        _ => {
                            interaction.key_down(
                                &event,
                                crate::ui::interact::Keys::EnterAndSpace,
                                || check(()),
                            );
                        }
                    }
                }
            },
            onkeyup: move |event| interaction.key_up(&event, || check(())),
            if previews.is_empty() {
                div {
                    "aria-hidden": "true",
                    class: "aspect-video w-full rounded-control border border-border bg-surface-base",
                }
            } else {
                div { class: "grid auto-cols-fr grid-flow-col gap-1",
                    // Each image fills a sized box, as GameArt's: an <img>
                    // sized by its aspect ratio in a grid track lays out as
                    // NaN in Blitz while it loads.
                    for preview in previews {
                        div {
                            key: "{preview}",
                            class: "relative aspect-video w-full overflow-hidden rounded-control border border-border bg-surface-base",
                            img {
                                src: "{preview}",
                                alt: "",
                                class: "absolute inset-0 size-full object-cover",
                            }
                        }
                    }
                }
            }
            span { class: "flex items-baseline justify-between gap-2",
                span { id: "{label_id}", class: "truncate font-display font-semibold tracking-wide",
                    "{theme.name}"
                }
                if !theme.version.is_empty() {
                    span { class: "shrink-0 font-mono text-xs text-fg-muted", "{theme.version}" }
                }
            }
            if let Some(author) = &theme.author {
                span { class: "text-xs text-fg-muted",
                    {t!(note = "A theme's author", "by {0}", 0 = author.clone())}
                }
            }
            if let Some(description) = &theme.description {
                span { class: "text-xs text-fg-muted", "{description}" }
            }
            if let Some(source) = source {
                span { class: "font-mono text-[0.7rem] tracking-wider text-accent-alt uppercase",
                    "{source}"
                }
            }
            if broken {
                span { class: "text-xs whitespace-pre-line text-danger",
                    {t!("Can't be used:")}
                    "\n{problems}"
                }
            } else if !warnings.is_empty() {
                span { class: "text-xs whitespace-pre-line text-warning", "{warnings}" }
            }
        }
    }
}

/// Development builds: loads a theme folder in place, chosen in the app's
/// folder browser.
#[component]
fn LoadThemeFolder(on_loaded: EventHandler<String>, themes: Query<Vec<ThemeInfo>>) -> Element {
    let handle = crate::data::try_use_themes();
    let picker = crate::flows::paths::use_path_picker();
    let mut error = use_signal(|| None);
    let mut loading = use_signal(|| false);
    let Some(handle) = handle.filter(|handle| handle.loads_unpacked()) else {
        return rsx! {};
    };
    let load = move |_| {
        let handle = handle.clone();
        error.set(None);
        spawn(async move {
            let request = crate::flows::paths::PathRequest::new(
                crate::flows::paths::PickKind::Folder,
                t!("Theme folder"),
            );
            let Some(folder) = picker.pick(request).await else {
                return;
            };
            loading.set(true);
            let outcome = change(async move { handle.load_unpacked(&folder).await }).await;
            loading.set(false);
            match outcome {
                Ok(id) => {
                    let mut themes = themes;
                    themes.restart();
                    on_loaded.call(id);
                }
                Err(failed) => error.set(Some(failed)),
            }
        });
    };
    rsx! {
        Button {
            variant: ButtonVariant::Ghost,
            size: ButtonSize::Sm,
            icon: Some(FOLDER_OPEN),
            disabled: loading(),
            on_press: load,
            {t!("Load theme folder…")}
        }
        if let Some(failed) = error() {
            FailureNotice { error: failed, variant: NoticeVariant::Inline }
        }
    }
}

// ── Accent ──

/// What the accent picker says about a pick: a note, or why it was refused.
#[derive(Clone, PartialEq)]
struct Note {
    text: String,
    refused: bool,
}

/// The theme's surfaces as the window draws them, for the accent's
/// contrast check.
fn surfaces(handle: &NodeHandle) -> Vec<caskade_ui_theme::accent::Rgb> {
    let doc = handle.doc();
    let Some(root) = doc.try_root_element().map(|root| root.id) else {
        return Vec::new();
    };
    caskade_ui_theme::appearance::surfaces(
        doc.resolved_style_value(root, "--cs-color-surface-base")
            .trim(),
        doc.resolved_style_value(root, "--cs-color-surface-raised")
            .trim(),
    )
}

/// The theme's accent unless the user picked one; a pick that would be
/// hard to see on this theme's surfaces is adjusted (with a note) or
/// refused. Swatches for a pad and a finger; a `#rrggbb` field for any
/// other colour (Blitz has no colour dialog, and a field works with the
/// on-screen keyboard too).
#[component]
fn AccentPicker(value: Option<String>, on_change: EventHandler<Option<String>>) -> Element {
    let mut note = use_signal(|| None::<Note>);
    let mut node = use_signal(|| None::<NodeHandle>);
    let derive = move |colour: &str| {
        let surfaces = node.peek().as_ref().map(surfaces).unwrap_or_default();
        caskade_ui_theme::accent::derive(colour, &surfaces)
    };
    let mut pick = move |colour: String| match derive(&colour) {
        Err(message) => note.set(Some(Note {
            text: message,
            refused: true,
        })),
        Ok(accent) => {
            note.set(accent.message.clone().map(|text| Note {
                text,
                refused: false,
            }));
            on_change.call(Some(accent.accent.clone()));
        }
    };
    // A swatch adjusted for this theme is saved as its adjusted colour.
    let chosen = value.as_ref().and_then(|value| {
        swatches()
            .into_iter()
            .find(|(id, _)| derive(id).is_ok_and(|accent| &accent.accent == value))
            .map(|(id, _)| id)
    });
    // What's typed in the colour field, until Enter.
    let mut typed = use_signal(|| None::<String>);
    let shown = typed().unwrap_or_else(|| value.clone().unwrap_or_default());
    rsx! {
        div {
            "data-part": "accent",
            class: "flex flex-col items-end gap-1.5",
            onmounted: move |event| node.set(event.data().downcast::<NodeHandle>().cloned()),
            div {
                role: "radiogroup",
                "aria-label": t!("Accent colour"),
                "data-part": "accent-swatches",
                class: "flex flex-wrap justify-end gap-1.5",
                for (id, label) in swatches() {
                    Swatch {
                        key: "{id}",
                        colour: id,
                        label,
                        selected: chosen == Some(id),
                        on_pick: move |colour: String| pick(colour),
                    }
                }
            }
            div { class: "flex items-center gap-2",
                div { class: "w-32",
                    TextField {
                        value: shown,
                        mono: true,
                        placeholder: "#5b5ff7".to_owned(),
                        "aria-label": t!("Custom accent colour"),
                        on_input: move |next: String| typed.set(Some(next)),
                        on_submit: move |next: String| {
                            typed.set(None);
                            let next = next.trim().to_owned();
                            if !next.is_empty() {
                                pick(next);
                            }
                        },
                    }
                }
                Button {
                    variant: ButtonVariant::Ghost,
                    size: ButtonSize::Sm,
                    disabled: value.is_none(),
                    on_press: move |_| {
                        note.set(None);
                        on_change.call(None);
                    },
                    {t!("Theme accent")}
                }
            }
            if let Some(note) = note() {
                p {
                    role: if note.refused { "alert" } else { "status" },
                    class: if note.refused { "max-w-64 text-right text-xs text-danger" } else { "max-w-64 text-right text-xs text-fg-muted" },
                    "{note.text}"
                }
            }
        }
    }
}

/// One accent swatch: a radio in its colour, pressed by click, Enter,
/// Space or a pad's A.
#[component]
fn Swatch(
    colour: &'static str,
    label: String,
    selected: bool,
    on_pick: EventHandler<String>,
) -> Element {
    let interaction = crate::ui::interact::use_interaction();
    let press = move || on_pick.call(colour.to_owned());
    rsx! {
        button {
            r#type: "button",
            role: "radio",
            "data-part": "accent-swatch",
            "aria-label": "{label}",
            "aria-checked": if selected { "true" } else { "false" },
            "data-selected": crate::ui::interact::flag(selected),
            "data-hovered": interaction.hovered(),
            "data-focus-visible": interaction.ring(),
            style: "background-color: {colour}",
            class: "size-8 rounded-control border border-border-strong outline-offset-2 data-focus-visible:focus-ring data-hovered:brightness-115 data-selected:outline-2 data-selected:outline-fg",
            onmouseenter: move |_| interaction.enter(),
            onmouseleave: move |_| interaction.leave(),
            onfocus: move |_| interaction.focus(),
            onblur: move |_| interaction.blur(),
            onclick: move |_| press(),
            onkeydown: move |event| {
                interaction.key_down(&event, crate::ui::interact::Keys::EnterAndSpace, press);
            },
            onkeyup: move |event| interaction.key_up(&event, press),
        }
    }
}

// ── Custom CSS ──

fn css_problems(problems: &[CssProblem]) -> String {
    user_css::problem_text(problems)
}

/// Appearance → Custom CSS (#142): the user's rules in the `cs.user` layer,
/// saved through the settings and applied without a reload. A switch turns
/// them off without losing them, for a look that broke. The CSS waits for
/// Save CSS (#43): leaving the section asks first.
#[component]
fn CustomCssEditor(
    css: String,
    enabled: bool,
    saving: bool,
    on_save: EventHandler<String>,
    on_enabled_change: EventHandler<bool>,
) -> Element {
    let mut draft = use_signal(|| None::<String>);
    let mut problems = use_signal(Vec::<CssProblem>::new);
    let safe_mode = caskade_app::commands::themes::safe_mode(None);
    let saved = css.clone();
    // Once the saved value is what was typed, the draft is done.
    use_effect(use_reactive!(|saved| {
        if draft.peek().as_deref() == Some(saved.as_str()) {
            draft.set(None);
        }
    }));
    let text = draft().unwrap_or_else(|| css.clone());
    let too_long = text.chars().count() > USER_CSS_MAX;
    let dirty = draft().is_some_and(|draft| draft != css);
    let lines = text.split('\n').count();
    let save = use_callback(move |()| {
        let text = draft.peek().clone().unwrap_or_default();
        let found = user_css::check(&text);
        let refused = !found.is_empty() || text.chars().count() > USER_CSS_MAX;
        problems.set(found);
        if !refused {
            on_save.call(text);
        }
        !refused
    });
    let revert = use_callback(move |()| {
        draft.set(None);
        problems.set(Vec::new());
    });
    use_unsaved_changes(
        dirty,
        UnsavedForm {
            label: t!("Custom CSS"),
            save: std::rc::Rc::new(move || Box::pin(async move { save(()) })),
            discard: std::rc::Rc::new(move || revert(())),
        },
    );
    let count = t!(
        "{length} / {max} characters",
        length = format::count(text.chars().count() as f64),
        max = format::count(USER_CSS_MAX as f64)
    );
    let docs = [
        (
            format!("{THEMING_DOCS}anatomy.md"),
            tp!("theming docs", "Selectors"),
        ),
        (
            format!("{THEMING_DOCS}tokens.md"),
            tp!("theming docs", "Tokens"),
        ),
        (format!("{THEMING_DOCS}README.md"), t!("Theming guide")),
    ];
    rsx! {
        div { "data-part": "custom-css", class: "flex flex-col gap-3",
            SettingRow {
                icon: Some(POWER_OFF),
                label: t!("Disable custom CSS"),
                description: t!("Keeps your CSS but stops applying it, to recover from a broken look."),
                Switch {
                    checked: !enabled,
                    on_change: move |off: bool| on_enabled_change.call(!off),
                }
            }
            if safe_mode {
                p {
                    role: "status",
                    class: "rounded-control border border-warning/40 bg-warning/10 px-3 py-2 text-sm",
                    {t!("Safe mode is on: your custom CSS isn't applied until Caskade starts normally.")}
                }
            }
            div { class: "flex overflow-hidden rounded-control border border-border-strong bg-surface-base/60 font-mono text-xs leading-5 focus-within:border-accent-alt/60",
                div {
                    "aria-hidden": "true",
                    class: "shrink-0 overflow-hidden border-r border-border py-2 pr-2 pl-3 text-right text-fg-subtle select-none",
                    for number in 1..=lines {
                        div { key: "{number}", "{number}" }
                    }
                }
                textarea {
                    "aria-label": t!("Custom CSS"),
                    "data-part": "css",
                    "data-spatial": "focusable",
                    spellcheck: "false",
                    wrap: "off",
                    rows: "14",
                    placeholder: "[data-cs=\"game-card\"] [data-part=\"title\"] {{\n  color: var(--cs-color-accent);\n}}",
                    value: "{text}",
                    class: "block h-72 w-full flex-1 resize-y bg-transparent py-2 pr-3 pl-2 text-fg outline-none placeholder:text-fg-subtle",
                    oninput: move |event| draft.set(Some(event.value())),
                }
            }
            div { class: "flex flex-wrap items-center gap-2",
                span {
                    class: if too_long { "mr-auto font-mono text-xs text-danger" } else { "mr-auto font-mono text-xs text-fg-subtle" },
                    "{count}"
                }
                UnsavedHint { dirty }
                Button {
                    variant: ButtonVariant::Ghost,
                    size: ButtonSize::Sm,
                    disabled: draft().is_none(),
                    on_press: move |_| revert(()),
                    {t!("Revert")}
                }
                Button {
                    variant: ButtonVariant::Primary,
                    size: ButtonSize::Sm,
                    disabled: draft().is_none() || saving || too_long,
                    on_press: move |_| {
                        save(());
                    },
                    {t!("Save CSS")}
                }
            }
            if !problems().is_empty() {
                div { role: "alert", class: "text-xs text-danger",
                    p { class: "font-medium", {t!("Custom CSS wasn't saved:")} }
                    p { class: "font-mono whitespace-pre-line", {css_problems(&problems())} }
                }
            }
            p { class: "flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-fg-muted",
                crate::ui::Svg { icon: BOOK_OPEN, class: "size-3.5" }
                {t!("Your rules beat the theme and every component. Style through the contract:")}
                for (href, label) in docs {
                    Link {
                        key: "{href}",
                        href: href.clone(),
                        class: "rounded-sm text-accent-alt underline-offset-2 hover:underline data-focus-visible:focus-ring",
                        on_press: move |_| {
                            if let Ok(url) = url::Url::parse(&href) {
                                caskade_app::navigation::open_in_browser(&url);
                            }
                        },
                        "{label}"
                    }
                }
            }
        }
    }
}
