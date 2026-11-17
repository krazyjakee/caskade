//! Screenshots of the reference screens per theme (#90), rendered headless
//! with vello_cpu through the app's own look (`theme.rs`): built with the
//! `shots` feature, `caskade-native --shots <folder> [--themes <folder>]`
//! writes `<screen>-<theme>.png` for every valid theme (the built-in ones
//! and those installed in `--themes`), and beside each the same document as
//! `<screen>-<theme>.html` (its CSS, attributes and markup, with the fonts
//! and theme assets next to it) for another engine to render
//! (`scripts/webkit-shots.py`). `tests/shots.rs` compares the PNGs with
//! the committed ones.
//!
//! The screens: the gallery's pages and the reference screens
//! (`shots/screens.rs`), and the real frame over the fixture library
//! (`frame::shot`): the library (`shell`), Home, a game's page, the Store,
//! Collections, Activity, Settings' first section, first-run setup and the
//! game editor, in every theme; and in the default theme only, every other
//! Settings section, console mode, reduced motion and effects, and (debug
//! builds) the pseudo-locale (#153).
//!
//! `--shots <folder> --website-art <folder>` writes the website's captures
//! instead (#150, [`website`]): `{desktop,console}-{en,sv}.jpg`, over a
//! sample library whose covers are in the second folder
//! (`scripts/website-art.sh`).

mod screens;

/// The app's stylesheet with the reference screens' classes too
/// (`scripts/build-css.mjs`).
const SHOTS_CSS: &str = include_str!("shots/shots.css");

use std::path::{Path, PathBuf};

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::{BaseDocument, DocumentConfig};
use blitz_traits::shell::{ColorScheme, Viewport};
use caskade_core::settings::AppearanceSettings;
use caskade_core::themes::ThemeService;
use dioxus_native::prelude::VirtualDom;

/// The window size every screen is rendered at, in CSS pixels at scale 1.
pub const WIDTH: u32 = 1280;
pub const HEIGHT: u32 = 800;

/// `--shots <folder> [--themes <folder>]`, when given.
pub fn requested() -> Option<(PathBuf, Option<PathBuf>)> {
    let args: Vec<String> = std::env::args().collect();
    let value = |flag: &str| {
        args.iter()
            .position(|arg| arg == flag)
            .and_then(|at| args.get(at + 1))
            .map(PathBuf::from)
    };
    Some((value("--shots")?, value("--themes")))
}

/// Writes every screen in every valid theme to `out` (with
/// `--website-art <folder>`, the website's captures).
pub fn run(out: &Path, themes_folder: Option<&Path>) -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if let Some(at) = args.iter().position(|arg| arg == "--website-art") {
        let art = args
            .get(at + 1)
            .ok_or("--website-art needs the covers' folder")?;
        return website(out, Path::new(art));
    }
    std::fs::create_dir_all(out).map_err(|error| format!("{}: {error}", out.display()))?;
    let scratch;
    let folder = match themes_folder {
        Some(folder) => folder.to_owned(),
        None => {
            scratch = out.join(".no-themes");
            std::fs::create_dir_all(&scratch).map_err(|error| error.to_string())?;
            scratch.clone()
        }
    };
    let themes = ThemeService::new(&folder);
    crate::theme::serve(&themes);
    for info in themes.list() {
        if !info.problems.is_empty() {
            tracing::warn!(theme = info.id, "invalid theme: not rendered");
            continue;
        }
        for &(screen, root) in screens::SCREENS.iter().chain(crate::gallery::SCREENS) {
            render(out, &themes, &info.id, screen, root, None, Shown::Plain)?;
        }
        // The app's own frame over the fixture library (#96), once its data
        // is there.
        render(
            out,
            &themes,
            &info.id,
            "shell",
            crate::frame::shot::screen,
            Some(&crate::frame::shot::ready),
            Shown::Plain,
        )?;
        render(
            out,
            &themes,
            &info.id,
            "home",
            crate::frame::shot::home,
            Some(&crate::frame::shot::home_ready),
            Shown::Plain,
        )?;
        // The views (#153): a game's page, the Store, Collections, Activity,
        // Settings' first section, first-run setup and the game editor.
        for &(screen, href, (name, value)) in crate::frame::shot::PAGES {
            crate::frame::shot::open_page(href);
            render(
                out,
                &themes,
                &info.id,
                screen,
                crate::frame::shot::page,
                Some(&|doc: &BaseDocument| crate::frame::shot::has(doc, name, value)),
                Shown::Plain,
            )?;
        }
        render(
            out,
            &themes,
            &info.id,
            "onboarding",
            crate::frame::shot::onboarding,
            Some(&crate::frame::shot::onboarding_ready),
            Shown::Plain,
        )?;
        render(
            out,
            &themes,
            &info.id,
            "editor",
            crate::frame::shot::editor,
            Some(&crate::frame::shot::editor_ready),
            Shown::Plain,
        )?;
        if info.id == DEFAULT_THEME {
            default_only(out, &themes)?;
        }
    }
    Ok(())
}

