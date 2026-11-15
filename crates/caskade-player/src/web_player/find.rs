//! Finding the game in the page it's captured in.
//! Portals build their game's frame with scripts, and some serve it only
//! inside their page (Poki's sitelock), so the capture plays the whole
//! page and looks for the game while it runs. On Done the offline copy
//! starts at the game it found and keeps only what the game loaded.

use caskade_core::web::{GameSize, block, is_swf, is_video};
use serde::Deserialize;
use url::Url;

/// In every frame while capturing. Each frame tells the page its address
/// and size, which the page can't read across origins: a portal's loader
/// often wraps the game in a frame of its own. The page answers
/// `__caskadeFind` with its biggest visible elements a game could be
/// (frames, Flash movies, canvases and videos, at least
/// [`GameSize::MIN_WIDTH`] × [`GameSize::MIN_HEIGHT`]); a frame's address is that
/// of the deepest frame inside it that fills it. HTML Blob frames are traced
/// back to the HTTP response the page read to create them.
pub const FRAMES_SCRIPT: &str = include_str!("find.js");

/// Asks the page what it shows; answered as JSON.
pub const FIND: &str = "window.__caskadeFind ? window.__caskadeFind() : []";

/// The game, found in its page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// Its frame's or movie's address.
    pub url: Url,
    /// How big the page showed it.
    pub size: Option<GameSize>,
    /// The page framed a Blob of an HTTP response: use the response itself,
    /// including when it came from the page's own origin.
    pub from_blob: bool,
}

#[derive(Deserialize)]
struct Shown {
    kind: String,
    src: Option<String>,
    width: f64,
    height: f64,
    #[serde(default, rename = "fromBlob")]
    from_blob: bool,
}

/// The game in `answer`, `FIND`'s: the biggest frame or Flash movie that
/// isn't an ad or a video. `None` when the page draws the game itself (a
/// canvas comes first) or shows nothing that could be one.
pub fn choose(answer: &str) -> Option<Found> {
    let shown: Vec<Shown> = serde_json::from_str(answer).ok()?;
    for element in shown {
        if element.kind == "canvas" {
            return None;
        }
        let Some(url) = element
            .src
            .as_deref()
            .and_then(|src| Url::parse(src).ok())
            .filter(|url| matches!(url.scheme(), "http" | "https"))
        else {
            continue;
        };
        let movie = element.kind != "iframe";
        if element.kind == "video"
            || is_video(&url)
            || url.host_str().is_some_and(block::blocked)
            || (movie && !is_swf(&url))
        {
            continue;
        }
        let side = |pixels: f64| pixels.round() as u32;
        return Some(Found {
            size: GameSize::new(side(element.width), side(element.height)),
            url,
            from_blob: element.from_blob,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_frame_tracking_runs_in_the_pages_javascript() {
        let result = std::process::Command::new("node")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/src/web_player/find.test.mjs"
            ))
            .output()
            .expect("Node.js runs the capture script's regression checks");
        assert!(
            result.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }

    #[test]
    fn a_resolved_blob_uses_its_original_http_document() {
        let answer = r#"[{"kind":"iframe","src":"https://cdn.example/unity/index.html","fromBlob":true,"width":960,"height":540}]"#;
        let found = choose(answer).unwrap();
        assert!(found.from_blob);
        assert_eq!(found.url.as_str(), "https://cdn.example/unity/index.html");
        let unresolved = r#"[{"kind":"iframe","src":"blob:https://portal.example/temporary","width":960,"height":540}]"#;
        assert_eq!(choose(unresolved), None);
    }

    #[test]
    fn the_biggest_frame_that_isnt_an_ad_or_a_video_is_the_game() {
        let answer = r#"[
            {"kind": "video", "src": null, "width": 1280, "height": 720},
            {"kind": "iframe", "src": "https://www.youtube.com/embed/x", "width": 1200, "height": 675},
            {"kind": "iframe", "src": "https://securepubads.g.doubleclick.net/ad", "width": 1000, "height": 600},
            {"kind": "iframe", "src": "about:blank", "width": 900, "height": 600},
            {"kind": "iframe", "src": "https://g1.gdn.poki.com/v2/index.html?country=GB", "width": 836, "height": 470},
            {"kind": "iframe", "src": "https://ads.example/300x250", "width": 300, "height": 250}
        ]"#;
        assert_eq!(
            choose(answer),
            Some(Found {
                url: Url::parse("https://g1.gdn.poki.com/v2/index.html?country=GB").unwrap(),
                size: GameSize::new(836, 470),
                from_blob: false,
            })
        );
    }

    #[test]
    fn a_movie_is_the_game_and_a_canvas_keeps_the_page() {
        let movie = r#"[{"kind": "ruffle-embed", "src": "https://cdn.example/g.swf", "width": 640, "height": 480}]"#;
        assert_eq!(
            choose(movie).map(|found| found.url.to_string()),
            Some("https://cdn.example/g.swf".into())
        );
        let pdf = r#"[{"kind": "embed", "src": "https://cdn.example/manual.pdf", "width": 640, "height": 480}]"#;
        assert_eq!(choose(pdf), None);
        let canvas = r#"[
            {"kind": "canvas", "src": null, "width": 1280, "height": 720},
            {"kind": "iframe", "src": "https://games.example/other/", "width": 640, "height": 480}
        ]"#;
        assert_eq!(choose(canvas), None, "the page draws the game");
        assert_eq!(choose("[]"), None);
        assert_eq!(choose("null"), None);
    }
}
