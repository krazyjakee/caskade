//! What the input rules read of the UI: a tree of elements with tags,
//! attributes and layout boxes, in document order. The native app reads
//! Blitz's document through [`FocusTree`]; tests build trees by hand
//! (`test_tree`).

use std::fmt::Debug;
use std::hash::Hash;

/// An element's box on screen, in CSS pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            left: x,
            top: y,
            right: x + width,
            bottom: y + height,
        }
    }

    pub fn width(&self) -> f32 {
        self.right - self.left
    }

    pub fn height(&self) -> f32 {
        self.bottom - self.top
    }

    /// Whether it takes up any room: a hidden element's box doesn't.
    pub fn has_area(&self) -> bool {
        self.width() > 0.0 || self.height() > 0.0
    }
}

/// The UI's elements, as the input rules see them.
pub trait FocusTree {
    type Node: Copy + Eq + Hash + Debug;

    /// The document's body: the scope when no overlay is open.
    fn root(&self) -> Self::Node;
    fn parent(&self, node: Self::Node) -> Option<Self::Node>;
    /// Element children, in document order (text is left out).
    fn children(&self, node: Self::Node) -> Vec<Self::Node>;
    /// Calls `visit` on each element child of `node` in document order,
    /// stopping early if `visit` returns `false`. Returns `false` if stopped
    /// early, `true` otherwise.
    fn visit_children(&self, node: Self::Node, visit: &mut dyn FnMut(Self::Node) -> bool) -> bool {
        for child in self.children(node) {
            if !visit(child) {
                return false;
            }
        }
        true
    }
    /// The element's tag, lower case.
    fn tag(&self, node: Self::Node) -> &str;
    fn attr(&self, node: Self::Node, name: &str) -> Option<&str>;
    /// Its box on screen; an empty one when it has none.
    fn rect(&self, node: Self::Node) -> Rect;
    /// The text it shows, for the e2e harness and tests.
    fn text(&self, node: Self::Node) -> String;
}

/// Whether `inner` is `outer` or inside it.
pub fn contains<T: FocusTree + ?Sized>(tree: &T, outer: T::Node, inner: T::Node) -> bool {
    let mut at = Some(inner);
    while let Some(node) = at {
        if node == outer {
            return true;
        }
        at = tree.parent(node);
    }
    false
}

/// The nearest of `node` and its ancestors that `test` accepts.
pub fn closest<T: FocusTree + ?Sized>(
    tree: &T,
    node: T::Node,
    test: impl Fn(T::Node) -> bool,
) -> Option<T::Node> {
    let mut at = Some(node);
    while let Some(here) = at {
        if test(here) {
            return Some(here);
        }
        at = tree.parent(here);
    }
    None
}

/// The first descendant of `node` in document order that `test` accepts,
/// `node` not included. Stops as soon as a match is found.
pub fn find_descendant<T: FocusTree + ?Sized>(
    tree: &T,
    node: T::Node,
    mut test: impl FnMut(T::Node) -> bool,
) -> Option<T::Node> {
    fn walk<T: FocusTree + ?Sized, F: FnMut(T::Node) -> bool>(
        tree: &T,
        node: T::Node,
        test: &mut F,
        found: &mut Option<T::Node>,
    ) {
        tree.visit_children(node, &mut |child| {
            if test(child) {
                *found = Some(child);
                return false;
            }
            walk(tree, child, test, found);
            found.is_none()
        });
    }
    let mut found = None;
    walk(tree, node, &mut test, &mut found);
    found
}

/// How deep `node` is: the root is 0.
pub fn depth<T: FocusTree + ?Sized>(tree: &T, node: T::Node) -> usize {
    let mut level = 0;
    let mut at = tree.parent(node);
    while let Some(parent) = at {
        level += 1;
        at = tree.parent(parent);
    }
    level
}

/// A tree built by hand, for tests: elements with attributes and a box from
/// `data-rect="x y w h"`.
#[cfg(test)]
pub mod test_tree {
    use super::*;

