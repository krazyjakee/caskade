//! Web games (WEB-GAMES-PLAN): pages played in Caskade's game window.

#[cfg(feature = "net")]
mod add;
pub mod block;
pub mod bundle;
#[cfg(feature = "net")]
mod offline;
#[cfg(feature = "net")]
pub mod page;
#[cfg(feature = "net")]
pub mod swf;
#[cfg(all(test, feature = "net"))]
mod tests;

#[cfg(feature = "net")]
pub use add::WebGames;
use caskade_discovery::SourceId;
use url::Url;

/// The library source of web games added by URL; their `external_id` is
/// the page's [`page_url`].
pub const WEB_SOURCE: SourceId = SourceId::new("web");
/// Web games' platform: "Browser (Flash/HTML5)" in `platforms.toml`.
pub const PLATFORM: &str = "browser";

/// The page a web game plays: `http` or `https`, without credentials or a
/// fragment, so the same page gives the same URL.
pub fn page_url(input: &str) -> crate::Result<Url> {
    let input = input.trim();
    let mut url = Url::parse(input)
        .or_else(|error| {
            // `example.com/game`, as typed or pasted without its scheme.
            if matches!(error, url::ParseError::RelativeUrlWithoutBase) && !input.contains("://") {
                Url::parse(&format!("https://{input}"))
            } else {
                Err(error)
            }
        })
        .map_err(|_| crate::Error::Invalid("that isn't a web address".into()))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none_or(str::is_empty) {
        return Err(crate::Error::Invalid(
            "a web game needs an http or https address".into(),
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(crate::Error::Invalid(
            "a web game's address can't contain a user name or password".into(),
        ));
    }
    url.set_fragment(None);
    Ok(url)
}

/// How big a game is when it isn't fullscreen, in CSS pixels: its page's
/// size for its frame or movie, or a movie's stage. The game window opens
/// at it (WEB-GAMES-PLAN §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GameSize {
    pub width: u32,
    pub height: u32,
}

impl GameSize {
    /// The smallest a game is: QVGA. Smaller frames are a page's badges,
    /// teasers and ads (300×250, 728×90, 160×600), not its game.
    pub const MIN_WIDTH: u32 = 320;
    pub const MIN_HEIGHT: u32 = 240;

    /// Big enough to play in: a page's 1×1 tracking frame isn't a game.
    pub fn new(width: u32, height: u32) -> Option<Self> {
        let fits = (Self::MIN_WIDTH..=8192).contains(&width)
            && (Self::MIN_HEIGHT..=8192).contains(&height);
        fits.then_some(Self { width, height })
    }

    /// `640x480`, as [`GameSize::arg`] writes it.
    pub fn parse(text: &str) -> Option<Self> {
        let (width, height) = text.split_once('x')?;
        Self::new(width.parse().ok()?, height.parse().ok()?)
    }

    pub fn arg(self) -> String {
        format!("{}x{}", self.width, self.height)
    }
}

/// Whether `url` is a video player's (a trailer's), never a game's.
pub fn is_video(url: &Url) -> bool {
    const HOSTS: &[&str] = &[
        "youtube.com",
        "youtube-nocookie.com",
        "youtu.be",
        "vimeo.com",
        "dailymotion.com",
        "twitch.tv",
    ];
    let host = url.host_str().unwrap_or_default();
    HOSTS.iter().any(|video| {
        host == *video
            || host
                .strip_suffix(video)
                .is_some_and(|rest| rest.ends_with('.'))
    })
}

/// Whether `url` is a Flash movie, which the game window plays in Ruffle.
pub fn is_swf(url: &Url) -> bool {
    url.path().to_ascii_lowercase().ends_with(".swf")
}

#[cfg(test)]
mod url_tests {
    use super::page_url;

    #[test]
    fn page_urls_are_http_without_credentials_or_fragments() {
        let url = |input: &str| page_url(input).map(String::from).map_err(|e| e.to_string());
        assert_eq!(
            url(" https://Example.com/game#play ").unwrap(),
            "https://example.com/game"
        );
        assert_eq!(url("example.com/game").unwrap(), "https://example.com/game");
        assert_eq!(
            url("http://localhost:8080/").unwrap(),
            "http://localhost:8080/"
        );
        assert!(url("file:///tmp/index.html").is_err());
        assert!(url("javascript:alert(1)").is_err());
        assert!(url("https://user:secret@example.com/").is_err());
        assert!(url("").is_err());
    }
}
