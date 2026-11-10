//! BLITZ-0 (#79): Caskade's library on Dioxus Native (Blitz), read straight
//! from caskade-core, to measure against today's webview UI. Throwaway: see
//! README.md for the switches and docs/spikes/blitz-0.md for the results.
#![cfg_attr(all(not(test), target_os = "windows"), windows_subsystem = "windows")]

mod options;

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

use caskade_core::{
    db::LibraryStore,
    model::{GameSummary, ListGames},
};
use dioxus_native::prelude::dioxus_html::geometry::PixelsVector2D;
use dioxus_native::prelude::*;
use options::{Bench, Options};

const CSS: &str = include_str!("../assets/app.css");
/// The webview's grid (components/gameCardLayout.ts).
const ROW_HEIGHT: f64 = 312.0;
const GAP: f64 = 16.0;
const MIN_CARD: f64 = 176.0;
/// Games fetched per query, around what is on screen.
const PAGE: u32 = 100;

static OPTIONS: OnceLock<Options> = OnceLock::new();
static STARTED: OnceLock<Instant> = OnceLock::new();

fn options() -> &'static Options {
    OPTIONS.get().expect("options are read first")
}

/// Milliseconds since the process started, for the logs the benchmarks read.
fn since_start() -> f64 {
    STARTED
        .get()
        .map_or(0.0, |t| t.elapsed().as_secs_f64() * 1000.0)
}

fn mark(what: &str) {
    println!("proto: {:.1} ms {what}", since_start());
}

fn main() {
    STARTED.get_or_init(Instant::now);
    let options = OPTIONS.get_or_init(Options::from_env);
    mark("start");
    if options.core_only {
        core_only(options);
        return;
    }
    let mut window = dioxus_native::WindowAttributes::default()
        .with_title("Caskade (native prototype)")
        .with_surface_size(dioxus_native::LogicalSize::new(
            options.width,
            options.height,
        ));
    // A real-display benchmark stays visible, so nothing covers the window
    // it times, without taking the focus.
    if options.on_top {
        use dioxus_native::winit::window::WindowLevel;
        window = window
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_active(false);
    }
    if let Some((x, y)) = options.position {
        window = window.with_position(dioxus_native::winit::dpi::PhysicalPosition::new(x, y));
    }
    let fonts = fonts();
    let config = dioxus_native::Config::new().with_font_ctx(fonts);
    dioxus_native::launch_cfg(app, vec![], vec![Box::new(config), Box::new(window)]);
}

/// The process without a window: the library opened and the first pages
/// read, as the windowed app does. Its memory is the prototype's baseline
/// before any UI.
fn core_only(options: &Options) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("a tokio runtime");
    runtime.block_on(async {
        let store = LibraryStore::open(&options.database)
            .await
            .expect("the library opens");
        let first = store
            .list_games_for_account(&page_query("", 0), None)
            .await
            .expect("the first page");
        let cards: Vec<Card> = first.items.iter().map(Card::from).collect();
        mark(&format!(
            "core-only: {} of {} games read",
            cards.len(),
            first.total
        ));
        loop {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            std::hint::black_box(&cards);
        }
    });
}

/// System fonts plus the app's three variable fonts (Inter, Oxanium and
/// JetBrains Mono), which the stylesheet names.
fn fonts() -> dioxus_native::FontContext {
    use parley::fontique::Blob;
    let mut fonts = dioxus_native::FontContext::default();
    for data in [
        &include_bytes!("../assets/fonts/inter-latin-wght-normal.woff2")[..],
        &include_bytes!("../assets/fonts/oxanium-latin-wght-normal.woff2")[..],
        &include_bytes!("../assets/fonts/jetbrains-mono-latin-wght-normal.woff2")[..],
    ] {
        let decoded = blitz_dom::util::decode_font_bytes(data).into_owned();
        fonts
            .collection
            .register_fonts(Blob::new(Arc::new(decoded) as _), None);
    }
    fonts
}

fn page_query(query: &str, page: u32) -> ListGames {
    ListGames {
        query: (!query.is_empty()).then(|| query.to_owned()),
        offset: page * PAGE,
        limit: PAGE,
        ..ListGames::default()
    }
}

/// What a card shows.
#[derive(Clone, PartialEq)]
struct Card {
    id: String,
    title: String,
    subtitle: Option<String>,
    cover: Option<String>,
    favorite: bool,
}