/// The theme the screens shown in one theme only are in.
const DEFAULT_THEME: &str = "caskade.default";

/// How a screen is shown, besides its theme.
#[derive(Clone, Copy, PartialEq)]
enum Shown {
    Plain,
    /// Appearance's reduced motion and reduced effects.
    Reduced,
}

/// Settings sections no image of stays the same: they show this machine's
/// free space (Storage) or times relative to now (Backup, Statistics).
const UNSTEADY_SETTINGS: [&str; 3] = ["storage", "backup", "statistics"];

/// The screens in the default theme only (#153): every Settings section
/// past the first (but [`UNSTEADY_SETTINGS`]), console mode's Home, Home
/// with reduced motion and effects, and Settings in the pseudo-locale
/// (debug builds, which have it).
fn default_only(out: &Path, themes: &ThemeService) -> Result<(), String> {
    for tab in crate::router::SETTINGS_TABS
        .iter()
        .skip(1)
        .filter(|tab| !UNSTEADY_SETTINGS.contains(*tab))
    {
        let href: &'static str = Box::leak(format!("/settings?tab={tab}").into_boxed_str());
        crate::frame::shot::open_page(href);
        let panel = format!("settings-tab-{tab}-panel");
        render(
            out,
            themes,
            DEFAULT_THEME,
            &format!("settings-{tab}"),
            crate::frame::shot::page,
            Some(&|doc: &BaseDocument| crate::frame::shot::has(doc, "id", &panel)),
            Shown::Plain,
        )?;
    }
    render(
        out,
        themes,
        DEFAULT_THEME,
        "console",
        crate::frame::shot::console_home,
        Some(&crate::frame::shot::console_ready),
        Shown::Plain,
    )?;
    render(
        out,
        themes,
        DEFAULT_THEME,
        "reduced",
        crate::frame::shot::home,
        Some(&crate::frame::shot::home_ready),
        Shown::Reduced,
    )?;
    Ok(())
}

/// The website's captures (`apps/website/assets`, #150): Home over the
/// website's sample library (`frame::shot::open_website`) in the default
/// theme, on the desktop and in console mode, in English and Swedish. Each
/// renders at its window size, then keeps the top rows the site shows: the
/// desktop's hero and first shelf, without the status bar; all of console
/// mode, with its button hints.
/// A website capture: its mode, root, window width and height, and the
/// rows kept.
pub type WebsiteShot = (
    &'static str,
    fn() -> dioxus_native::prelude::Element,
    u32,
    u32,
    u32,
);

pub const WEBSITE: [WebsiteShot; 2] = [
    ("desktop", crate::frame::shot::website_home, 1440, 900, 780),
    (
        "console",
        crate::frame::shot::website_console_home,
        1440,
        900,
        900,
    ),
];

/// The languages the website comes in.
pub const WEBSITE_LANGUAGES: [&str; 2] = ["en", "sv"];

/// Writes `<mode>-<language>.jpg` for [`WEBSITE`] and
/// [`WEBSITE_LANGUAGES`] to `out`, with the covers in `art`.
pub fn website(out: &Path, art: &Path) -> Result<(), String> {
    std::fs::create_dir_all(out).map_err(|error| format!("{}: {error}", out.display()))?;
    // The built-in themes only, and the covers' artwork cache, outside
    // `out`.
    let work = std::env::temp_dir().join(format!("caskade-website-{}", std::process::id()));
    let themes_folder = work.join("themes");
    std::fs::create_dir_all(&themes_folder).map_err(|error| error.to_string())?;
    let written = crate::frame::shot::open_website(art, &work.join("artwork")).and_then(|()| {
        let themes = ThemeService::new(&themes_folder);
        crate::theme::serve(&themes);
        write_website(out, &themes)
    });
    let _ = std::fs::remove_dir_all(&work);
    written
}

