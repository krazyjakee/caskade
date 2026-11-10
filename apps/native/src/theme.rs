//! The look (#90): the app's stylesheet, the chosen theme, the user's CSS
//! and the Appearance settings, applied to Blitz's document before the
//! first frame and again whenever one changes, as the webview's
//! `appearance/startup.ts` and `AppearanceRoot` do.
//!
//! - `assets/app.css` is the webview's Tailwind entry compiled over this
//!   app's sources (`scripts/build-css.mjs`): the layer order, the tokens,
//!   base and the utilities the native UI uses.
//! - The theme's compiled stylesheet comes from `ThemeService`, in
//!   `@layer cs.theme`; its assets (fonts, images) are on the
//!   `caskade-theme:` scheme, which the document's base URL and [`Net`]
//!   serve. Built-in, installed and dev themes apply before the first
//!   frame; a plugin's theme once the plugin host has started.
//! - The user's CSS, checked as the webview checks it, in `@layer cs.user`,
//!   unless it's switched off or Caskade started in safe mode.
//! - Appearance (`caskade_ui_theme::appearance`): `data-*` attributes and
//!   inline tokens on `<html>`, the accent checked against the theme's
//!   computed surfaces.
//!
//! Each sheet is one `<style>` in `<head>`, created in this order: Blitz
//! orders author sheets by node, and the layers decide the rest. Its text
//! is parsed as written (`upsert_stylesheet_for_node`), not as HTML.

use std::sync::{Arc, Mutex, OnceLock};

use blitz_dom::{BaseDocument, LocalName, NodeId, QualName, ns};
use blitz_traits::net::{NetHandler, NetProvider, NetWaker, Request};
use caskade_core::settings::{AppearanceSettings, SettingsService};
use caskade_core::themes::ThemeService;
use caskade_ui_theme::{appearance, user_css};

/// The app's stylesheet, compiled at build time.
pub const APP_CSS: &str = include_str!("../assets/app.css");

/// Relative URLs (a theme's `/<id>/<asset>`) resolve on the theme scheme.
pub const BASE_URL: &str = "caskade-theme://localhost/";

/// Everything the look is made of, gathered off the main thread.
#[derive(Debug, Clone)]
pub struct Look {
    pub theme: String,
    pub theme_css: Arc<str>,
    pub modes: Vec<String>,
    /// The user stylesheet to apply, already checked.
    pub user_css: Option<String>,
    pub prefs: AppearanceSettings,
    pub prefers_reduced_motion: bool,
}

/// The theme service the net provider serves assets from.
static THEMES: OnceLock<ThemeService> = OnceLock::new();
/// A look waiting for the main thread.
static PENDING: Mutex<Option<Look>> = Mutex::new(None);
/// The last look applied: the OS's colour scheme re-applies it.
static LAST: Mutex<Option<Look>> = Mutex::new(None);
/// The OS asks for a light look (winit's window theme).
static PREFERS_LIGHT: Mutex<bool> = Mutex::new(false);
/// The app's stylesheet when it isn't [`APP_CSS`]: the gallery's (#93).
static APP_SHEET: OnceLock<&'static str> = OnceLock::new();

/// The window's documents get `css` as the app's stylesheet instead of
/// [`APP_CSS`]: the gallery's, which has its pages' classes too. Before
/// the window opens.
#[cfg(any(debug_assertions, feature = "gallery"))]
pub fn use_app_css(css: &'static str) {
    let _ = APP_SHEET.set(css);
}

/// A `<style>` and its text node.
#[derive(Clone, Copy)]
struct Sheet {
    style: NodeId,
    text: NodeId,
}

/// The theme's and the user's sheets (the app's never changes).
#[derive(Clone, Copy)]
struct Sheets {
    theme: Sheet,
    user: Sheet,
}

/// The settings' appearance, or the defaults.
async fn prefs(settings: &SettingsService) -> AppearanceSettings {
    settings
        .read_or_default::<AppearanceSettings>("core.appearance")
        .await
}

