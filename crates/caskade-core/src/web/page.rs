//! What a game's page says about it: title, description and pictures.

use super::GameSize;
use scraper::{Html, Selector};
use serde_json::Value;
use url::Url;

/// Where a picture of the game came from, the most dedicated first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    /// The page's own preview: Open Graph, Twitter cards, `image_src`,
    /// microdata and JSON-LD.
    Preview,
    /// The page's oEmbed answer (`thumbnail_url`).
    Embed,
    /// The web app manifest: its icons, then its screenshots.
    App,
    /// The page's icons.
    Icon,
}

/// A picture the page offers, with the size it says it is, when it says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    pub url: Url,
    pub source: Source,
    pub size: Option<(u32, u32)>,
}

impl Picture {
    /// Said to be at least half as wide again as it is tall: a banner, for
    /// the game page's hero rather than a tile.
    pub fn wide(&self) -> bool {
        self.size
            .is_some_and(|(width, height)| u64::from(width) * 2 >= u64::from(height) * 3)
    }

    /// Said to be square, or near enough for a tile.
    pub fn square(&self) -> bool {
        self.size.is_some_and(|(width, height)| {
            let (long, short) = (width.max(height), width.min(height));
            short > 0 && u64::from(long) * 10 <= u64::from(short) * 12
        })
    }
}

/// Why a store's game page has no game to play in the game window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unplayable {
    /// It has to be bought first (an itch.io page with Buy Now and no
    /// game): what it shows is its store listing and trailer.
    MustBuy,
    /// It's a download, not a browser game.
    Download,
}

impl Unplayable {
    /// Why, for people.
    pub fn message(self) -> String {
        match self {
            Self::MustBuy => crate::tr!(
                "this game has to be bought before it can be played: its page shows only the store listing, not the game"
            ),
            Self::Download => {
                crate::tr!("this game doesn't play in a browser: its page offers it as a download")
            }
        }
    }
}

/// The game's details as its page gives them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageInfo {
    pub title: Option<String>,
    pub description: Option<String>,
    /// Pictures to try in order: by [`Source`], then as each source ranks
    /// them.
    pub pictures: Vec<Picture>,
    /// The page the game itself runs in, when the page embeds it: what an
    /// offline bundle keeps, without the site around it.
    pub frame: Option<Url>,
    /// How big the page shows the frame, when it says.
    pub frame_size: Option<GameSize>,
    /// Why the page has no game to play, when it's a store's page that
    /// says so.
    pub unplayable: Option<Unplayable>,
    /// The page's web app manifest, for pictures when its own are missing.
    pub manifest: Option<Url>,
    /// The page's oEmbed endpoint (JSON), likewise.
    pub oembed: Option<Url>,
}

impl PageInfo {
    /// Whether the page has a picture made to present it: anything but its
    /// icons.
    pub fn has_preview(&self) -> bool {
        self.pictures
            .iter()
            .any(|picture| picture.source < Source::App)
    }

    /// Adds pictures from another source, keeping the order by source.
    pub fn add_pictures(&mut self, pictures: impl IntoIterator<Item = Picture>) {
        for picture in pictures {
            if !self.pictures.iter().any(|kept| kept.url == picture.url) {
                self.pictures.push(picture);
            }
        }
        // Stable: each source keeps its own order.
        self.pictures.sort_by_key(|picture| picture.source);
    }
}