fn write_website(out: &Path, themes: &ThemeService) -> Result<(), String> {
    for language in WEBSITE_LANGUAGES {
        // The core's words (`tr!`) and the UI's (`t!`).
        caskade_core::i18n::set_locale(language);
        caskade_ui_i18n::set_language(language);
        for (mode, root, width, height, kept) in WEBSITE {
            let (document, _) = settled(
                themes,
                "caskade.default",
                mode,
                root,
                Some(&crate::frame::shot::website_ready),
                AppearanceSettings::default(),
                (width, height),
            )?;
            let mut pixels = paint(&mut document.inner.borrow_mut(), width, height);
            pixels.truncate(width as usize * kept as usize * 4);
            let rgb = image::DynamicImage::ImageRgba8(
                image::RgbaImage::from_raw(width, kept, pixels)
                    .ok_or("the render is smaller than the window")?,
            )
            .to_rgb8();
            let jpg = out.join(format!("{mode}-{language}.jpg"));
            let file = std::fs::File::create(&jpg)
                .map_err(|error| format!("{}: {error}", jpg.display()))?;
            image::codecs::jpeg::JpegEncoder::new_with_quality(std::io::BufWriter::new(file), 88)
                .encode(rgb.as_raw(), width, kept, image::ExtendedColorType::Rgb8)
                .map_err(|error| format!("{}: {error}", jpg.display()))?;
            tracing::info!(jpg = %jpg.display(), "website shot");
        }
    }
    Ok(())
}

fn render(
    out: &Path,
    themes: &ThemeService,
    theme: &str,
    screen: &str,
    root: fn() -> dioxus_native::prelude::Element,
    // A screen whose data loads after its first render: rendered once this
    // says it's there.
    ready: Option<&dyn Fn(&BaseDocument) -> bool>,
    shown: Shown,
) -> Result<(), String> {
    let prefs = match shown {
        Shown::Reduced => AppearanceSettings {
            motion: "reduced".to_owned(),
            reduce_effects: true,
            ..AppearanceSettings::default()
        },
        Shown::Plain => AppearanceSettings::default(),
    };
    let (document, look) = settled(themes, theme, screen, root, ready, prefs, (WIDTH, HEIGHT))?;
    let mut doc = document.inner.borrow_mut();
    let pixels = paint(&mut doc, WIDTH, HEIGHT);
    let name = format!("{screen}-{theme}");
    let png = out.join(format!("{name}.png"));
    image::save_buffer(
        &png,
        &pixels,
        WIDTH,
        HEIGHT,
        image::ExtendedColorType::Rgba8,
    )
    .map_err(|error| format!("{}: {error}", png.display()))?;
    let html = out.join(format!("{name}.html"));
    std::fs::write(&html, export(&doc, &look, themes, out)?)
        .map_err(|error| format!("{}: {error}", html.display()))?;
    tracing::info!(png = %png.display(), "shot");
    Ok(())
}

