//! What the input rules read of the UI: a tree of elements with tags,
//! attributes and layout boxes, in document order. The native app reads
//! Blitz's document through [`FocusTree`]; tests build trees by hand
//! ([`test_tree`]).

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

/// `node`'s descendants in document order, `node` not included.
pub fn descendants<T: FocusTree + ?Sized>(tree: &T, node: T::Node) -> Vec<T::Node> {
    let mut found = Vec::new();
    let mut stack: Vec<T::Node> = tree.children(node).into_iter().rev().collect();
    while let Some(next) = stack.pop() {
        found.push(next);
        stack.extend(tree.children(next).into_iter().rev());
    }
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
/// `data-rect="x y w h"`, as the webview's tests give jsdom boxes.
#[cfg(any(test, feature = "test-tree"))]
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

        pub fn unset(&mut self, node: usize, name: &str) {
            self.elements[node]
                .attrs
                .retain(|(existing, _)| existing != name);
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
                // Overlays fill the screen, as in the webview's tests.
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