/// Reads `html`, whose links are relative to `base`.
pub fn parse(html: &str, base: &Url) -> PageInfo {
    let document = Html::parse_document(html);
    let meta = |names: &[&str]| {
        let selector = Selector::parse("meta[content]").expect("valid selector");
        names.iter().find_map(|name| {
            document
                .select(&selector)
                .find(|meta| {
                    [
                        meta.attr("property"),
                        meta.attr("name"),
                        meta.attr("itemprop"),
                    ]
                    .into_iter()
                    .flatten()
                    .any(|key| key.eq_ignore_ascii_case(name))
                })
                .and_then(|meta| meta.attr("content"))
                .map(str::trim)
                .filter(|content| !content.is_empty())
        })
    };
    let title = meta(&["og:title", "twitter:title"])
        .map(str::to_owned)
        .or_else(|| {
            let selector = Selector::parse("title").expect("valid selector");
            document
                .select(&selector)
                .next()
                .map(|title| title.text().collect::<String>())
        })
        .map(|title| clean_title(&title))
        .filter(|title| !title.is_empty());
    let description = meta(&["og:description", "description", "twitter:description"])
        .map(|text| text.split_whitespace().collect::<Vec<_>>().join(" "));

    let og_size = (|| {
        let side = |name| meta(&[name])?.parse::<u32>().ok().filter(|side| *side > 0);
        Some((side("og:image:width")?, side("og:image:height")?))
    })();
    let mut pictures = Vec::new();
    let mut preview = |href: &str, size| {
        if let Some(url) = picture_url(base, href) {
            pictures.push(Picture {
                url,
                source: Source::Preview,
                size,
            });
        }
    };
    for name in ["og:image:secure_url", "og:image", "og:image:url"] {
        if let Some(href) = meta(&[name]) {
            preview(href, og_size);
        }
    }
    for name in ["twitter:image", "twitter:image:src", "image"] {
        if let Some(href) = meta(&[name]) {
            preview(href, None);
        }
    }
    for href in links(&document, |rel| rel == "image_src") {
        preview(&href, None);
    }
    for href in json_ld(&document) {
        preview(&href, None);
    }
    let unplayable = itch_unplayable(&document);
    let frame = game_frame(&document, base).filter(|_| unplayable.is_none());
    let mut info = PageInfo {
        title,
        description,
        pictures: Vec::new(),
        frame: frame.as_ref().map(|(url, _)| url.clone()),
        frame_size: frame.and_then(|(_, size)| size),
        unplayable,
        manifest: links(&document, |rel| rel == "manifest")
            .first()
            .and_then(|href| base.join(href).ok()),
        oembed: {
            let selector =
                Selector::parse("link[rel~=alternate][href][type]").expect("valid selector");
            document
                .select(&selector)
                .find(|link| {
                    link.attr("type")
                        .is_some_and(|kind| kind.eq_ignore_ascii_case("application/json+oembed"))
                })
                .and_then(|link| base.join(link.attr("href")?.trim()).ok())
        },
    };
    info.add_pictures(pictures);
    info.add_pictures(icons(&document, base));
    // Last, the site's favicon, which browsers find there unlinked.
    if let Ok(favicon) = base.join("/favicon.ico") {
        info.add_pictures([Picture {
            url: favicon,
            source: Source::Icon,
            size: None,
        }]);
    }
    info
}

/// The `href`s of the page's `<link>`s whose `rel` has a value `wanted`
/// accepts.
fn links(document: &Html, wanted: impl Fn(&str) -> bool) -> Vec<String> {
    let selector = Selector::parse("link[rel][href]").expect("valid selector");
    document
        .select(&selector)
        .filter(|link| {
            link.attr("rel").is_some_and(|rel| {
                rel.split_whitespace()
                    .any(|rel| wanted(&rel.to_ascii_lowercase()))
            })
        })
        .filter_map(|link| Some(link.attr("href")?.trim().to_owned()))
        .collect()
}

