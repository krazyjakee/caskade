//! The library's scroll benchmark (#94), as the webview's
//! `apps/desktop/tests/library-scroll.py`: `CASKADE_LIBRARY_BENCH=<report>`
//! (debug builds, or the `bench` feature, in fixtures mode) fills the window
//! with the virtual grid (or list: `CASKADE_LIBRARY_BENCH_LAYOUT=list`) over
//! the fixture library, scrolls it from top to bottom a step per frame,
//! writes what it measured as JSON and quits. `scripts/library-scroll.sh`
//! runs it on a private desktop.
//!
//! Frame times are the intervals between the window's redraws while it
//! scrolls; on Xvfb they measure the renderer on the CPU, not a display's
//! refresh rate (that's a real-machine check, #120).

use super::grid::VirtualLibrary;
use super::window::Layout;
use crate::data::sleep;
use crate::input::use_input;
use blitz_dom::{LocalName, NodeId, ScrollBehavior};
use caskade_core::model::{GameFilter, GameSort, ListGames};
use dioxus_native::NodeHandle;
use dioxus_native::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Steps from top to bottom.
const STEPS: usize = 240;

fn report_path() -> Option<std::path::PathBuf> {
    if !cfg!(any(debug_assertions, feature = "bench")) {
        return None;
    }
    std::env::var_os("CASKADE_LIBRARY_BENCH").map(Into::into)
}

/// The benchmark's screen when it was asked for, in place of the views.
pub fn screen() -> Option<Element> {
    report_path()?;
    Some(rsx! {
        Bench {}
    })
}

fn layout() -> Layout {
    std::env::var("CASKADE_LIBRARY_BENCH_LAYOUT").map_or(Layout::Grid, |name| Layout::parse(&name))
}

/// What the benchmark measured.
#[derive(Default)]
struct Measured {
    /// Redraw times while scrolling.
    redraws: Vec<Instant>,
    scrolling: bool,
    mounted_max: usize,
}

#[component]
fn Bench() -> Element {
    // Until the fixture library opens: DataRoot mounts this again then.
    if try_consume_context::<crate::data::DataSource>().is_none() {
        return rsx! {};
    }
    let input = use_input();
    let layout = layout();
    let measured: Rc<RefCell<Measured>> = use_hook(Rc::default);
    let mut root = use_signal(|| None::<NodeHandle>);
    {
        let measured = measured.clone();
        dioxus_native::use_window_event(move |event, _| {
            if matches!(
                event,
                dioxus_native::winit::event::WindowEvent::RedrawRequested
            ) {
                let mut measured = measured.borrow_mut();
                if measured.scrolling {
                    measured.redraws.push(Instant::now());
                }
            }
        });
    }
    use_hook(move || {
        spawn(async move {
            // The grid, with its first page.
            let scroller = loop {
                sleep(Duration::from_millis(100)).await;
                let found = root.peek().clone().and_then(|handle| {
                    let doc = handle.doc();
                    let scroller = crate::input::find_attr(
                        &doc,
                        handle.node_id(),
                        "data-cs",
                        "virtual-library",
                    )?;
                    crate::input::find_attr(&doc, scroller, "data-index", "0")
                        .map(|_| (handle.clone(), scroller))
                });
                if let Some(found) = found {
                    break found;
                }
            };
            sleep(Duration::from_secs(1)).await;
            let (handle, node) = scroller;
            let content = handle
                .doc()
                .get_node(node)
                .map_or(0.0, |node| f64::from(node.scroll_height()));
            measured.borrow_mut().scrolling = true;
            let started = Instant::now();
            for step in 0..=STEPS {
                let top = content * step as f64 / STEPS as f64;
                handle
                    .doc_mut()
                    .scroll_to(node, 0.0, top, ScrollBehavior::Instant);
                input.scrolled_by_code();
                sleep(Duration::from_millis(16)).await;
                let mounted = count(&handle.doc(), node);
                let mut measured = measured.borrow_mut();
                measured.mounted_max = measured.mounted_max.max(mounted);
            }
            let elapsed = started.elapsed();
            measured.borrow_mut().scrolling = false;
            write_report(&measured.borrow(), layout, elapsed);
            crate::launch::ask(crate::launch::Request::Quit);
        });
    });
    let query = ListGames {
        filter: GameFilter::All,
        sort: GameSort::Title,
        ..ListGames::default()
    };
    rsx! {
        div {
            style: "display: flex; flex-direction: column; height: 100vh; padding: 0 24px;",
            onmounted: move |event| root.set(event.data().downcast::<NodeHandle>().cloned()),
            VirtualLibrary { query, layout }
        }
    }
}

/// The games mounted under `scroller`.
fn count(doc: &blitz_dom::BaseDocument, scroller: NodeId) -> usize {
    let mut count = 0;
    let mut stack = vec![scroller];
    while let Some(at) = stack.pop() {
        let Some(node) = doc.get_node(at) else {
            continue;
        };
        if node.attr(LocalName::from("data-index")).is_some() {
            count += 1;
        }
        stack.extend(node.children.iter().copied());
    }
    count
}

fn write_report(measured: &Measured, layout: Layout, elapsed: Duration) {
    let Some(path) = report_path() else {
        return;
    };
    let mut intervals: Vec<f64> = measured
        .redraws
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).as_secs_f64() * 1000.0)
        .collect();
    intervals.sort_by(f64::total_cmp);
    let percentile = |p: f64| {
        intervals
            .get(((intervals.len() as f64 - 1.0) * p).round() as usize)
            .copied()
            .unwrap_or(0.0)
    };
    let report = serde_json::json!({
        "layout": layout.name(),
        "steps": STEPS,
        "frames": measured.redraws.len(),
        "frameMsP50": percentile(0.5),
        "frameMsP95": percentile(0.95),
        "frameMsMax": intervals.last().copied().unwrap_or(0.0),
        "mountedMax": measured.mounted_max,
        "seconds": elapsed.as_secs_f64(),
    });
    if let Err(error) = std::fs::write(&path, report.to_string()) {
        tracing::warn!(%error, "the benchmark's report wasn't written");
    }
    tracing::info!(%report, "library scroll benchmark");
}
