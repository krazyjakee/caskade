//! Screenshots of the reference screens per theme (#90), rendered headless
//! with vello_cpu through the app's own look (`theme.rs`): built with the
//! `shots` feature, `caskade-native --shots <folder> [--themes <folder>]`
//! writes `<screen>-<theme>.png` for every valid theme (the built-in ones
//! and those installed in `--themes`), and beside each the same document as
//! `<screen>-<theme>.html` (its CSS, attributes and markup, with the fonts
//! and theme assets next to it) for another engine to render
//! (`scripts/webkit-shots.py`). `tests/shots.rs` compares the PNGs with
//! the committed ones.

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

/// Writes every screen in every valid theme to `out`.
pub fn run(out: &Path, themes_folder: Option<&Path>) -> Result<(), String> {
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
        for (screen, root) in screens::SCREENS {
            render(out, &themes, &info.id, screen, root)?;
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
) -> Result<(), String> {
    let active = themes.resolve(theme);
    let scheme = match caskade_ui_theme::appearance::color_scheme(&active.modes, false) {
        "light" => ColorScheme::Light,
        _ => ColorScheme::Dark,
    };
    let look = crate::theme::look(
        themes,
        &active.id,
        active.modes,
        AppearanceSettings::default(),
        false,
        false,
    );
    let mut document = dioxus_native::DioxusDocument::new(
        VirtualDom::new(root),
        DocumentConfig {
            viewport: Some(Viewport::new(WIDTH, HEIGHT, 1.0, scheme)),
            base_url: Some(crate::theme::BASE_URL.to_owned()),
            net_provider: Some(crate::theme::Net::shared(None)),
            font_ctx: Some(crate::theme::fonts()),
            ..Default::default()
        },
    );
    crate::theme::apply(&mut document.inner.borrow_mut(), &look);
    crate::theme::set_app_css(&mut document.inner.borrow_mut(), SHOTS_CSS);
    document.initial_build();
    let mut doc = document.inner.borrow_mut();
    // Fonts a theme loads arrive as messages the next resolve takes, and
    // lay the text out again in the one after.
    for _ in 0..3 {
        doc.resolve(0.0);
    }
    let pixels = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| blitz_paint::paint_scene(scene, &mut doc, 1.0, WIDTH, HEIGHT, 0, 0),
        WIDTH,
        HEIGHT,
    );
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
