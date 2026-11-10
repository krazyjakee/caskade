//! Cut titles and empty fields as Blitz paints them (vello_cpu) and as
//! Chromium does (#80): the same page, rendered by both, measured the same
//! way. Chromium's rendering is committed (`tests/text/chromium.png`,
//! `scripts/text-oracle.sh` makes it again from the page this test
//! writes); a title cut with `text-overflow: ellipsis` ends in three dots
//! where Chromium's does, a clipped one doesn't, and a placeholder shows
//! in its `::placeholder` colour (or the default grey) only while its
//! field is empty. A second page does the same with the app's own
//! stylesheet and classes (`truncate`, `placeholder:text-fg-subtle`): the
//! placeholder takes the theme's muted colour.
//!
//! `cargo test -p caskade-native --features shots --test text_pixels`

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::DocumentConfig;
use blitz_traits::shell::{ColorScheme, Viewport};
use dioxus_native::DioxusDocument;
use dioxus_native::prelude::*;
use image::RgbaImage;
use std::path::Path;

const WIDTH: u32 = 400;
const HEIGHT: u32 = 260;
const FONT: &[u8] = include_bytes!("../assets/fonts/inter-latin-wght-normal.woff2");
const FAMILY: &str = "Inter Variable";

/// Where each case is: left, top, width, height.
const CUT: Rect = (20, 20, 150, 24);
const CLIPPED: Rect = (20, 60, 150, 24);
const THEMED: Rect = (20, 100, 220, 30);
const DEFAULT: Rect = (20, 150, 220, 30);
const FILLED: Rect = (20, 200, 220, 30);

type Rect = (u32, u32, u32, u32);

const TITLE: &str = "The Legend of a Very Long Game Title";

/// The app's stylesheet, with the default theme's tokens.
const APP_CSS: &str = include_str!("../assets/app.css");
/// On the app's page: a card's title and a text field, with its classes.
const APP_TITLE: Rect = (20, 20, 150, 24);
const APP_FIELD: Rect = (20, 60, 220, 30);

fn page() -> Element {
    let place = |(left, top, width, height): Rect| {
        format!(
            "position: absolute; left: {left}px; top: {top}px; width: {width}px; height: {height}px; \
             box-sizing: border-box; margin: 0; font: 16px/24px '{FAMILY}'; color: #000;"
        )
    };
    let field = "border: 0; padding: 0 6px; background: #fff; outline: none;";
    rsx! {
        style {
            "body {{ margin: 0; background: #fff; }} .themed::placeholder {{ color: rgb(220, 0, 0); }}"
        }
        div {
            style: "{place(CUT)} white-space: nowrap; overflow: hidden; text-overflow: ellipsis;",
            "{TITLE}"
        }
        div { style: "{place(CLIPPED)} white-space: nowrap; overflow: hidden;", "{TITLE}" }
        input {
            class: "themed",
            style: "{place(THEMED)} {field}",
            placeholder: "Search games",
        }
        input { style: "{place(DEFAULT)} {field}", placeholder: "Search games" }
        input {
            style: "{place(FILLED)} {field}",
            placeholder: "Search games",
            value: "Hi",
        }
    }
}

fn fonts() -> blitz_dom::FontContext {
    use parley::fontique::{Blob, FontInfoOverride};
    let mut fonts = blitz_dom::FontContext::default();
    let decoded = blitz_dom::util::decode_font_bytes(FONT).into_owned();
    fonts.collection.register_fonts(
        Blob::new(std::sync::Arc::new(decoded) as _),
        Some(FontInfoOverride {
            family_name: Some(FAMILY),
            ..FontInfoOverride::default()
        }),
    );
    fonts
}

fn app_page() -> Element {
    rsx! {
        style { "{APP_CSS}" }
        style { "body {{ margin: 0; background: #fff; }}" }
        p {
            class: "truncate text-sm font-semibold",
            style: "position: absolute; left: {APP_TITLE.0}px; top: {APP_TITLE.1}px; width: {APP_TITLE.2}px; height: {APP_TITLE.3}px; font-family: '{FAMILY}'; line-height: 24px; color: #000;",
            "{TITLE}"
        }
        input {
            class: "w-full min-w-0 bg-transparent text-sm placeholder:text-fg-subtle focus:outline-none",
            style: "position: absolute; left: {APP_FIELD.0}px; top: {APP_FIELD.1}px; width: {APP_FIELD.2}px; height: {APP_FIELD.3}px; padding: 0 6px; font-family: '{FAMILY}';",
            placeholder: "Search games",
        }
    }
}