/// Pictures in the page's JSON-LD (`image`, `thumbnailUrl`, `screenshot`),
/// leaving out the publisher's, author's and brand's (their logos and
/// avatars).
fn json_ld(document: &Html) -> Vec<String> {
    let selector = Selector::parse("script[type]").expect("valid selector");
    let mut found = Vec::new();
    for script in document.select(&selector) {
        if !script
            .attr("type")
            .is_some_and(|kind| kind.trim().eq_ignore_ascii_case("application/ld+json"))
        {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<Value>(&script.text().collect::<String>()) {
            ld_pictures(&value, &mut found, 0);
        }
    }
    found
}

fn ld_pictures(value: &Value, found: &mut Vec<String>, depth: usize) {
    if depth > 8 {
        return;
    }
    match value {
        Value::Array(items) => {
            for item in items {
                ld_pictures(item, found, depth + 1);
            }
        }
        Value::Object(object) => {
            for (key, value) in object {
                match key.as_str() {
                    "image" | "thumbnailUrl" | "screenshot" => ld_urls(value, found),
                    "publisher" | "author" | "brand" | "creator" | "logo" => {}
                    _ => ld_pictures(value, found, depth + 1),
                }
            }
        }
        _ => {}
    }
}

/// An image value: a URL, an `ImageObject` (`url`, `contentUrl`), or a list.
fn ld_urls(value: &Value, found: &mut Vec<String>) {
    match value {
        Value::String(url) => found.push(url.clone()),
        Value::Array(items) => items.iter().for_each(|item| ld_urls(item, found)),
        Value::Object(object) => {
            if let Some(Value::String(url)) = object.get("url").or_else(|| object.get("contentUrl"))
            {
                found.push(url.clone());
            }
        }
        _ => {}
    }
}

/// The pictures in a web app manifest: its square icons, largest first,
/// then its other icons and its screenshots. Monochrome icons are outlines,
/// not pictures.
pub fn manifest(json: &str, base: &Url) -> Vec<Picture> {
    let Ok(Value::Object(manifest)) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    let mut pictures: Vec<Picture> = ["icons", "screenshots"]
        .iter()
        .filter_map(|key| manifest.get(*key)?.as_array())
        .flatten()
        .filter(|image| {
            !image
                .get("purpose")
                .and_then(Value::as_str)
                .is_some_and(|purpose| purpose.split_whitespace().all(|part| part == "monochrome"))
        })
        .filter_map(|image| {
            let url = picture_url(base, image.get("src")?.as_str()?)?;
            let size = image.get("sizes").and_then(Value::as_str).and_then(sizes);
            Some(Picture {
                url,
                source: Source::App,
                size,
            })
        })
        .collect();
    // Square ones first, largest first; then the rest as listed.
    pictures.sort_by_key(|picture| {
        let largest = picture.size.map_or(0, |(width, height)| width.max(height));
        (
            !picture.square(),
            std::cmp::Reverse(if picture.square() { largest } else { 0 }),
        )
    });
    pictures
}

/// The thumbnail in an oEmbed answer.
pub fn oembed(json: &str, base: &Url) -> Option<Picture> {
    let answer: Value = serde_json::from_str(json).ok()?;
    let url = picture_url(base, answer.get("thumbnail_url")?.as_str()?)?;
    let side = |key| {
        answer
            .get(key)?
            .as_u64()
            .and_then(|side| u32::try_from(side).ok())
    };
    Some(Picture {
        url,
        source: Source::Embed,
        size: side("thumbnail_width").zip(side("thumbnail_height")),
    })
}

/// The page's icons, largest first. `mask-icon`s are monochrome outlines.
fn icons(document: &Html, base: &Url) -> Vec<Picture> {
    let selector = Selector::parse("link[rel][href]").expect("valid selector");
    let mut icons: Vec<(u32, Picture)> = document
        .select(&selector)
        .filter_map(|link| {
            let rel = link.attr("rel")?.to_ascii_lowercase();
            let rels: Vec<&str> = rel.split_whitespace().collect();
            let apple = rels.iter().any(|rel| rel.starts_with("apple-touch-icon"));
            if !apple && !rels.contains(&"icon") {
                return None;
            }
            let declared = link.attr("sizes").and_then(sizes);
            // Apple's touch icon is 180 px unless it says otherwise; a
            // plain icon without sizes is usually a 16 or 32 px favicon.
            let rank = declared.map_or(if apple { 180 } else { 16 }, |(width, height)| {
                width.min(height)
            });
            let url = picture_url(base, link.attr("href")?)?;
            Some((
                rank,
                Picture {
                    url,
                    source: Source::Icon,
                    size: declared,
                },
            ))
        })
        .collect();
    icons.sort_by_key(|(rank, _)| std::cmp::Reverse(*rank));
    icons.into_iter().map(|(_, picture)| picture).collect()
}

/// The largest size in a `sizes` attribute or value (`"32x32 192x192"`).
fn sizes(text: &str) -> Option<(u32, u32)> {
    text.split_whitespace()
        .filter_map(|size| {
            let (width, height) = size
                .to_ascii_lowercase()
                .split_once('x')
                .map(|(w, h)| (w.parse::<u32>().ok(), h.parse::<u32>().ok()))?;
            Some((width?, height?))
        })
        .max_by_key(|(width, height)| u64::from(*width) * u64::from(*height))
}

/// An image link as an absolute `http(s)` URL. SVG is skipped: the artwork
/// cache reads PNG, JPEG, WebP and ICO.
fn picture_url(base: &Url, href: &str) -> Option<Url> {
    let url = base.join(href.trim()).ok()?;
    let svg = url.path().to_ascii_lowercase().ends_with(".svg");
    (matches!(url.scheme(), "http" | "https") && !svg).then_some(url)
}

/// The game's frame and the size the page gives it: itch.io's (kept as
/// HTML in `data-iframe` until the player presses Run game), else the
/// page's Flash movie, else its largest iframe that says it's big enough
/// for a game ([`GameSize::new`]), else its only iframe. A lazy iframe
/// (Armor Games') gives its address in `data-src` and its size in `style`.
fn game_frame(document: &Html, base: &Url) -> Option<(Url, Option<GameSize>)> {
    let iframe = Selector::parse("iframe[src], iframe[data-src]").expect("valid selector");
    let placeholder = Selector::parse("[data-iframe]").expect("valid selector");
    let frame_url = |src: &str| {
        let url = base.join(src).ok()?;
        matches!(url.scheme(), "http" | "https").then_some(url)
    };
    if let Some(found) = document.select(&placeholder).find_map(|element| {
        let fragment = Html::parse_fragment(element.attr("data-iframe")?);
        let frame = fragment.select(&iframe).next()?;
        Some((frame_url(iframe_src(frame)?), element_size(frame)))
    }) {
        return Some((found.0?, found.1));
    }
    if let Some(movie) = flash_movie(document, base) {
        return Some(movie);
    }
    let frames: Vec<_> = document
        .select(&iframe)
        .filter(|frame| {
            iframe_src(*frame)
                .and_then(frame_url)
                .is_some_and(|url| !super::is_video(&url))
        })
        .collect();
    let largest = frames
        .iter()
        .filter_map(|frame| Some((element_size(*frame)?, *frame)))
        .max_by_key(|(size, _)| u64::from(size.width) * u64::from(size.height));
    if let Some((size, frame)) = largest {
        return Some((frame_url(iframe_src(frame)?)?, Some(size)));
    }
    // The only frame, unless the page says it's too small to be the game
    // (totaljerkface.com's 200×150 teaser beside the game its script adds).
    let [frame] = frames[..] else {
        return None;
    };
    let fits = |name, min| side(frame, name).is_none_or(|side| side >= min);
    if !(fits("width", GameSize::MIN_WIDTH) && fits("height", GameSize::MIN_HEIGHT)) {
        return None;
    }
    Some((frame_url(iframe_src(frame)?)?, None))
}

/// Why an itch.io game page has no game: it embeds its HTML5 games
/// (`data-iframe`, or the loaded `.html_embed_widget`), and a page without
/// one sells or offers a download instead.
fn itch_unplayable(document: &Html) -> Option<Unplayable> {
    let page = Selector::parse(r#"body[data-host="itch.io"][data-page_name="view_game"]"#)
        .expect("valid selector");
    let game = Selector::parse("[data-iframe], .html_embed_widget, iframe#game_drop")
        .expect("valid selector");
    let buy = Selector::parse(".buy_btn").expect("valid selector");
    if document.select(&page).next().is_none() || document.select(&game).next().is_some() {
        return None;
    }
    Some(if document.select(&buy).next().is_some() {
        Unplayable::MustBuy
    } else {
        Unplayable::Download
    })
}

/// An element's width or height in pixels, as an attribute or in its
/// inline `style`; not a percentage.
fn side(element: scraper::ElementRef, name: &str) -> Option<u32> {
    element
        .attr(name)
        .or_else(|| style_value(element.attr("style")?, name))
        .and_then(|value| value.trim_end_matches("px").trim().parse::<u32>().ok())
}

/// The size the page gives an element, when it's big enough to be a game.
fn element_size(element: scraper::ElementRef) -> Option<GameSize> {
    GameSize::new(side(element, "width")?, side(element, "height")?)
}

/// Where an iframe loads: a lazy one keeps it in `data-src` until it's
/// shown, often with a blank `src` meanwhile.
fn iframe_src<'a>(frame: scraper::ElementRef<'a>) -> Option<&'a str> {
    let frame = frame.value();
    frame
        .attr("data-src")
        .or_else(|| frame.attr("src"))
        .map(str::trim)
        .filter(|src| !src.is_empty() && *src != "about:blank")
}

