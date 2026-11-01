//! Flash games (WEB-GAMES-PLAN §6): a `.swf` plays in Ruffle's web build,
//! on a page of the game window's own that fills the window. The page is
//! loaded at the movie's address, so the movie comes from its own origin
//! and its saves stay there.

/// Ruffle's web build. A bundle keeps the copy it was captured with.
const RUFFLE: &str = "https://cdn.jsdelivr.net/npm/@ruffle-rs/ruffle/ruffle.js";

/// The page that plays `movie`.
pub fn page(movie: &str) -> String {
    // A JSON string is a JavaScript one; `<` escaped so it can't end the script.
    let movie = serde_json::Value::from(movie)
        .to_string()
        .replace('<', "\\u003c");
    format!(
        r#"<!doctype html>
<html>
<head>
<meta charset="utf-8">
<style>
html, body {{ margin: 0; width: 100%; height: 100%; overflow: hidden; background: #000; }}
#game {{ display: block; width: 100%; height: 100%; }}
</style>
<script>
window.RufflePlayer = {{ config: {{ autoplay: "on", unmuteOverlay: "hidden", splashScreen: false, letterbox: "on" }} }};
</script>
<script src="{RUFFLE}"></script>
</head>
<body>
<script>
const player = window.RufflePlayer.newest().createPlayer();
player.id = "game";
document.body.append(player);
(player.ruffle?.() ?? player).load({{ url: {movie} }});
</script>
</body>
</html>
"#
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_movie_is_a_string_in_the_page() {
        let page = super::page(r#"https://a.example/g.swf?x="</script><b>"#);
        assert!(
            page.contains(r#"load({ url: "https://a.example/g.swf?x=\"\u003c/script>\u003cb>" })"#)
        );
        assert_eq!(page.matches("</script>").count(), 3);
    }
}
