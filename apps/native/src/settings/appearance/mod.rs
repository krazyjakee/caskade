//! Settings → Appearance (#104): the theme (every installed one with its
//! previews and problems), the look (density, interface scale, accent),
//! the library's cards, motion and effects, and the user's own CSS. A
//! change applies at once: `theme::follow` restyles the window when
//! `core.appearance` changes.

mod accent;
mod custom_css;
mod themes;

use caskade_theme::DEFAULT_THEME;
use caskade_ui_i18n::{t, tp};
use dioxus_native::prelude::*;
use serde_json::Value;

use self::accent::AccentPicker;
use self::custom_css::CustomCssEditor;
pub use self::themes::ThemePicker;
#[cfg(test)]
pub use self::themes::preview_url;
use super::drafts::DraftSlider;
use super::reset::{ResetDefaults, ResetField, ResetPart};
use super::state::{SettingsFeedback, SettingsLoading, use_settings_section};
use super::sync::SectionSync;
use crate::router::{Route, use_router};
use crate::ui::icons::{
    ACTIVITY, CAPTIONS, CODE, CONTRAST, DROPLET, IMAGE, LAYOUT_GRID, PALETTE, RECTANGLE_HORIZONTAL,
    ROWS_3, SCALING, SPARKLES,
};
use crate::ui::{Link, Segment, SegmentedControl, SettingRow, SettingsSection, Switch};

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
