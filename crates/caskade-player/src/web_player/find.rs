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
/// of the deepest frame inside it that fills it.
pub const FRAMES_SCRIPT: &str = r#"(() => {
  if (window === window.top) {
    const shown = new WeakMap();
    addEventListener("message", (event) => {
      const frame = event.data && event.data.__caskadeFrame;
      if (frame && typeof frame.href === "string" && event.source) shown.set(event.source, frame);
    });
    const filling = (win, width, height, depth) => {
      let best = null;
      const own = shown.get(win);
      if (own && own.width * own.height >= 0.8 * width * height) best = { href: own.href, depth };
      let count = 0;
      try {
        count = depth < 4 ? win.length : 0;
      } catch {}
      for (let i = 0; i < count; i++) {
        let inner = null;
        try {
          inner = win[i];
        } catch {}
        const found = inner && filling(inner, width, height, depth + 1);
        if (found && (!best || found.depth > best.depth)) best = found;
      }
      return best;
    };
    const kinds = "iframe, embed, object, ruffle-embed, ruffle-object, canvas, video";
    Object.defineProperty(window, "__caskadeFind", {
      value: () => {
        const found = [];
        for (const element of document.querySelectorAll(kinds)) {
          const box = element.getBoundingClientRect();
          if (box.width < 320 || box.height < 240) continue;
          const style = getComputedStyle(element);
          if (style.visibility === "hidden" || Number(style.opacity) === 0) continue;
          const kind = element.localName;
          let src = null;
          if (kind === "iframe") {
            const inner = element.contentWindow && filling(element.contentWindow, box.width, box.height, 0);
            src = inner ? inner.href : element.src;
          } else if (kind.endsWith("embed")) src = element.getAttribute("src");
          else if (kind.endsWith("object")) src = element.getAttribute("data");
          try {
            src = src ? new URL(src, document.baseURI).href : null;
          } catch {
            src = null;
          }
          found.push({ kind, src, width: Math.round(box.width), height: Math.round(box.height) });
        }
        return found.sort((a, b) => b.width * b.height - a.width * a.height).slice(0, 8);
      },
    });
  } else {
    const tell = () => {
      try {
        window.top.postMessage(
          { __caskadeFrame: { href: location.href, width: innerWidth, height: innerHeight } },
          "*",
        );
      } catch {}
    };
    tell();
    addEventListener("load", tell);
    addEventListener("resize", tell);
  }
})();"#;

/// Asks the page what it shows; answered as JSON.
pub const FIND: &str = "window.__caskadeFind ? window.__caskadeFind() : []";

/// The game, found in its page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// Its frame's or movie's address.
    pub url: Url,
    /// How big the page showed it.
    pub size: Option<GameSize>,
}

#[derive(Deserialize)]
struct Shown {
    kind: String,
    src: Option<String>,
    width: f64,
    height: f64,
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
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

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