impl From<&GameSummary> for Card {
    fn from(game: &GameSummary) -> Self {
        // The 320 px thumbnail, as the webview's `coverGrid`.
        let cover = game.cover.as_ref().and_then(|art| {
            art.thumbnails
                .iter()
                .find(|thumb| thumb.size == 320)
                .map(|thumb| file_url(&options().artwork.join(&thumb.path)))
        });
        // Its platform unless it is a PC game, as `platformLine`.
        let subtitle = match game.platforms.as_slice() {
            [] => None,
            [only] if only == "PC" || only == "Windows" => None,
            [first] => Some(first.clone()),
            [first, rest @ ..] => Some(format!("{first} +{}", rest.len())),
        };
        Self {
            id: game.id.clone(),
            title: game.title.clone(),
            subtitle,
            cover,
            favorite: game.favorite,
        }
    }
}

fn file_url(path: &std::path::Path) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    if path.starts_with('/') {
        format!("file://{path}")
    } else {
        format!("file:///{path}")
    }
}

/// The library as loaded so far: pages of cards by page index.
#[derive(Default)]
struct Loaded {
    pages: HashMap<u32, Rc<Vec<Card>>>,
    pending: HashSet<u32>,
}

fn columns_for(width: f64) -> usize {
    (((width + GAP) / (MIN_CARD + GAP)).floor() as usize).max(1)
}