    #[derive(Debug, Clone)]
    pub struct Element {
        pub tag: String,
        pub attrs: Vec<(String, String)>,
        pub text: String,
        pub parent: Option<usize>,
        pub children: Vec<usize>,
    }

    /// Elements by index; 0 is the body.
    #[derive(Debug, Clone)]
    pub struct TestTree {
        pub elements: Vec<Element>,
    }

    impl Default for TestTree {
        fn default() -> Self {
            Self {
                elements: vec![Element {
                    tag: "body".into(),
                    attrs: Vec::new(),
                    text: String::new(),
                    parent: None,
                    children: Vec::new(),
                }],
            }
        }
    }

    impl TestTree {
        /// Adds a `tag` element under `parent` with `attrs` and `text`.
        pub fn add(
            &mut self,
            parent: usize,
            tag: &str,
            attrs: &[(&str, &str)],
            text: &str,
        ) -> usize {
            let id = self.elements.len();
            self.elements.push(Element {
                tag: tag.into(),
                attrs: attrs
                    .iter()
                    .map(|(name, value)| ((*name).into(), (*value).into()))
                    .collect(),
                text: text.into(),
                parent: Some(parent),
                children: Vec::new(),
            });
            self.elements[parent].children.push(id);
            id
        }

        /// A button at `x y w h` reading `text`.
        pub fn button(&mut self, parent: usize, rect: &str, text: &str) -> usize {
            self.add(parent, "button", &[("data-rect", rect)], text)
        }

        /// A `data-focus-section` named `id`, with no box of its own.
        pub fn section(&mut self, parent: usize, id: &str) -> usize {
            self.add(parent, "div", &[("data-focus-section", id)], "")
        }

        pub fn set(&mut self, node: usize, name: &str, value: &str) {
            let attrs = &mut self.elements[node].attrs;
            attrs.retain(|(existing, _)| existing != name);
            attrs.push((name.into(), value.into()));
        }

        /// Takes `node` out of the tree.
        pub fn remove(&mut self, node: usize) {
            if let Some(parent) = self.elements[node].parent.take() {
                self.elements[parent]
                    .children
                    .retain(|child| *child != node);
            }
        }

        /// The element whose text is `text`.
        pub fn by_text(&self, text: &str) -> usize {
            self.elements
                .iter()
                .position(|element| element.text == text)
                .unwrap_or_else(|| panic!("no element reads {text:?}"))
        }
    }

    impl FocusTree for TestTree {
        type Node = usize;

        fn root(&self) -> usize {
            0
        }

        fn parent(&self, node: usize) -> Option<usize> {
            self.elements[node].parent
        }

        fn children(&self, node: usize) -> Vec<usize> {
            self.elements[node].children.clone()
        }

        fn visit_children(&self, node: usize, visit: &mut dyn FnMut(usize) -> bool) -> bool {
            for &child in &self.elements[node].children {
                if !visit(child) {
                    return false;
                }
            }
            true
        }

        fn tag(&self, node: usize) -> &str {
            &self.elements[node].tag
        }

        fn attr(&self, node: usize, name: &str) -> Option<&str> {
            self.elements[node]
                .attrs
                .iter()
                .find(|(existing, _)| existing == name)
                .map(|(_, value)| value.as_str())
        }

        fn rect(&self, node: usize) -> Rect {
            let Some(spec) = self.attr(node, "data-rect") else {
                // Overlays fill the screen by default when no explicit `data-rect` is given.
                let overlay = matches!(self.attr(node, "role"), Some("dialog" | "alertdialog"))
                    || self.attr(node, "data-placement").is_some();
                return if overlay {
                    Rect::new(0.0, 0.0, 1600.0, 900.0)
                } else {
                    Rect::default()
                };
            };
            let numbers: Vec<f32> = spec
                .split_whitespace()
                .map(|n| n.parse().expect("data-rect numbers"))
                .collect();
            Rect::new(numbers[0], numbers[1], numbers[2], numbers[3])
        }

        fn text(&self, node: usize) -> String {
            let mut text = self.elements[node].text.clone();
            for child in &self.elements[node].children {
                text.push_str(&self.text(*child));
            }
            text
        }
    }
}