/// `name`'s value in an inline `style`: `width:960px` → `960px`.
fn style_value<'a>(style: &'a str, name: &str) -> Option<&'a str> {
    style.split(';').find_map(|declaration| {
        let (property, value) = declaration.split_once(':')?;
        property
            .trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim())
    })
}

/// The page's Flash movie, an `<embed>`'s or `<object>`'s `.swf`, and the
/// size the page gives it (a `<param>`'s is its `<object>`'s).
fn flash_movie(document: &Html, base: &Url) -> Option<(Url, Option<GameSize>)> {
    let selector = Selector::parse("embed[src], object[data], object param[name][value]")
        .expect("valid selector");
    document.select(&selector).find_map(|element| {
        let (src, sized) = match element.value().name() {
            "embed" => (element.attr("src"), element),
            "object" => (element.attr("data"), element),
            _ => (
                element
                    .attr("name")
                    .filter(|name| name.eq_ignore_ascii_case("movie"))
                    .and(element.attr("value")),
                element.parent().and_then(scraper::ElementRef::wrap)?,
            ),
        };
        let url = base.join(src?.trim()).ok()?;
        (matches!(url.scheme(), "http" | "https") && super::is_swf(&url))
            .then(|| (url, element_size(sized)))
    })
}

/// A page title without the site's name or slogan after it:
/// `Penalty Shooters 2 🕹️ Play on CrazyGames` → `Penalty Shooters 2`,
/// `Celeste Classic by Maddy | itch.io` → `Celeste Classic by Maddy`.
pub fn clean_title(raw: &str) -> String {
    let title = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut title = title.as_str();
    for separator in [" | ", " - ", " – ", " — ", " · ", " :: ", " » "] {
        if let Some((first, _)) = title.split_once(separator)
            && !first.trim().is_empty()
        {
            title = first;
        }
    }
    for suffix in [" Play on ", " Play Online", " - Play "] {
        if let Some(index) = title.find(suffix)
            && index > 0
        {
            title = &title[..index];
        }
    }
    title
        .trim_end_matches(|c: char| {
            !(c.is_alphanumeric() || matches!(c, ')' | ']' | '!' | '?' | '\'' | '"' | '.'))
        })
        .trim()
        .to_owned()
}
