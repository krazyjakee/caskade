//! What screen readers get, as text (#83, #93): Blitz's AccessKit tree, one
//! line per node with its role, name, states and actions, children
//! indented. One format for the accessibility test (`tests/accessibility.rs`
//! includes this file) and the components' tests (`ui`).
//!
//! Text that names its parent is folded into the name; other text (a live
//! region's) is a quoted line. Wrappers with nothing to hear (a generic
//! container, unknown or window node with no label and no live setting) add
//! nothing. Ids and bounds (fonts) are left out.

#![allow(dead_code, reason = "each user takes what it needs")]

use accesskit::Action;
use blitz_dom::{BaseDocument, NodeId};
use std::fmt::Write as _;

/// The tree as text: one line per node, children indented. Text that
/// names its parent is folded into the name; other text (a live
/// region's) is a quoted line. Ids and bounds (fonts) are left out.
pub fn snapshot(doc: &BaseDocument) -> String {
    snapshot_from(doc, None)
}

/// [`snapshot`] of the subtree at `node` (a Blitz node): one component's.
pub fn snapshot_of(doc: &BaseDocument, node: NodeId) -> String {
    snapshot_from(doc, Some(node))
}

fn snapshot_from(doc: &BaseDocument, start: Option<NodeId>) -> String {
    let update = doc.build_accessibility_tree();
    let nodes: std::collections::HashMap<_, _> = update.nodes.iter().cloned().collect();
    let naming: std::collections::HashSet<_> = update
        .nodes
        .iter()
        .flat_map(|(_, node)| node.labelled_by().iter().copied())
        .collect();
    // Text inside an element that names something is that name too.
    let parents: std::collections::HashMap<_, _> = update
        .nodes
        .iter()
        .flat_map(|(id, node)| node.children().iter().map(move |child| (*child, *id)))
        .collect();
    let names = |id: &accesskit::NodeId| {
        naming.contains(id)
            || parents
                .get(id)
                .is_some_and(|parent| naming.contains(parent))
    };
    let mut out = String::new();
    let root = match start {
        Some(node) => accesskit::NodeId(node.as_u64()),
        None => update.tree.as_ref().unwrap().root,
    };
    let mut stack = vec![(root, 0usize)];
    while let Some((id, depth)) = stack.pop() {
        let node = &nodes[&id];
        // What screen readers skip (`aria-hidden`), with what's inside it.
        if node.is_hidden() {
            continue;
        }
        let role = format!("{:?}", node.role());
        // Text is a label without a tag.
        if node.role() == accesskit::Role::Label && node.html_tag().is_none() {
            let text = node.value().unwrap_or_default().trim();
            if !names(&id) && !text.is_empty() {
                let _ = writeln!(out, "{}{text:?}", "  ".repeat(depth));
            }
            continue;
        }
        // Wrappers without semantics add depth but nothing to hear.
        let quiet = matches!(role.as_str(), "GenericContainer" | "Unknown" | "Window")
            && node.label().is_none()
            && node.live().is_none();
        if !quiet {
            let mut line = format!("{}{role}", "  ".repeat(depth));
            let text: Vec<_> = node
                .labelled_by()
                .iter()
                .filter_map(|id| nodes.get(id))
                .filter_map(|node| node.value())
                .collect();
            let name = node
                .label()
                .map(str::to_owned)
                .or_else(|| (!text.is_empty()).then(|| text.join(" ").trim().to_owned()));
            if let Some(name) = name.filter(|name| !name.is_empty()) {
                let _ = write!(line, " {name:?}");
            }
            let mut states = Vec::new();
            macro_rules! state {
                ($name:literal, $value:expr) => {
                    if let Some(value) = $value {
                        states.push(format!("{}={:?}", $name, value));
                    }
                };
            }
            state!("toggled", node.toggled());
            state!("selected", node.is_selected());
            state!("expanded", node.is_expanded());
            state!("current", node.aria_current());
            state!("live", node.live());
            state!("popup", node.has_popup());
            state!("orientation", node.orientation());
            state!("invalid", node.invalid());
            state!("value", node.value());
            state!("now", node.numeric_value());
            state!("min", node.min_numeric_value());
            state!("max", node.max_numeric_value());
            state!("placeholder", node.placeholder());
            state!("pos", node.position_in_set());
            state!("of", node.size_of_set());
            for (flag, on) in [
                ("modal", node.is_modal()),
                ("busy", node.is_busy()),
                ("atomic", node.is_live_atomic()),
                ("required", node.is_required()),
                ("disabled", node.is_disabled()),
            ] {
                if on {
                    states.push(flag.to_owned());
                }
            }
            if !node.controls().is_empty() {
                states.push("controls".to_owned());
            }
            if node.active_descendant().is_some() {
                states.push("activedescendant".to_owned());
            }
            if node.bounds().is_none() && role != "Window" {
                states.push("NO BOUNDS".to_owned());
            }
            let actions: Vec<_> = [
                ("focus", Action::Focus),
                ("click", Action::Click),
                ("expand", Action::Expand),
                ("collapse", Action::Collapse),
                ("step", Action::Increment),
            ]
            .into_iter()
            .filter(|(_, action)| node.supports_action(*action))
            .map(|(name, _)| name)
            .collect();
            if !actions.is_empty() {
                states.push(format!("[{}]", actions.join(" ")));
            }
            if !states.is_empty() {
                let _ = write!(line, " {}", states.join(" "));
            }
            out.push_str(&line);
            out.push('\n');
        }
        let depth = if quiet { depth } else { depth + 1 };
        for child in node.children().iter().rev() {
            stack.push((*child, depth));
        }
    }
    out
}
