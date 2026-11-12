//! Plugins' drawn UI (#109) in the catalogue: the tree a 0.2 backend
//! draws, and the placeholder for one that failed.

use caskade_ui_theme::anatomy::{Entry, Source};

pub const ANATOMY: [Entry; 2] = [
    Entry {
        parts: &[
            ("text", "Plain text."),
            ("heading", "A heading (`h2`, or `h3` at level 3)."),
            ("paragraph", "A paragraph."),
            ("note", "Secondary text, muted."),
            ("alert", "Something that went wrong, announced."),
            ("stack", "Children in a column or a row (`data-direction`)."),
            ("section", "A labelled part that pad focus moves between."),
            ("stats", "Labelled values (a description list)."),
            ("tile", "One large value with what it is."),
            ("value", "A tile's value."),
            ("label", "A tile's label."),
            ("bars", "A bar chart, with each value as text."),
            ("bar", "One bar."),
            ("table", "A table."),
            ("list", "A list."),
            ("badge", "A short status label (`data-tone`)."),
            ("progress", "A progress bar."),
            ("button", "A button that tells the plugin (a `button`)."),
            (
                "segmented",
                "One of a few options that tells the plugin (a `segmented`).",
            ),
        ],
        contains: &["button", "segmented"],
        ..Entry::new(
            "plugin-ui",
            "PluginTree (apps/native/src/plugins/tree.rs)",
            Source::App,
            "What a plugin draws in a slot or view (plugin API 0.2): Caskade's components as the plugin's backend asked for them.",
        )
    },
    Entry::new(
        "plugin-error",
        "Placeholder (apps/native/src/plugins/mod.rs)",
        Source::App,
        "In place of a plugin's slot or view that failed to draw; it holds focus, so a pad can move on.",
    ),
];