fn app() -> Element {
    let opts = options();
    let mut store = use_signal(|| None::<LibraryStore>);
    let mut query = use_signal(String::new);
    // Bumped when the query changes, so late pages of the old one are dropped.
    let mut generation = use_signal(|| 0u32);
    let mut total = use_signal(|| 0u64);
    let mut loaded = use_signal(Loaded::default);
    let mut scroll_top = use_signal(|| 0.0f64);
    let mut viewport = use_signal(|| (900.0f64, 600.0f64));
    let mut scroller = use_signal(|| None::<Rc<MountedData>>);
    let mut focused = use_signal(|| None::<usize>);
    let mut spinner = use_signal(|| opts.spinner);
    let first_page_marked = use_hook(|| Rc::new(RefCell::new(false)));

    // Open the library once.
    use_future(move || async move {
        match LibraryStore::open(&options().database).await {
            Ok(opened) => {
                mark("library opened");
                store.set(Some(opened));
            }
            Err(error) => eprintln!("proto: the library didn't open: {error}"),
        }
    });

    let columns = columns_for(viewport().0);
    let rows = (total() as usize).div_ceil(columns);
    let (width, height) = viewport();
    let _ = width;
    let first_row = ((scroll_top() / ROW_HEIGHT).floor() as usize).saturating_sub(1);
    let last_row =
        (((scroll_top() + height) / ROW_HEIGHT).floor() as usize + 1).min(rows.max(1) - 1);

    // Fetch the pages the visible rows need.
    use_effect(move || {
        let Some(store) = store() else { return };
        let gen_now = generation();
        let q = query();
        let columns = columns_for(viewport().0);
        let first = ((scroll_top() / ROW_HEIGHT).floor() as usize).saturating_sub(1) * columns;
        let last = ((((scroll_top() + viewport().1) / ROW_HEIGHT).floor() as usize) + 2) * columns;
        let pages = (first as u32 / PAGE)..=(last as u32 / PAGE);
        for page in pages {
            {
                let current = loaded.peek();
                if current.pages.contains_key(&page) || current.pending.contains(&page) {
                    continue;
                }
                if page > 0 && u64::from(page * PAGE) >= *total.peek() {
                    continue;
                }
            }
            loaded.write().pending.insert(page);
            let store = store.clone();
            let q = q.clone();
            let first_page_marked = first_page_marked.clone();
            spawn(async move {
                let result = store
                    .list_games_for_account(&page_query(&q, page), None)
                    .await;
                if *generation.peek() != gen_now {
                    return;
                }
                match result {
                    Ok(got) => {
                        let cards: Vec<Card> = got.items.iter().map(Card::from).collect();
                        total.set(got.total);
                        let mut state = loaded.write();
                        state.pending.remove(&page);
                        state.pages.insert(page, Rc::new(cards));
                        if !*first_page_marked.borrow() {
                            *first_page_marked.borrow_mut() = true;
                            mark(&format!("first page: {} games in the library", got.total));
                        }
                    }
                    Err(error) => {
                        eprintln!("proto: page {page} failed: {error}");
                        loaded.write().pending.remove(&page);
                    }
                }
            });
        }
    });

    // Pads move the focus between the cards, as the arrow keys do.
    use_future(move || async move {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let _service = caskade_input::InputService::start(move |event| {
            if let caskade_input::InputEvent::Action(action) = event {
                let _ = sender.send(action.action);
            }
        });
        while let Some(action) = receiver.recv().await {
            use caskade_input::Action as A;
            let step = match action {
                A::NavLeft => Some(Move::Left),
                A::NavRight => Some(Move::Right),
                A::NavUp => Some(Move::Up),
                A::NavDown => Some(Move::Down),
                A::PagePrev => Some(Move::PageUp),
                A::PageNext => Some(Move::PageDown),
                _ => None,
            };
            if let Some(step) = step {
                move_focus(step, focused, scroll_top, viewport, total, scroller);
            }
        }
    });

    // Window resizes change the columns.
    dioxus_native::use_window_event(move |event, _| {
        if let dioxus_native::winit::event::WindowEvent::SurfaceResized(_) = event
            && let Some(element) = scroller.peek().clone()
        {
            spawn(async move {
                if let Ok(rect) = element.get_client_rect().await {
                    viewport.set((rect.size.width, rect.size.height));
                }
            });
        }
    });

    // Benchmarks drive the grid themselves: no synthetic input.
    use_future(move || async move {
        let Some(bench) = options().bench.clone() else {
            return;
        };
        run_bench(bench, scroller, scroll_top, viewport, total, spinner).await;
    });

    if opts.debug {
        println!(
            "proto: render scroll_top={:.0} viewport={:?} columns={columns} rows {first_row}..={last_row} of {rows}",
            scroll_top(),
            viewport()
        );
    }
    let rows_to_show: Vec<usize> = if rows == 0 {
        vec![]
    } else {
        (first_row..=last_row).collect()
    };
    let state = loaded.read();
    let card_at = |index: usize| -> Option<Card> {
        let page = state.pages.get(&((index as u32) / PAGE))?;
        page.get(index % PAGE as usize).cloned()
    };
    let ambient = if opts.ambient { "running" } else { "paused" };
    let growing_row = if opts.flex_fix { "height: 0" } else { "" };
    let label = if spinner() {
        "Checking for game updates"
    } else {
        "Ready"
    };

    rsx! {
        style { {CSS} }
        div {
            "data-cs": "app-shell",
            class: "relative flex h-screen flex-col overflow-clip",
            style: "--cs-motion-ambient: {ambient}",
            div { "data-cs": "backdrop", "aria-hidden": "true" }
            div { class: "flex min-h-0 flex-1",
                Sidebar {}
                div { "data-part": "main", class: "relative flex min-w-0 flex-1 flex-col",
                    header {
                        "data-cs": "topbar",
                        class: "relative z-10 flex h-topbar shrink-0 items-center gap-4 pr-4 pl-6",
                        div { "data-cs": "search", class: "w-full max-w-xl",
                            label { class: "flex h-11 items-center gap-3 rounded-control border border-border bg-surface-raised px-4 text-sm",
                                SearchIcon {}
                                input {
                                    "aria-label": "Search your library",
                                    class: "w-full bg-transparent focus:outline-none",
                                    value: "{query}",
                                    oninput: move |event| {
                                        query.set(event.value());
                                        generation += 1;
                                        loaded.set(Loaded::default());
                                        total.set(0);
                                        focused.set(None);
                                        scroll_top.set(0.0);
                                        if let Some(element) = scroller.peek().clone() {
                                            spawn(async move {
                                                let _ = element.scroll(PixelsVector2D::new(0.0, 0.0), ScrollBehavior::Instant).await;
                                            });
                                        }
                                    },
                                }
                            }
                        }
                        div { "data-part": "actions", class: "ml-auto flex items-center gap-4 text-sm text-fg-muted",
                            button {
                                class: "rounded-control border border-border px-3 py-1.5 hover:bg-surface-hover",
                                onclick: move |_| spinner.toggle(),
                                if spinner() { "Stop spinner" } else { "Start spinner" }
                            }
                        }
                    }
                    main {
                        "data-cs": "view",
                        class: "min-h-0 flex-1 overflow-y-auto px-6 pb-8",
                        div { class: "flex min-h-full flex-col",
                            div { class: "flex items-baseline gap-3 pt-2 pb-4",
                                h1 { class: "font-display text-2xl font-semibold", "Library" }
                                span { class: "text-sm text-fg-muted", "{total} games" }
                            }
                            div { class: "flex min-h-96 grow basis-0 gap-2", style: "{growing_row}",
                                section {
                                    "data-cs": "virtual-library",
                                    "data-layout": "grid",
                                    "aria-label": "Library games",
                                    tabindex: "0",
                                    class: "min-w-0 grow overflow-auto overscroll-contain",
                                    onmounted: move |event| {
                                        let element = event.data();
                                        scroller.set(Some(element.clone()));
                                        spawn(async move {
                                            if let Ok(rect) = element.get_client_rect().await {
                                                mark(&format!("grid viewport {:.0}x{:.0}", rect.size.width, rect.size.height));
                                                viewport.set((rect.size.width, rect.size.height));
                                            }
                                        });
                                    },
                                    onscroll: move |event| {
                                        scroll_top.set(event.scroll_top());
                                        let size = (f64::from(event.client_width()), f64::from(event.client_height()));
                                        if size.0 > 0.0 && size != *viewport.peek() {
                                            viewport.set(size);
                                        }
                                    },
                                    onkeydown: move |event| {
                                        let step = match event.key() {
                                            Key::Named(NamedKey::ArrowLeft) => Move::Left,
                                            Key::Named(NamedKey::ArrowRight) => Move::Right,
                                            Key::Named(NamedKey::ArrowUp) => Move::Up,
                                            Key::Named(NamedKey::ArrowDown) => Move::Down,
                                            Key::Named(NamedKey::PageUp) => Move::PageUp,
                                            Key::Named(NamedKey::PageDown) => Move::PageDown,
                                            _ => return,
                                        };
                                        event.prevent_default();
                                        println!("proto: {:.1} ms key", since_start());
                                        move_focus(step, focused, scroll_top, viewport, total, scroller);
                                    },
                                    div {
                                        class: "relative w-full",
                                        style: "height: {rows as f64 * ROW_HEIGHT}px",
                                        for row in rows_to_show {
                                            div {
                                                key: "{columns}-{row}",
                                                "data-part": "virtual-row",
                                                "data-row": "{row}",
                                                class: "absolute top-0 left-0 grid w-full gap-4 pb-4",
                                                style: "height: {ROW_HEIGHT}px; transform: translateY({row as f64 * ROW_HEIGHT}px); grid-template-columns: repeat({columns}, minmax(0, 1fr))",
                                                for col in 0..columns.min(total() as usize - row * columns) {
                                                    {
                                                        let index = row * columns + col;
                                                        match card_at(index) {
                                                            Some(card) => rsx! {
                                                                GameCard { key: "{card.id}", card, focused: focused() == Some(index) }
                                                            },
                                                            None => rsx! {
                                                                div {
                                                                    key: "loading-{index}",
                                                                    "data-part": "placeholder",
                                                                    role: "status",
                                                                    "aria-label": "Loading game",
                                                                    class: "rounded-card border border-border bg-surface-raised",
                                                                }
                                                            },
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            footer {
                "data-cs": "status-bar",
                class: "relative z-10 flex min-h-9 shrink-0 items-center gap-4 border-t border-border bg-surface-raised px-4 py-2 text-xs text-fg-muted",
                output { class: "flex min-w-0 flex-1 items-center gap-2",
                    if spinner() {
                        Spinner {}
                    }
                    span { "data-part": "activity", class: "truncate", "{label}" }
                }
                span { "data-part": "version", class: "shrink-0 font-mono text-fg-subtle", "Caskade native prototype (BLITZ-0)" }
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Move {
    Left,
    Right,
    Up,
    Down,
    PageUp,
    PageDown,
}

/// Moves the focus ring to the next card in `step`'s direction and scrolls
/// it into view.
fn move_focus(
    step: Move,
    mut focused: Signal<Option<usize>>,
    mut scroll_top: Signal<f64>,
    viewport: Signal<(f64, f64)>,
    total: Signal<u64>,
    scroller: Signal<Option<Rc<MountedData>>>,
) {
    let total = *total.peek() as usize;
    if total == 0 {
        return;
    }
    let (width, height) = *viewport.peek();
    let columns = columns_for(width);
    let per_page = ((height / ROW_HEIGHT).floor() as usize).max(1) * columns;
    let top = *scroll_top.peek();
    let from = focused
        .peek()
        .unwrap_or(((top / ROW_HEIGHT).ceil() as usize) * columns)
        .min(total - 1);
    let to = match step {
        Move::Left => from.saturating_sub(1),
        Move::Right => (from + 1).min(total - 1),
        Move::Up => from.checked_sub(columns).unwrap_or(from),
        Move::Down => {
            if from + columns < total {
                from + columns
            } else {
                from
            }
        }
        Move::PageUp => from.saturating_sub(per_page),
        Move::PageDown => (from + per_page).min(total - 1),
    };
    focused.set(Some(to));
    let row_top = (to / columns) as f64 * ROW_HEIGHT;
    let target = if row_top < top {
        Some(row_top)
    } else if row_top + ROW_HEIGHT > top + height {
        Some(row_top + ROW_HEIGHT - height)
    } else {
        None
    };
    if let Some(y) = target {
        // Programmatic scrolls send no scroll event in Blitz yet.
        scroll_top.set(y);
        if let Some(element) = scroller.peek().clone() {
            spawn(async move {
                let _ = element
                    .scroll(PixelsVector2D::new(0.0, y), ScrollBehavior::Instant)
                    .await;
            });
        }
    }
}

/// Scrolls the grid at a steady speed, down and back up, while Blitz logs
/// each frame (feature `log-frame-times`), then exits; or just idles with
/// the spinner on or off and exits.
async fn run_bench(
    bench: Bench,
    scroller: Signal<Option<Rc<MountedData>>>,
    mut scroll_top: Signal<f64>,
    viewport: Signal<(f64, f64)>,
    total: Signal<u64>,
    mut spinner: Signal<bool>,
) {
    while total.peek().eq(&0) || scroller.peek().is_none() {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    tokio::time::sleep(Duration::from_secs_f64(bench.settle_s)).await;
    let element = scroller.peek().clone().expect("the grid is mounted");
    if let Some(on) = bench.spinner {
        spinner.set(on);
    }
    mark("bench: start");
    let start = Instant::now();
    if bench.scroll_px_per_s > 0.0 {
        let (width, height) = *viewport.peek();
        let rows = (*total.peek() as usize).div_ceil(columns_for(width));
        let max = (rows as f64 * ROW_HEIGHT - height).max(1.0);
        let mut ticks = tokio::time::interval(Duration::from_millis(2));
        while start.elapsed().as_secs_f64() < bench.seconds {
            ticks.tick().await;
            let travelled = start.elapsed().as_secs_f64() * bench.scroll_px_per_s;
            // Down to the end, then back up, and again.
            let lap = travelled % (2.0 * max);
            let y = if lap <= max { lap } else { 2.0 * max - lap };
            if (y - *scroll_top.peek()).abs() >= 0.5 {
                scroll_top.set(y);
                let _ = element
                    .scroll(PixelsVector2D::new(0.0, y), ScrollBehavior::Instant)
                    .await;
            }
        }
    } else {
        tokio::time::sleep(Duration::from_secs_f64(bench.seconds)).await;
    }
    mark("bench: end");
    if bench.exit {
        std::process::exit(0);
    }
}

#[component]
fn GameCard(card: Card, focused: bool) -> Element {
    rsx! {
        div {
            "data-cs": "game-card",
            "data-variant": "library",
            "data-part": "grid-item",
            class: "relative h-full min-w-0 rounded-card",
            a {
                "data-focused": focused.then_some("true"),
                class: "group relative flex h-full flex-col overflow-hidden rounded-card border border-border bg-surface-raised outline-none hover:bg-surface-hover data-focused:focus-ring-over",
                div {
                    "data-cs": "game-art",
                    "data-part": "art",
                    "data-source": if card.cover.is_some() { "image" } else { "placeholder" },
                    "aria-hidden": "true",
                    class: "relative min-h-0 w-full flex-1 overflow-hidden",
                    style: "background: linear-gradient(145deg, var(--cs-color-surface-raised), var(--cs-color-surface-base))",
                    if let Some(src) = card.cover.clone() {
                        img { "data-part": "image", src, alt: "", class: "absolute inset-0 size-full object-cover" }
                    }
                }
                div { class: "h-14 min-w-0 shrink-0 px-3 py-2.5",
                    p { "data-part": "title", class: "truncate text-sm font-semibold", "{card.title}" }
                    if let Some(subtitle) = card.subtitle.clone() {
                        p { "data-part": "subtitle", class: "truncate text-xs text-fg-subtle", "{subtitle}" }
                    }
                }
                if card.favorite {
                    span { "data-part": "favorite", class: "absolute top-3 right-3 size-3.5 text-accent-alt", HeartIcon {} }
                }
            }
        }
    }
}

const NAV: [(&str, &str); 4] = [
    ("Home", "M3 10.5 12 3l9 7.5V21H3z"),
    ("Library", "M4 4h6v16H4zM14 4h6v16h-6z"),
    ("Store", "M4 7h16l-1.5 12h-13zM9 7V5a3 3 0 0 1 6 0v2"),
    ("Activity", "M3 12h4l3 8 4-16 3 8h4"),
];

#[component]
fn Sidebar() -> Element {
    let item = "relative flex items-center gap-3 rounded-control px-3 py-2.5 text-sm text-fg-muted hover:translate-x-0.5 hover:bg-surface-hover hover:text-fg data-[status=active]:text-fg";
    rsx! {
        aside {
            "data-cs": "sidebar",
            class: "relative z-10 flex w-sidebar shrink-0 flex-col border-r border-border bg-surface-raised/80 px-4 pb-6",
            div { "data-part": "brand", class: "-mx-4 mb-4 flex h-topbar shrink-0 items-center gap-3 px-6",
                span { class: "font-display text-xl font-semibold tracking-[0.3em]", "CASKADE" }
            }
            nav { "data-part": "nav", class: "flex flex-col gap-1",
                for (label, path) in NAV {
                    a {
                        key: "{label}",
                        "data-part": "nav-item",
                        "data-status": if label == "Library" { "active" } else { "" },
                        class: item,
                        Icon { path }
                        span { "data-part": "label", class: "flex-1", "{label}" }
                    }
                }
            }
            hr { "data-part": "divider", class: "my-4 border-border" }
            nav { "data-part": "nav", class: "flex flex-col gap-1",
                a { "data-part": "nav-item", class: item,
                    Icon { path: "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" }
                    span { "data-part": "label", class: "flex-1", "Settings" }
                }
            }
        }
    }
}

#[component]
fn Icon(path: &'static str) -> Element {
    rsx! {
        svg {
            "data-part": "icon",
            class: "size-5 shrink-0",
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "2",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            path { d: path }
        }
    }
}

#[component]
fn SearchIcon() -> Element {
    rsx! {
        svg {
            "data-part": "icon",
            class: "size-4 shrink-0 text-fg-subtle",
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "2",
            stroke_linecap: "round",
            circle { cx: "11", cy: "11", r: "8" }
            path { d: "m21 21-4.3-4.3" }
        }
    }
}

#[component]
fn HeartIcon() -> Element {
    rsx! {
        svg {
            class: "size-3.5",
            view_box: "0 0 24 24",
            fill: "currentColor",
            stroke: "currentColor",
            stroke_width: "2",
            path { d: "M19 14c1.5-1.5 3-3.2 3-5.5A5.5 5.5 0 0 0 16.5 3c-1.8 0-3 .5-4.5 2-1.5-1.5-2.7-2-4.5-2A5.5 5.5 0 0 0 2 8.5c0 2.3 1.5 4 3 5.5l7 7z" }
        }
    }
}

/// The status bar's spinner (lucide's LoaderCircle) with `animate-worker`.
#[component]
fn Spinner() -> Element {
    rsx! {
        svg {
            "data-part": "spinner",
            "aria-hidden": "true",
            class: "size-3.5 shrink-0 animate-worker text-accent-alt",
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "2",
            stroke_linecap: "round",
            path { d: "M21 12a9 9 0 1 1-6.219-8.56" }
        }
    }
}
