//! BLITZ-6 (#85): the layouts the native views use where today's UI uses
//! what Blitz lacks (container queries) work in Blitz as they do in a
//! browser, and the fixed-positioning bug (Blitz #690) is gone. The table of
//! resolutions: docs/spikes/blitz-6.md.
//!
//! `cargo test -p caskade-native --test design_resolutions`

use blitz_dom::{Document, DocumentConfig, NodeId};
use blitz_traits::shell::{ColorScheme, Viewport};
use dioxus_native::DioxusDocument;
use dioxus_native::prelude::*;

fn open(root: fn() -> Element, width: u32) -> DioxusDocument {
    let mut doc = DioxusDocument::new(
        VirtualDom::new(root),
        DocumentConfig {
            viewport: Some(Viewport::new(width, 800, 1.0, ColorScheme::Dark)),
            ..Default::default()
        },
    );
    doc.initial_build();
    for _ in 0..3 {
        Document::poll(&mut doc, None);
        doc.inner.borrow_mut().resolve(0.0);
    }
    doc
}

fn node(doc: &DioxusDocument, id: &str) -> NodeId {
    doc.inner
        .borrow()
        .get_element_by_id(id)
        .unwrap_or_else(|| panic!("no #{id}"))
}

/// `(x, y, width)` of `id`, in CSS pixels from the viewport.
fn rect(doc: &DioxusDocument, id: &str) -> (f64, f64, f64) {
    let rect = doc
        .inner
        .borrow()
        .get_client_bounding_rect(node(doc, id))
        .unwrap();
    (rect.x, rect.y, rect.width)
}

/// `@xl:grid-cols-2` (two columns from a 36rem container) as an intrinsic
/// grid: `repeat(auto-fit, minmax(17rem, 1fr))` makes two columns once two
/// 17rem tracks and the 1rem gap fit, and one below.
fn auto_fit_grid() -> Element {
    rsx! {
        div { id: "panel", style: "width: 100%; display: grid; gap: 16px; grid-template-columns: repeat(auto-fit, minmax(272px, 1fr));",
            div { id: "a", style: "height: 40px", "a" }
            div { id: "b", style: "height: 40px", "b" }
        }
    }
}

#[test]
fn an_auto_fit_grid_switches_columns_where_the_container_query_did() {
    // 560px of room (two 272px tracks and a 16px gap) or more: side by side.
    let wide = open(auto_fit_grid, 600);
    let (ax, ay, _) = rect(&wide, "a");
    let (bx, by, _) = rect(&wide, "b");
    assert_eq!(ay, by, "two columns at 600px");
    assert!(bx > ax);
    // Less: stacked.
    let narrow = open(auto_fit_grid, 540);
    let (ax, ay, _) = rect(&narrow, "a");
    let (bx, by, _) = rect(&narrow, "b");
    assert_eq!(ax, bx, "one column at 540px");
    assert!(by > ay);
}

/// `@3xl:grid-cols-[minmax(0,1fr)_18rem]` (a side column from a 48rem
/// container) as a wrapping flex row: the side column wraps under the main
/// one when both don't fit.
fn wrapping_row() -> Element {
    rsx! {
        div { id: "row", style: "display: flex; flex-wrap: wrap; gap: 16px;",
            div { id: "main", style: "flex: 1 1 480px; min-width: 0; height: 40px", "main" }
            div { id: "side", style: "flex: 0 1 288px; height: 40px", "side" }
        }
    }
}

#[test]
fn a_wrapping_row_puts_the_side_column_under_when_it_doesnt_fit() {
    let wide = open(wrapping_row, 900);
    assert_eq!(rect(&wide, "main").1, rect(&wide, "side").1);
    let narrow = open(wrapping_row, 700);
    assert!(rect(&narrow, "side").1 > rect(&narrow, "main").1);
    assert_eq!(
        rect(&narrow, "main").2,
        rect(&narrow, "row").2,
        "the main column takes the row"
    );
}

/// StackedTable's table/cards switch (`@6xl`, a 72rem container) as a
/// viewport media query offset by the chrome around the settings panel.
fn stacked_table() -> Element {
    rsx! {
        style { "#table {{ display: block }} @media (min-width: 1500px) {{ #table {{ display: table }} }}" }
        div { id: "table", "rows" }
    }
}

#[test]
fn a_media_query_switches_the_table_layout() {
    let narrow = open(stacked_table, 1280);
    let wide = open(stacked_table, 1600);
    // A block spans the row; a table shrinks to its content.
    let page = rect(&narrow, "table").2;
    assert!(page > 1000.0, "a block below the breakpoint: {page}");
    let table = rect(&wide, "table").2;
    assert!(table < 200.0, "a table from the breakpoint: {table}");
}

/// A bar fixed to the window's bottom stays there while the page scrolls,
/// inside a positioned ancestor (Blitz #690, fixed at the pin by #922).
fn fixed_bar() -> Element {
    rsx! {
        div { style: "position: relative; margin-top: 40px; height: 3000px",
            div { id: "bar", style: "position: fixed; left: 0; bottom: 0; width: 100px; height: 20px" }
        }
    }
}

#[test]
fn a_fixed_bar_stays_on_the_window_while_the_page_scrolls() {
    let doc = open(fixed_bar, 800);
    let before = rect(&doc, "bar");
    assert_eq!((before.0, before.1), (0.0, 780.0));
    doc.inner.borrow_mut().scroll_viewport_by(0.0, 600.0);
    doc.inner.borrow_mut().resolve(0.0);
    assert_eq!(rect(&doc, "bar"), before);
}

/// `position: sticky` (Blitz series 0024, blitz#887): a `sticky top-0`
/// header in a scrolling panel, as the library list and the settings nav
/// have, and a `sticky bottom-2` bar, as the selection bar has.
fn sticky_panel() -> Element {
    rsx! {
        div { id: "scroller", style: "height: 400px; overflow-y: auto; margin: 0",
            div { id: "header", style: "position: sticky; top: 0; height: 40px; z-index: 20", "header" }
            div { style: "height: 2000px" }
            div { id: "bar", style: "position: sticky; bottom: 8px; height: 30px", "bar" }
        }
    }
}

#[test]
fn sticky_headers_and_bars_stay_in_view_while_their_panel_scrolls() {
    let doc = open(sticky_panel, 800);
    let header = rect(&doc, "header").1;
    let scroller = rect(&doc, "scroller").1;
    // The bar's place is at the content's end, out of view: it sits 8px
    // above the panel's bottom.
    assert_eq!(rect(&doc, "bar").1, scroller + 400.0 - 8.0 - 30.0);
    let panel = node(&doc, "scroller");
    doc.inner
        .borrow_mut()
        .scroll_by(panel, 0.0, 600.0, blitz_dom::ScrollBehavior::Instant);
    doc.inner.borrow_mut().resolve(0.0);
    assert_eq!(
        rect(&doc, "header").1,
        header,
        "the header stays at the top"
    );
    assert_eq!(rect(&doc, "bar").1, scroller + 400.0 - 8.0 - 30.0);
}