/// `root` in `theme` with `prefs` at `width` × `height`, built and settled
/// (with its data, given `ready`), and the look it has.
fn settled(
    themes: &ThemeService,
    theme: &str,
    screen: &str,
    root: fn() -> dioxus_native::prelude::Element,
    ready: Option<&dyn Fn(&BaseDocument) -> bool>,
    prefs: AppearanceSettings,
    (width, height): (u32, u32),
) -> Result<(dioxus_native::DioxusDocument, crate::theme::Look), String> {
    let active = themes.resolve(theme);
    let scheme = match caskade_ui_theme::appearance::color_scheme(&active.modes, false) {
        "light" => ColorScheme::Light,
        _ => ColorScheme::Dark,
    };
    let look = crate::theme::look(themes, &active.id, active.modes, prefs, false, false);
    let mut document = dioxus_native::DioxusDocument::new(
        VirtualDom::new(root),
        DocumentConfig {
            viewport: Some(Viewport::new(width, height, 1.0, scheme)),
            base_url: Some(crate::theme::BASE_URL.to_owned()),
            net_provider: Some(crate::theme::Net::shared(None)),
            font_ctx: Some(crate::theme::fonts()),
            ..Default::default()
        },
    );
    crate::theme::apply(&mut document.inner.borrow_mut(), &look);
    crate::theme::set_app_css(&mut document.inner.borrow_mut(), SHOTS_CSS);
    document.initial_build();
    // Effects (the gallery's toasts) and what they render.
    for _ in 0..3 {
        blitz_dom::Document::poll(&mut document, None);
    }
    if let Some(ready) = ready {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            blitz_dom::Document::poll(&mut document, None);
            document.inner.borrow_mut().resolve(0.0);
            if ready(&document.inner.borrow()) {
                break;
            }
            if std::time::Instant::now() > until {
                return Err(format!("{screen}: its data never came"));
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        // What the data brought in renders, and settles.
        for _ in 0..10 {
            blitz_dom::Document::poll(&mut document, None);
            document.inner.borrow_mut().resolve(0.0);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    {
        let mut doc = document.inner.borrow_mut();
        // Fonts a theme loads arrive as messages the next resolve takes, and
        // lay the text out again in the one after.
        // A still: animations (an overlay's or a toast's entrance) have
        // ended by the last.
        for time in [0.0, 10.0, 10.0] {
            doc.resolve(time);
        }
    }
    Ok((document, look))
}

/// A settled document's RGBA pixels at `width` × `height`.
fn paint(doc: &mut BaseDocument, width: u32, height: u32) -> Vec<u8> {
    render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| blitz_paint::paint_scene(scene, doc, 1.0, width, height, 0, 0),
        width,
        height,
    )
}

/// Elements HTML lets close themselves; inside `<svg>` any element may.
const VOID: [&str; 13] = [
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track",
    "wbr",
];

/// Blitz writes an empty element as `<div … />`, which an HTML parser
/// reads as an opening tag: closes those (but void and SVG elements) as
/// `<div …></div>`.
fn html_closed(markup: &str) -> String {
    let mut out = String::with_capacity(markup.len());
    let mut rest = markup;
    let mut svg_depth = 0usize;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('>').map(|end| start + end) else {
            out.push_str(&rest[start..]);
            return out;
        };
        let tag = &rest[start..=end];
        let name: String = tag[1..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '/')
            .collect();
        if name == "svg" && !tag.ends_with("/>") {
            svg_depth += 1;
        } else if name == "/svg" {
            svg_depth = svg_depth.saturating_sub(1);
        }
        if tag.ends_with("/>") && svg_depth == 0 && !VOID.contains(&name.as_str()) {
            out.push_str(tag[..tag.len() - 2].trim_end());
            out.push_str(&format!("></{name}>"));
        } else {
            out.push_str(tag);
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

fn escape_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
}

/// The document as a page another engine renders the same: the root's
/// attributes, the bundled fonts as `@font-face`s, the same stylesheets
/// (theme asset URLs made relative to the page, the assets written beside
/// it) and the body's markup.
fn export(
    doc: &BaseDocument,
    look: &crate::theme::Look,
    themes: &ThemeService,
    out: &Path,
) -> Result<String, String> {
    let write = |path: PathBuf, bytes: &[u8]| -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::write(&path, bytes).map_err(|error| format!("{}: {error}", path.display()))
    };
    let mut fonts = String::new();
    for (family, file, bytes) in crate::theme::FONTS {
        write(out.join("fonts").join(file), bytes)?;
        fonts.push_str(&format!(
            "@font-face {{ font-family: \"{family}\"; src: url(\"fonts/{file}\") format(\"woff2\"); \
             font-weight: 100 900; font-style: normal; }}\n"
        ));
    }
    // A compiled theme's `url("/<id>/<path>")`s, relative to the page.
    let mut theme_css = String::new();
    let mut rest = &*look.theme_css;
    while let Some(at) = rest.find("url(") {
        theme_css.push_str(&rest[..at + 4]);
        rest = &rest[at + 4..];
        let quote = rest
            .chars()
            .next()
            .filter(|c| *c == '"' || *c == '\'')
            .map(String::from)
            .unwrap_or_default();
        let inner = &rest[quote.len()..];
        if let Some(path) = inner.strip_prefix('/') {
            let end = path
                .find(|c: char| c == ')' || quote.starts_with(c))
                .unwrap_or(path.len());
            let url = &path[..end];
            if let Some((id, asset)) = url.split_once('/')
                && let Some(bytes) = themes.asset(id, asset)
            {
                write(out.join("theme-assets").join(id).join(asset), &bytes)?;
            }
            theme_css.push_str(&quote);
            theme_css.push_str("theme-assets/");
            rest = path;
        }
    }
    theme_css.push_str(rest);
    let root = doc.root_element();
    let attributes: String = root
        .attrs()
        .map(|attrs| {
            attrs
                .iter()
                .map(|attr| format!(" {}=\"{}\"", attr.name.local, escape_attribute(&attr.value)))
                .collect()
        })
        .unwrap_or_default();
    let body = doc
        .find_body_node()
        .map(|body| html_closed(&body.outer_html()))
        .unwrap_or_default();
    let user = look.user_css.as_deref().unwrap_or("");
    Ok(format!(
        "<!DOCTYPE html>\n<html{attributes}>\n<head>\n<meta charset=\"utf-8\">\n\
         <style>\n{fonts}</style>\n<style>\n{}</style>\n<style>\n{theme_css}</style>\n<style>\n{user}</style>\n\
         </head>\n{body}\n</html>\n",
        SHOTS_CSS
    ))
}