/// The default theme's muted text colour (`--cs-color-fg-subtle`).
fn fg_subtle() -> [u8; 3] {
    let at = APP_CSS.find("--cs-color-fg-subtle: #").expect("the token") + 23;
    let hex = &APP_CSS[at..at + 6];
    let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).unwrap();
    [channel(0), channel(2), channel(4)]
}

/// Blitz's rendering of a page, and the page as HTML for Chromium.
fn render(page: fn() -> Element) -> (RgbaImage, String) {
    let mut document = DioxusDocument::new(
        VirtualDom::new(page),
        DocumentConfig {
            viewport: Some(Viewport::new(WIDTH, HEIGHT, 1.0, ColorScheme::Light)),
            font_ctx: Some(fonts()),
            ..Default::default()
        },
    );
    document.initial_build();
    let mut doc = document.inner.borrow_mut();
    for _ in 0..3 {
        doc.resolve(0.0);
    }
    let pixels = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| blitz_paint::paint_scene(scene, &mut doc, 1.0, WIDTH, HEIGHT, 0, 0),
        WIDTH,
        HEIGHT,
    );
    let body = doc
        .find_body_node()
        .map(|body| body.outer_html())
        .unwrap_or_default();
    let html = format!(
        "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n<style>\n\
         @font-face {{ font-family: \"{FAMILY}\"; src: url(\"inter.woff2\") format(\"woff2\"); \
         font-weight: 100 900; }}\n</style>\n</head>\n{body}\n</html>\n"
    );
    (
        RgbaImage::from_raw(WIDTH, HEIGHT, pixels).expect("a whole image"),
        html,
    )
}

/// What's drawn in a case's box.
#[derive(Debug)]
struct Ink {
    /// The first and last columns with ink, from the box's left.
    left: u32,
    right: u32,
    /// The darkest pixel: a glyph's full colour.
    darkest: [u8; 3],
    /// Dots at the end: small marks, apart, on the baseline (an ellipsis
    /// is three).
    dots: usize,
}

fn inked(pixel: &image::Rgba<u8>) -> bool {
    pixel.0[..3].iter().any(|&channel| channel < 200)
}

fn ink(image: &RgbaImage, (left, top, width, height): Rect) -> Option<Ink> {
    let column = |x: u32| -> Vec<u32> {
        (top..top + height)
            .filter(|&y| inked(image.get_pixel(left + x, y)))
            .collect()
    };
    let columns: Vec<Vec<u32>> = (0..width).map(column).collect();
    let first = columns.iter().position(|rows| !rows.is_empty())? as u32;
    let last = columns.iter().rposition(|rows| !rows.is_empty())? as u32;
    let mut darkest = [255u8; 3];
    for x in left..left + width {
        for y in top..top + height {
            let pixel = image.get_pixel(x, y).0;
            let sum = |p: [u8; 3]| p.iter().map(|&c| u32::from(c)).sum::<u32>();
            if sum([pixel[0], pixel[1], pixel[2]]) < sum(darkest) {
                darkest = [pixel[0], pixel[1], pixel[2]];
            }
        }
    }
    // Runs of inked columns from the right, while they're dots: narrow
    // and short.
    let mut dots = 0;
    let mut x = last as i64;
    while x >= 0 {
        let end = x;
        let (mut low, mut high) = (u32::MAX, 0);
        while x >= 0 && !columns[x as usize].is_empty() {
            for &y in &columns[x as usize] {
                low = low.min(y);
                high = high.max(y);
            }
            x -= 1;
        }
        let wide = end - x;
        if wide > 4 || high - low > 4 {
            break;
        }
        dots += 1;
        while x >= 0 && columns[x as usize].is_empty() {
            x -= 1;
        }
    }
    Some(Ink {
        left: first,
        right: last,
        darkest,
        dots,
    })
}

fn close(a: [u8; 3], b: [u8; 3], by: u8) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= by)
}

