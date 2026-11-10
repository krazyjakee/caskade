//! Blitz layouts that once differed from Chromium's, kept as Chromium lays
//! them out (numbers from headless Chromium, `docs/spikes/blitz.md`).

use blitz_dom::DocumentConfig;
use blitz_traits::shell::{ColorScheme, Viewport};
use dioxus_native::DioxusDocument;
use dioxus_native::prelude::*;

/// The heights of the elements with these ids, laid out in a 1280×800
/// window.
fn heights(app: fn() -> Element, ids: &[&str]) -> Vec<f64> {
    let mut doc = DioxusDocument::new(
        VirtualDom::new(app),
        DocumentConfig {
            viewport: Some(Viewport::new(1280, 800, 1.0, ColorScheme::Light)),
            ..Default::default()
        },
    );
    doc.initial_build();
    blitz_dom::Document::poll(&mut doc, None);
    let mut inner = doc.inner.borrow_mut();
    inner.resolve(0.0);
    ids.iter()
        .map(|id| {
            let mut found = None;
            inner.visit(|node_id, node| {
                if node.attr(blitz_dom::LocalName::from("id")) == Some(*id) {
                    found = Some(node_id);
                }
            });
            let node = found.unwrap_or_else(|| panic!("no #{id}"));
            inner.get_client_bounding_rect(node).unwrap().height
        })
        .collect()
}

/// #81: an auto-height column (`min-height: 100%`) whose growing row has
/// `flex-basis: 0` takes its height from that basis, not from the row's
/// content, so the scroller inside stays the view's height.
fn growing_row() -> Element {
    rsx! {
        div { id: "view", style: "height: 600px; overflow-y: auto;",
            div { id: "column", style: "display: flex; flex-direction: column; min-height: 100%;",
                div { id: "row", style: "display: flex; flex-grow: 1; flex-basis: 0; min-height: 384px;",
                    div { id: "scroller", style: "flex-grow: 1; overflow: auto;",
                        div { style: "height: 55000px;" }
                    }
                }
            }
        }
    }
}

#[test]
fn a_column_sizes_from_a_growing_rows_flex_basis_not_its_content() {
    // Chromium: 600, 600, 600.
    assert_eq!(
        heights(growing_row, &["column", "row", "scroller"]),
        [600.0, 600.0, 600.0]
    );
}