/// The look for `theme` (resolved, with its compiled sheet) and `prefs`.
pub fn look(
    themes: &ThemeService,
    theme: &str,
    modes: Vec<String>,
    prefs: AppearanceSettings,
    safe: bool,
    prefers_reduced_motion: bool,
) -> Look {
    let theme_css = themes.stylesheet(theme).unwrap_or_else(|error| {
        tracing::warn!(%error, theme, "theme not applied");
        Arc::from("")
    });
    let user_css = if prefs.custom_css_enabled && !safe {
        match user_css::applicable(&prefs.custom_css) {
            Ok(css) => css,
            Err(problems) => {
                tracing::warn!(
                    "custom CSS not applied:\n{}",
                    user_css::problem_text(&problems)
                );
                None
            }
        }
    } else {
        None
    };
    Look {
        theme: theme.to_owned(),
        theme_css,
        modes,
        user_css,
        prefs,
        prefers_reduced_motion,
    }
}

/// Serves `themes`' assets to the documents (once).
pub fn serve(themes: &ThemeService) {
    let _ = THEMES.set(themes.clone());
}

/// The look before the window opens: the chosen theme from the themes
/// found so far (plugins' come later), the settings and the user's CSS.
pub fn first(themes: &ThemeService, settings: &SettingsService) -> Look {
    serve(themes);
    let safe = caskade_app::commands::themes::safe_mode(None);
    let prefs = caskade_app::runtime::block_on(prefs(settings));
    let requested = if safe {
        AppearanceSettings::default().theme
    } else {
        prefs.theme.clone()
    };
    let active = themes.resolve(&requested);
    if let Some(notice) = &active.notice {
        tracing::info!(notice, "the first frame has another theme");
    }
    look(
        themes,
        &active.id,
        active.modes,
        prefs,
        safe,
        prefers_reduced_motion(),
    )
}

/// The look now, from the running services: plugin themes included, the
/// requested one's fallback noticed.
async fn current(services: &caskade_app::start::Services) -> Look {
    let safe = caskade_app::commands::themes::safe_mode(None);
    let prefs = prefs(&services.settings).await;
    let resolved = caskade_app::commands::themes::theme_resolve(
        &services.themes,
        Some(&services.settings),
        services.plugins.as_ref(),
        safe,
        |_, _| String::new(),
        None,
    )
    .await;
    let (theme, modes) = match resolved {
        Ok(resolved) => {
            if let Some(notice) = &resolved.notice {
                tracing::warn!(notice, "theme");
            }
            (resolved.id, resolved.modes)
        }
        Err(error) => {
            tracing::warn!(error = %error.summary, "theme not applied");
            let fallback = services
                .themes
                .resolve(&AppearanceSettings::default().theme);
            (fallback.id, fallback.modes)
        }
    };
    look(
        &services.themes,
        &theme,
        modes,
        prefs,
        safe,
        prefers_reduced_motion(),
    )
}

/// Shows `look` in the window: the gallery's theme switcher (#93).
#[cfg(any(debug_assertions, feature = "gallery"))]
pub fn show(look: Look) {
    restyle(look);
}

/// Asks the main thread to apply `look`.
fn restyle(look: Look) {
    *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some(look);
    crate::launch::ask(crate::launch::Request::Restyle);
}

/// Keeps the look live once the services run: the theme with plugins'
/// themes known, then every `core.appearance` change and dev theme reload.
pub fn follow(services: &'static caskade_app::start::Services) {
    caskade_app::runtime::spawn(async move {
        restyle(current(services).await);
        let mut settings = services.settings.subscribe();
        let mut events = crate::shell::events().subscribe();
        use tokio::sync::broadcast::error::RecvError;
        loop {
            let changed = tokio::select! {
                changed = settings.recv() => match changed {
                    Ok(changed) => changed.namespace == "core.appearance",
                    Err(RecvError::Lagged(_)) => true,
                    Err(RecvError::Closed) => break,
                },
                event = events.recv() => match event {
                    Ok(caskade_app::events::Event::ThemeReloaded(reloaded)) => {
                        if let Some(error) = &reloaded.error {
                            tracing::warn!(theme = reloaded.theme, error, "theme reload failed");
                        }
                        true
                    }
                    Ok(_) => false,
                    Err(RecvError::Lagged(_)) => true,
                    Err(RecvError::Closed) => break,
                },
            };
            if changed {
                restyle(current(services).await);
            }
        }
    });
}

/// The OS switched between light and dark: themes with both follow it.
pub fn prefers_light(light: bool) {
    {
        let mut current = PREFERS_LIGHT.lock().unwrap_or_else(|e| e.into_inner());
        if *current == light {
            return;
        }
        *current = light;
    }
    if let Some(look) = LAST.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        restyle(look);
    }
}