/// Blitz's rendering of `page` and Chromium's (`tests/text/<name>.png`);
/// with `CASKADE_TEXT_PAGE`, writes the page there for Chromium.
fn renders(name: &str, page: fn() -> Element) -> (RgbaImage, RgbaImage) {
    let (blitz, html) = render(page);
    if let Some(out) = std::env::var_os("CASKADE_TEXT_PAGE") {
        let out = Path::new(&out);
        std::fs::write(out.join(format!("{name}.html")), html).unwrap();
        std::fs::write(out.join("inter.woff2"), FONT).unwrap();
        blitz.save(out.join(format!("{name}-blitz.png"))).unwrap();
    }
    let path = format!("tests/text/{name}.png");
    let chromium = image::open(Path::new(env!("CARGO_MANIFEST_DIR")).join(&path))
        .unwrap_or_else(|error| panic!("{path} (scripts/text-oracle.sh): {error}"))
        .into_rgba8();
    assert_eq!(chromium.dimensions(), (WIDTH, HEIGHT));
    (blitz, chromium)
}

#[test]
fn cut_titles_and_placeholders_look_as_in_chromium() {
    let (blitz, chromium) = renders("chromium", page);
    let both = |rect: Rect| {
        let blitz = ink(&blitz, rect).expect("Blitz drew the case");
        let chromium = ink(&chromium, rect).expect("Chromium drew the case");
        (blitz, chromium)
    };

    // Cut: an ellipsis, inside the box, where Chromium puts it.
    let (b, c) = both(CUT);
    assert_eq!(c.dots, 3, "Chromium's ellipsis: {c:?}");
    assert_eq!(b.dots, 3, "an ellipsis: {b:?}");
    assert!(
        b.right.abs_diff(c.right) <= 3,
        "Blitz {b:?}, Chromium {c:?}"
    );
    assert!(b.left.abs_diff(c.left) <= 1, "Blitz {b:?}, Chromium {c:?}");

    // Clipped: no ellipsis; the text runs to the box's edge.
    let (b, c) = both(CLIPPED);
    assert_eq!((b.dots, c.dots), (0, 0), "Blitz {b:?}, Chromium {c:?}");
    assert!(b.right >= CLIPPED.2 - 2 && c.right >= CLIPPED.2 - 2);

    // Placeholders: the `::placeholder` colour, or the default grey, the
    // text where Chromium has it.
    for (rect, colour) in [(THEMED, [220, 0, 0]), (DEFAULT, [117, 117, 117])] {
        let (b, c) = both(rect);
        assert!(close(c.darkest, colour, 30), "Chromium {c:?}");
        assert!(close(b.darkest, colour, 30), "Blitz {b:?}");
        assert!(b.left.abs_diff(c.left) <= 1, "Blitz {b:?}, Chromium {c:?}");
        assert!(
            b.right.abs_diff(c.right) <= 4,
            "Blitz {b:?}, Chromium {c:?}"
        );
    }

    // A value hides it: black text, as wide as "Hi".
    let (b, c) = both(FILLED);
    assert!(close(b.darkest, [0, 0, 0], 40), "Blitz {b:?}");
    assert!(close(c.darkest, [0, 0, 0], 40), "Chromium {c:?}");
    assert!(
        b.right.abs_diff(c.right) <= 2,
        "Blitz {b:?}, Chromium {c:?}"
    );
}

#[test]
fn the_apps_classes_cut_titles_and_mute_placeholders_as_in_chromium() {
    let (blitz, chromium) = renders("chromium-app", app_page);
    let both = |rect: Rect| {
        let blitz = ink(&blitz, rect).expect("Blitz drew the case");
        let chromium = ink(&chromium, rect).expect("Chromium drew the case");
        (blitz, chromium)
    };
    let (b, c) = both(APP_TITLE);
    assert_eq!((b.dots, c.dots), (3, 3), "Blitz {b:?}, Chromium {c:?}");
    assert!(
        b.right.abs_diff(c.right) <= 3,
        "Blitz {b:?}, Chromium {c:?}"
    );
    let (b, c) = both(APP_FIELD);
    let muted = fg_subtle();
    assert!(
        close(c.darkest, muted, 30),
        "Chromium {c:?}, the token {muted:?}"
    );
    assert!(
        close(b.darkest, muted, 30),
        "Blitz {b:?}, the token {muted:?}"
    );
    assert!(b.left.abs_diff(c.left) <= 1, "Blitz {b:?}, Chromium {c:?}");
    assert!(
        b.right.abs_diff(c.right) <= 4,
        "Blitz {b:?}, Chromium {c:?}"
    );
}
