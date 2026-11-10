//! Appearance settings (#141) as the root element's attributes and tokens,
//! as the webview's `appearance/apply.ts` and `themes/apply.ts` set them on
//! `<html>`: each choice is a token set the stylesheet selects by an
//! attribute (`data-density`, `data-motion`, `data-effects`,
//! `data-color-scheme`) or an inline token (`--cs-ui-scale`, the accent's).

use caskade_core::settings::AppearanceSettings;

use crate::accent::{self, Accent, Rgb};

/// What the app knows besides the settings.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Context {
    /// The applied theme's colour schemes, its default first.
    pub modes: Vec<String>,
    /// The OS asks for a light look.
    pub prefers_light: bool,
    /// The OS asks for less motion. The webview leaves this to
    /// `@media (prefers-reduced-motion)`, which Blitz doesn't evaluate, so
    /// the native app reads it from the OS and sets `data-motion` instead.
    pub prefers_reduced_motion: bool,
}

/// The scheme to show: the OS preference when the theme has it, otherwise
/// the theme's default (its first mode).
pub fn color_scheme(modes: &[String], prefers_light: bool) -> &'static str {
    let wanted = if prefers_light { "light" } else { "dark" };
    let mut schemes = modes
        .iter()
        .filter_map(|mode| match mode.as_str() {
            "dark" => Some("dark"),
            "light" => Some("light"),
            _ => None,
        })
        .peekable();
    let first = schemes.peek().copied();
    if schemes.any(|scheme| scheme == wanted) {
        wanted
    } else {
        first.unwrap_or("dark")
    }
}

/// The root element's attributes: each `Some` sets it, `None` removes it.
pub type Attributes = Vec<(&'static str, Option<String>)>;

/// The attributes for `prefs` in `context`.
pub fn attributes(prefs: &AppearanceSettings, context: &Context) -> Attributes {
    let motion = match prefs.motion.as_str() {
        "system" if context.prefers_reduced_motion => "reduced",
        "full" | "reduced" | "system" => prefs.motion.as_str(),
        _ => "system",
    };
    vec![
        (
            "data-color-scheme",
            Some(color_scheme(&context.modes, context.prefers_light).to_owned()),
        ),
        ("data-density", Some(prefs.density.clone())),
        ("data-motion", Some(motion.to_owned())),
        (
            "data-effects",
            prefs.reduce_effects.then(|| "reduced".to_owned()),
        ),
    ]
}

/// The UI scale token, clamped to 80–150 % as the webview does.
pub fn ui_scale(prefs: &AppearanceSettings) -> String {
    let scale = f64::from(prefs.ui_scale.clamp(80, 150)) / 100.0;
    format!("{scale}")
}

/// The theme's opaque surfaces from its computed `--cs-color-surface-base`
/// and `-raised` (a translucent raised surface is composited over the
/// base), as the webview's `themeSurfaces`.
pub fn surfaces(base: &str, raised: &str) -> Vec<Rgb> {
    let Some(base) = accent::parse_surface(base, [0.0; 3]) else {
        return Vec::new();
    };
    match accent::parse_surface(raised, base) {
        Some(raised) => vec![base, raised],
        None => vec![base],
    }
}

/// The root's inline style: the UI scale and, when one is set and
/// readable, the accent's tokens. The accent's outcome comes back for the
/// Appearance panel (and the log) when one is set.
pub fn inline_style(
    prefs: &AppearanceSettings,
    surfaces: &[Rgb],
) -> (String, Option<Result<Accent, String>>) {
    let mut style = format!("--cs-ui-scale: {};", ui_scale(prefs));
    let accent = prefs
        .accent
        .as_deref()
        .filter(|accent| !accent.is_empty())
        .map(|chosen| accent::derive(chosen, surfaces));
    if let Some(Ok(accent)) = &accent {
        for (token, value) in accent.tokens() {
            style.push_str(&format!(" {token}: {value};"));
        }
    }
    (style, accent)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modes(list: &[&str]) -> Vec<String> {
        list.iter().map(|mode| (*mode).to_owned()).collect()
    }

    #[test]
    fn the_scheme_follows_the_os_when_the_theme_has_it() {
        assert_eq!(color_scheme(&modes(&["dark", "light"]), true), "light");
        assert_eq!(color_scheme(&modes(&["dark", "light"]), false), "dark");
        assert_eq!(color_scheme(&modes(&["light"]), false), "light");
        assert_eq!(color_scheme(&modes(&["dark"]), true), "dark");
        assert_eq!(color_scheme(&[], true), "dark");
    }

    #[test]
    fn the_settings_become_the_webviews_attributes() {
        let mut prefs = AppearanceSettings::default();
        let context = Context {
            modes: modes(&["dark"]),
            ..Context::default()
        };
        let get = |attributes: &Attributes, name: &str| {
            attributes
                .iter()
                .find(|(attribute, _)| *attribute == name)
                .and_then(|(_, value)| value.clone())
        };
        let defaults = attributes(&prefs, &context);
        assert_eq!(get(&defaults, "data-density"), Some(prefs.density.clone()));
        assert_eq!(get(&defaults, "data-motion").as_deref(), Some("system"));
        assert_eq!(get(&defaults, "data-effects"), None);
        assert_eq!(get(&defaults, "data-color-scheme").as_deref(), Some("dark"));
        prefs.density = "compact".into();
        prefs.reduce_effects = true;
        let changed = attributes(&prefs, &context);
        assert_eq!(get(&changed, "data-density").as_deref(), Some("compact"));
        assert_eq!(get(&changed, "data-effects").as_deref(), Some("reduced"));
        // The OS's reduced motion applies to "system" only.
        let reduced = Context {
            prefers_reduced_motion: true,
            ..context.clone()
        };
        assert_eq!(
            get(&attributes(&prefs, &reduced), "data-motion").as_deref(),
            Some("reduced")
        );
        prefs.motion = "full".into();
        assert_eq!(
            get(&attributes(&prefs, &reduced), "data-motion").as_deref(),
            Some("full")
        );
    }

    #[test]
    fn the_inline_style_carries_the_scale_and_a_readable_accent() {
        let mut prefs = AppearanceSettings {
            ui_scale: 200,
            ..AppearanceSettings::default()
        };
        let dark = surfaces("#060914", "#0d1224d9");
        assert_eq!(dark.len(), 2);
        assert_eq!(
            inline_style(&prefs, &dark),
            ("--cs-ui-scale: 1.5;".into(), None)
        );
        prefs.ui_scale = 110;
        prefs.accent = Some("#5b5ff7".into());
        let (style, accent) = inline_style(&prefs, &dark);
        assert!(accent.unwrap().is_ok());
        assert_eq!(
            style,
            "--cs-ui-scale: 1.1; --cs-color-accent: #5b5ff7; --cs-color-accent-strong: #7d6dff; \
             --cs-color-accent-fg: #ffffff; --cs-color-glow: #656cff80;"
        );
        // Refused: the theme's own accent stays.
        prefs.accent = Some("#5b5ff7".into());
        let (style, accent) = inline_style(&prefs, &[]);
        assert!(accent.unwrap().is_err());
        assert_eq!(style, "--cs-ui-scale: 1.1;");
    }
}