/// Applies the look waiting for the main thread, if any; whether it did.
pub fn apply_pending(doc: &mut BaseDocument) -> bool {
    let Some(look) = PENDING.lock().unwrap_or_else(|e| e.into_inner()).take() else {
        return false;
    };
    apply(doc, &look);
    true
}

/// `data-paint="cpu"` on the root: the window draws on the CPU (#110), so
/// the tokens' `paint-cpu` set pauses ambient loops.
pub fn mark_cpu_paint(doc: &mut BaseDocument) {
    let root = doc.root_element().id;
    doc.mutate().set_attribute(root, qual("data-paint"), "cpu");
}

fn qual(name: &str) -> QualName {
    QualName::new(None, ns!(), LocalName::from(name))
}

/// The `<style data-cs-sheet="which">` in `head` and its text node.
fn find_sheet(doc: &BaseDocument, head: NodeId, which: &str) -> Option<Sheet> {
    let head = doc.get_node(head)?;
    head.children.iter().find_map(|&style| {
        let node = doc.get_node(style)?;
        if node.attr(LocalName::from("data-cs-sheet")) != Some(which) {
            return None;
        }
        Some(Sheet {
            style,
            text: *node.children.first()?,
        })
    })
}

/// The document's sheets, created in `<head>` the first time, the app's
/// with its CSS.
fn sheets(doc: &mut BaseDocument) -> Option<Sheets> {
    let head = doc.find_head_node()?.id;
    if let (Some(theme), Some(user)) = (
        find_sheet(doc, head, "theme"),
        find_sheet(doc, head, "user"),
    ) {
        return Some(Sheets { theme, user });
    }
    let mut mutator = doc.mutate();
    let mut sheet = |which: &str, css: &str| {
        let style = mutator.create_element(qual("style"), Vec::new());
        mutator.set_attribute(style, qual("data-cs-sheet"), which);
        let text = mutator.create_text_node(css);
        mutator.append_children(style, &[text]);
        mutator.append_children(head, &[style]);
        Sheet { style, text }
    };
    let app = sheet("app", APP_SHEET.get().copied().unwrap_or(APP_CSS));
    let sheets = Sheets {
        theme: sheet("theme", ""),
        user: sheet("user", ""),
    };
    drop(mutator);
    doc.upsert_stylesheet_for_node(app.style);
    Some(sheets)
}

/// Replaces the app's stylesheet in `doc` with `css`: the screenshot
/// harness's, which also has its reference screens' classes.
#[cfg(feature = "shots")]
pub fn set_app_css(doc: &mut BaseDocument, css: &str) {
    if let Some(head) = doc.find_head_node().map(|head| head.id)
        && let Some(app) = find_sheet(doc, head, "app")
    {
        set_sheet(doc, app, css);
    }
}

/// Gives `sheet` the stylesheet `css`, unless it has it already (a
/// stylesheet is parsed again only when it changed).
fn set_sheet(doc: &mut BaseDocument, sheet: Sheet, css: &str) {
    let same = doc
        .get_node(sheet.text)
        .is_some_and(|text| text.text_data().is_some_and(|data| data.content == css));
    if same {
        return;
    }
    doc.mutate().set_node_text(sheet.text, css);
    // After the mutator's own pass, which reads the text as HTML.
    doc.upsert_stylesheet_for_node(sheet.style);
}

