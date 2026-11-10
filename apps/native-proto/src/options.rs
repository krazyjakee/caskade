//! The prototype's switches, all from the environment (README.md).
use std::{env, path::PathBuf};

#[derive(Clone, Debug)]
pub struct Options {
    /// The library to read: a copy, never the live one (migrations run on open).
    pub database: PathBuf,
    /// The artwork cache the library's thumbnail paths are relative to.
    pub artwork: PathBuf,
    /// The status bar's spinner runs from the start.
    pub spinner: bool,
    /// The default theme's ambient background animates.
    pub ambient: bool,
    /// `height: 0` on the grid's growing row, the spike's workaround for
    /// BLITZ-2 (#81); `CASKADE_PROTO_FLEX_BUG=1` leaves it out to show the bug.
    pub flex_fix: bool,
    /// Always on top and not focused, for benchmarks on a real display.
    pub on_top: bool,
    /// Where the window opens, in physical pixels.
    pub position: Option<(i32, i32)>,
    pub width: f64,
    pub height: f64,
    /// No window: the library opened and read, for the baseline memory.
    pub core_only: bool,
    pub bench: Option<Bench>,
    /// Logs each render's scroll position and rows.
    pub debug: bool,
    /// Leaves out the theme's backdrop (for measuring what it costs).
    pub no_backdrop: bool,
    /// The spinner turns in this many steps instead of continuously.
    pub spinner_steps: Option<u32>,
    /// Show the theme-animation gallery (assets/gallery.html) instead of the library.
    pub gallery: bool,
}

#[derive(Clone, Debug)]
pub struct Bench {
    /// Wait after the first page before starting.
    pub settle_s: f64,
    pub seconds: f64,
    /// 0 idles instead of scrolling.
    pub scroll_px_per_s: f64,
    /// Turn the spinner on or off as the bench starts.
    pub spinner: Option<bool>,
    pub exit: bool,
}

fn flag(name: &str) -> bool {
    env::var(name).is_ok_and(|v| v == "1" || v == "true")
}

fn number(name: &str, default: f64) -> f64 {
    env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

impl Options {
    pub fn from_env() -> Self {
        let home = env::var_os("CASKADE_PROTO_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                eprintln!("set CASKADE_PROTO_HOME to a copy of a library (README.md)");
                std::process::exit(2)
            });
        let database = env::var_os("CASKADE_PROTO_DB")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("data/caskade/library.sqlite"));
        let artwork = env::var_os("CASKADE_PROTO_ARTWORK")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("cache/caskade/artwork"));
        let bench = env::var("CASKADE_PROTO_BENCH").ok().map(|kind| Bench {
            settle_s: number("CASKADE_PROTO_BENCH_SETTLE", 5.0),
            seconds: number("CASKADE_PROTO_BENCH_SECONDS", 30.0),
            scroll_px_per_s: if kind == "scroll" {
                number("CASKADE_PROTO_BENCH_SPEED", 3000.0)
            } else {
                0.0
            },
            spinner: match kind.as_str() {
                "spinner" => Some(true),
                "idle" => Some(false),
                _ => None,
            },
            exit: !flag("CASKADE_PROTO_BENCH_STAY"),
        });
        Self {
            database,
            artwork,
            spinner: flag("CASKADE_PROTO_SPINNER"),
            ambient: flag("CASKADE_PROTO_AMBIENT"),
            flex_fix: !flag("CASKADE_PROTO_FLEX_BUG"),
            on_top: flag("CASKADE_PROTO_ON_TOP"),
            position: env::var("CASKADE_PROTO_POSITION").ok().and_then(|v| {
                let (x, y) = v.split_once(',')?;
                Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
            }),
            width: number("CASKADE_PROTO_WIDTH", 1600.0),
            height: number("CASKADE_PROTO_HEIGHT", 900.0),
            core_only: env::args().any(|a| a == "--core-only"),
            bench,
            debug: flag("CASKADE_PROTO_DEBUG"),
            no_backdrop: flag("CASKADE_PROTO_NO_BACKDROP"),
            gallery: flag("CASKADE_PROTO_GALLERY"),
            spinner_steps: env::var("CASKADE_PROTO_SPINNER_STEPS")
                .ok()
                .and_then(|v| v.parse().ok()),
        }
    }
}