/// Applies `look` to `doc` (on the main thread).
pub fn apply(doc: &mut BaseDocument, look: &Look) {
    tracing::debug!(
        theme = look.theme,
        theme_css = look.theme_css.len(),
        user_css = look.user_css.as_ref().map(String::len),
        "look applied"
    );
    let Some(sheets) = sheets(doc) else {
        tracing::warn!("the document has no <head>: no styles");
        return;
    };
    set_sheet(doc, sheets.theme, &look.theme_css);
    set_sheet(doc, sheets.user, look.user_css.as_deref().unwrap_or(""));
    let root = doc.root_element().id;
    let context = appearance::Context {
        modes: look.modes.clone(),
        prefers_light: *PREFERS_LIGHT.lock().unwrap_or_else(|e| e.into_inner()),
    };
    // `@media (prefers-reduced-motion)`, for "system" motion and themes.
    doc.viewport_mut().prefers_reduced_motion = look.prefers_reduced_motion;
    {
        let mut mutator = doc.mutate();
        for (name, value) in appearance::attributes(&look.prefs, &context) {
            match value {
                Some(value) => mutator.set_attribute(root, qual(name), &value),
                None => mutator.clear_attribute(root, qual(name)),
            }
        }
        // The accent is checked against the theme's surfaces, so they're
        // read with the theme and attributes in place.
        let (style, _) = appearance::inline_style(&look.prefs, &[]);
        mutator.set_attribute(root, qual("style"), &style);
    }
    if look
        .prefs
        .accent
        .as_deref()
        .is_some_and(|accent| !accent.is_empty())
    {
        doc.resolve(0.0);
        let surfaces = appearance::surfaces(
            doc.resolved_style_value(root, "--cs-color-surface-base")
                .trim(),
            doc.resolved_style_value(root, "--cs-color-surface-raised")
                .trim(),
        );
        let (style, accent) = appearance::inline_style(&look.prefs, &surfaces);
        match accent {
            Some(Ok(accent)) => {
                if let Some(message) = accent.message {
                    tracing::info!(theme = look.theme, message, "accent");
                }
            }
            Some(Err(message)) => tracing::warn!(theme = look.theme, message, "accent"),
            None => {}
        }
        doc.mutate().set_attribute(root, qual("style"), &style);
    }
    *LAST.lock().unwrap_or_else(|e| e.into_inner()) = Some(look.clone());
}

/// The fonts the tokens name, bundled as the webview bundles them
/// (`@fontsource-variable`, latin): family, file and bytes.
pub const FONTS: [(&str, &str, &[u8]); 3] = [
    (
        "Inter Variable",
        "inter-latin-wght-normal.woff2",
        include_bytes!("../assets/fonts/inter-latin-wght-normal.woff2"),
    ),
    (
        "Oxanium Variable",
        "oxanium-latin-wght-normal.woff2",
        include_bytes!("../assets/fonts/oxanium-latin-wght-normal.woff2"),
    ),
    (
        "JetBrains Mono Variable",
        "jetbrains-mono-latin-wght-normal.woff2",
        include_bytes!("../assets/fonts/jetbrains-mono-latin-wght-normal.woff2"),
    ),
];

/// [`FONTS`] under the tokens' family names, so a theme that names another
/// font still wins.
pub fn fonts() -> blitz_dom::FontContext {
    use parley::fontique::{Blob, FontInfoOverride};
    let mut fonts = blitz_dom::FontContext::default();
    for (family, _, data) in FONTS {
        let decoded = blitz_dom::util::decode_font_bytes(data).into_owned();
        fonts.collection.register_fonts(
            Blob::new(Arc::new(decoded) as _),
            Some(FontInfoOverride {
                family_name: Some(family),
                ..FontInfoOverride::default()
            }),
        );
    }
    fonts
}

/// The document's resources: `data:` URLs (Blitz's own provider), theme
/// assets on `caskade-theme://localhost/<id>/<path>`, read from the theme
/// service, and game artwork on `caskade-asset://` (the library's cached
/// thumbnails, `library::fetch_art`). Nothing else: no network (#87).
pub struct Net {
    data: Arc<dyn NetProvider>,
}

impl Net {
    pub fn shared(waker: Option<Arc<dyn NetWaker>>) -> Arc<dyn NetProvider> {
        Arc::new(Self {
            data: blitz_shell::DataUriNetProvider::shared(waker),
        })
    }
}

/// The theme and path of a `caskade-theme:` URL.
fn theme_asset(url: &url::Url) -> Option<(String, String)> {
    let mut segments = url.path_segments()?;
    let id = segments.next().filter(|id| !id.is_empty())?;
    let path = segments.collect::<Vec<_>>().join("/");
    let decode = |text: &str| {
        percent_encoding::percent_decode_str(text)
            .decode_utf8()
            .ok()
            .map(|text| text.into_owned())
    };
    Some((decode(id)?, decode(&path)?))
}

impl NetProvider for Net {
    fn fetch(&self, doc_id: usize, request: Request, handler: Box<dyn NetHandler>) {
        let handler = match crate::library::fetch_art(&request, handler) {
            Ok(()) => return,
            Err(handler) => handler,
        };
        if request.url.scheme() != "caskade-theme" {
            return self.data.fetch(doc_id, request, handler);
        }
        let url = request.url.to_string();
        let asset = theme_asset(&request.url)
            .and_then(|(id, path)| THEMES.get().and_then(|themes| themes.asset(&id, &path)));
        match asset {
            Some(bytes) => handler.bytes(url, bytes.into()),
            None => {
                tracing::debug!(url, "no such theme asset");
                handler.error(url, "no such theme asset".into());
            }
        }
    }
}

/// Whether the OS asks for less motion: Blitz evaluates
/// `@media (prefers-reduced-motion)` with it, which "system" motion and
/// themes follow.
pub fn prefers_reduced_motion() -> bool {
    #[cfg(target_os = "linux")]
    {
        // GNOME's and most GTK desktops' "animations" switch.
        use gtk::prelude::ObjectExt;
        crate::tray::on_gtk(|| {
            gtk::Settings::default()
                .map(|settings| !settings.property::<bool>("gtk-enable-animations"))
                .unwrap_or(false)
        })
        .unwrap_or(false)
    }
    #[cfg(target_os = "windows")]
    {
        // Settings → Accessibility → Visual effects → Animation effects,
        // which also turns the window animations off.
        windows_registry::CURRENT_USER
            .open(r"Control Panel\Desktop\WindowMetrics")
            .and_then(|key| key.get_string("MinAnimate"))
            .is_ok_and(|value| value.trim() == "0")
    }
    #[cfg(target_os = "macos")]
    {
        // System Settings → Accessibility → Display → Reduce motion.
        std::process::Command::new("defaults")
            .args(["read", "com.apple.universalaccess", "reduceMotion"])
            .output()
            .is_ok_and(|output| String::from_utf8_lossy(&output.stdout).trim() == "1")
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_theme_url_names_the_theme_and_its_asset() {
        let url = |text: &str| url::Url::parse(text).unwrap();
        assert_eq!(
            theme_asset(&url(
                "caskade-theme://localhost/caskade.paper/assets/fonts/a%20b.woff2"
            )),
            Some(("caskade.paper".into(), "assets/fonts/a b.woff2".into()))
        );
        // The base URL joined with a compiled theme's `/<id>/<path>`.
        let joined = url(BASE_URL)
            .join("/caskade.paper/assets/grain.svg")
            .unwrap();
        assert_eq!(
            theme_asset(&joined),
            Some(("caskade.paper".into(), "assets/grain.svg".into()))
        );
        assert_eq!(theme_asset(&url("caskade-theme://localhost/")), None);
    }

    fn document() -> dioxus_native::DioxusDocument {
        use dioxus_native::prelude::*;
        fn root() -> Element {
            rsx! { div { "Caskade" } }
        }
        dioxus_native::DioxusDocument::new(
            VirtualDom::new(root),
            blitz_dom::DocumentConfig {
                base_url: Some(BASE_URL.to_owned()),
                ..Default::default()
            },
        )
    }

    fn computed(doc: &mut BaseDocument, property: &str) -> String {
        doc.resolve(0.0);
        let root = doc.root_element().id;
        doc.resolved_style_value(root, property).trim().to_owned()
    }

    #[test]
    fn the_theme_and_user_css_beat_the_tokens() {
        let mut document = document();
        let mut look = Look {
            theme: "test".into(),
            theme_css: Arc::from(
                "@layer cs.theme {\n:root {\n  --cs-color-surface-base: #f3eee3;\n}\n}\n",
            ),
            modes: vec!["light".into()],
            user_css: None,
            prefs: AppearanceSettings::default(),
            prefers_reduced_motion: false,
        };
        // As the app does: before Dioxus builds the page.
        apply(&mut document.inner.borrow_mut(), &look);
        document.initial_build();
        let mut doc = document.inner.borrow_mut();
        assert_eq!(computed(&mut doc, "--cs-color-surface-base"), "#f3eee3");
        look.user_css = Some(user_css::stylesheet(
            ":root { --cs-color-surface-base: #123456; }",
        ));
        apply(&mut doc, &look);
        assert_eq!(computed(&mut doc, "--cs-color-surface-base"), "#123456");
        // Appearance: the attributes select the token sets, the accent is
        // checked against the computed surfaces.
        look.user_css = None;
        apply(&mut doc, &look);
        let own_accent = computed(&mut doc, "--cs-color-accent");
        let surfaces = appearance::surfaces(
            &computed(&mut doc, "--cs-color-surface-base"),
            &computed(&mut doc, "--cs-color-surface-raised"),
        );
        let expected = caskade_ui_theme::accent::derive("#ffe14d", &surfaces).unwrap();
        look.prefs.density = "compact".into();
        look.prefs.motion = "reduced".into();
        look.prefs.ui_scale = 120;
        look.prefs.accent = Some("#ffe14d".into());
        apply(&mut doc, &look);
        assert_eq!(computed(&mut doc, "--cs-density-scale"), "0.85");
        assert_eq!(computed(&mut doc, "--cs-motion-duration-fast"), "0ms");
        assert_eq!(computed(&mut doc, "--cs-ui-scale"), "1.2");
        assert_eq!(computed(&mut doc, "--cs-color-accent"), expected.accent);
        assert_eq!(computed(&mut doc, "--cs-color-glow"), expected.glow);
        let root = doc.root_element().id;
        assert_eq!(
            doc.get_node(root)
                .and_then(|node| node.attr(LocalName::from("data-color-scheme"))),
            Some("light")
        );
        // Back to the theme's own accent.
        look.prefs.accent = None;
        apply(&mut doc, &look);
        assert_eq!(computed(&mut doc, "--cs-color-accent"), own_accent);
    }

    /// "system" motion follows the OS through tokens.css's
    /// `@media (prefers-reduced-motion)`; "full" overrides it.
    #[test]
    fn system_motion_follows_the_os() {
        let mut document = document();
        let mut look = Look {
            theme: "test".into(),
            theme_css: Arc::from(""),
            modes: vec!["dark".into()],
            user_css: None,
            prefs: AppearanceSettings::default(),
            prefers_reduced_motion: true,
        };
        assert_eq!(look.prefs.motion, "system");
        apply(&mut document.inner.borrow_mut(), &look);
        document.initial_build();
        let mut doc = document.inner.borrow_mut();
        assert_eq!(computed(&mut doc, "--cs-motion-duration-fast"), "0ms");
        look.prefs.motion = "full".into();
        apply(&mut doc, &look);
        assert_ne!(computed(&mut doc, "--cs-motion-duration-fast"), "0ms");
        look.prefs.motion = "system".into();
        look.prefers_reduced_motion = false;
        apply(&mut doc, &look);
        assert_ne!(computed(&mut doc, "--cs-motion-duration-fast"), "0ms");
    }

    #[test]
    fn every_built_in_theme_sets_its_surfaces() {
        // Ember is the catalogue's: installed in the user's theme folder.
        let folder = tempfile::tempdir().unwrap();
        let ember = folder.path().join("ember");
        std::fs::create_dir(&ember).unwrap();
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes/ember");
        for file in std::fs::read_dir(&source).unwrap() {
            let file = file.unwrap();
            std::fs::copy(file.path(), ember.join(file.file_name())).unwrap();
        }
        let themes = ThemeService::new(folder.path());
        for (id, base) in [
            ("caskade.default", "#060914"),
            ("caskade.cobalt-five", "#081433"),
            ("caskade.emerald-one", "#0b0d0c"),
            ("caskade.toggle-two", "#eef0f3"),
            ("dev.caskade.ember", ""),
        ] {
            let mut document = document();
            let active = themes.resolve(id);
            assert_eq!(active.id, id);
            let look = look(
                &themes,
                id,
                active.modes,
                AppearanceSettings::default(),
                false,
                false,
            );
            apply(&mut document.inner.borrow_mut(), &look);
            document.initial_build();
            let mut doc = document.inner.borrow_mut();
            let found = computed(&mut doc, "--cs-color-surface-base");
            eprintln!("{id}: {found}");
            if !base.is_empty() {
                assert_eq!(found, base, "{id}");
            }
        }
    }

    #[test]
    fn the_app_stylesheet_is_current() {
        // `node apps/native/scripts/build-css.mjs --check` checks it against
        // the sources; here, that it's the generated one with the contract's
        // layers.
        assert!(APP_CSS.starts_with("/* Generated by apps/native/scripts/build-css.mjs"));
        assert!(APP_CSS.contains(caskade_ui_theme::user_css::LAYER_ORDER));
    }
}
