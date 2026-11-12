//! Spatial navigation over the UI's layout boxes: the next focusable in a
//! direction, by geometry. Ported from the webview's `input/spatial.ts`,
//! rule for rule, so a pad moves the same way in both.
//!
//! Every control is a focusable element, so nothing registers itself.
//! `data-focus-section` groups focusables; a section remembers where focus
//! last was and is entered there again. The topmost open dialog or popover is
//! the scope, so focus never leaves an overlay.

use crate::Action;
use crate::tree::{FocusTree, Rect, closest, contains, descendants};
use std::collections::HashMap;
use std::hash::Hash;

/// A way to move focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    /// The direction a nav action moves; `None` for any other action.
    pub fn of(action: Action) -> Option<Self> {
        match action {
            Action::NavUp => Some(Self::Up),
            Action::NavDown => Some(Self::Down),
            Action::NavLeft => Some(Self::Left),
            Action::NavRight => Some(Self::Right),
            _ => None,
        }
    }

    fn across(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }
}

/// Roles whose items rove their tab stop: they take focus like controls.
const ITEM_ROLES: [&str; 7] = [
    "option",
    "tab",
    "radio",
    "menuitem",
    "menuitemradio",
    "menuitemcheckbox",
    "row",
];

fn role<T: FocusTree + ?Sized>(tree: &T, node: T::Node) -> Option<&str> {
    tree.attr(node, "role")
}

/// Whether `node` is a kind of element that takes focus: natively
/// focusable ones, plus the items of collections.
fn takes_focus<T: FocusTree + ?Sized>(tree: &T, node: T::Node) -> bool {
    match tree.tag(node) {
        "a" if tree.attr(node, "href").is_some() => return true,
        "button" | "select" | "textarea" => return true,
        // A hidden input is a value, never a control.
        "input" => return tree.attr(node, "type") != Some("hidden"),
        _ => {}
    }
    tree.attr(node, "tabindex").is_some()
        || role(tree, node).is_some_and(|role| ITEM_ROLES.contains(&role))
        || tree.attr(node, "contenteditable") == Some("true")
}

/// Items and inputs: focus stops even with `tabindex="-1"`.
fn is_item<T: FocusTree + ?Sized>(tree: &T, node: T::Node) -> bool {
    tree.tag(node) == "input" || role(tree, node).is_some_and(|role| ITEM_ROLES.contains(&role))
}

fn is_overlay<T: FocusTree + ?Sized>(tree: &T, node: T::Node) -> bool {
    matches!(role(tree, node), Some("dialog" | "alertdialog"))
        || tree.attr(node, "data-placement").is_some()
}

/// A boolean attribute that is on: present and not `"false"`.
fn flag<T: FocusTree + ?Sized>(tree: &T, node: T::Node, name: &str) -> bool {
    tree.attr(node, name).is_some_and(|value| value != "false")
}

fn has_box<T: FocusTree + ?Sized>(tree: &T, node: T::Node) -> bool {
    tree.rect(node).has_area()
}

/// The section `node` is in, itself included.
pub fn section<T: FocusTree + ?Sized>(tree: &T, node: T::Node) -> Option<T::Node> {
    closest(tree, node, |at| {
        tree.attr(at, "data-focus-section").is_some()
    })
}

/// The section around `node`'s parent: the next one out.
fn outer_section<T: FocusTree + ?Sized>(tree: &T, node: T::Node) -> Option<T::Node> {
    tree.parent(node).and_then(|parent| section(tree, parent))
}

/// Where focus may go now: the topmost overlay, else the document.
pub fn scope<T: FocusTree + ?Sized>(tree: &T) -> T::Node {
    let root = tree.root();
    descendants(tree, root)
        .into_iter()
        .filter(|&node| {
            is_overlay(tree, node)
                && closest(tree, node, |at| {
                    flag(tree, at, "hidden") || flag(tree, at, "inert")
                })
                .is_none()
                && has_box(tree, node)
        })
        .last()
        .unwrap_or(root)
}

/// Whether a dialog or popover is open.
pub fn has_overlay<T: FocusTree + ?Sized>(tree: &T) -> bool {
    scope(tree) != tree.root()
}

/// The overlay `node` is in, if any.
pub fn overlay_of<T: FocusTree + ?Sized>(tree: &T, node: T::Node) -> Option<T::Node> {
    closest(tree, node, |at| is_overlay(tree, at))
}

/// Whether focus can stop on `node`.
pub fn is_focusable<T: FocusTree + ?Sized>(tree: &T, node: T::Node) -> bool {
    if !contains(tree, tree.root(), node) || !takes_focus(tree, node) {
        return false;
    }
    // A Tab stop that isn't a place to go by direction (a scroll box).
    if tree.attr(node, "data-spatial") == Some("skip") {
        return false;
    }
    let hidden = closest(tree, node, |at| {
        flag(tree, at, "hidden")
            || flag(tree, at, "inert")
            || tree.attr(at, "aria-hidden") == Some("true")
    });
    if hidden.is_some() {
        return false;
    }
    if flag(tree, node, "disabled") || tree.attr(node, "aria-disabled") == Some("true") {
        return false;
    }
    // Programmatic-only targets (dialogs, scroll boxes) aren't stops; a
    // visually hidden input is, when it is the control.
    if tree.attr(node, "tabindex") == Some("-1") && !is_item(tree, node) {
        return false;
    }
    // Hidden inputs count by the box that stands for them.
    has_box(tree, stand_in(tree, node))
}

/// The element whose box stands for `node` on screen: a switch's,
/// checkbox's or slider thumb's input may be hidden, so its nearest visible
/// ancestor (the label, the thumb) stands in.
pub fn stand_in<T: FocusTree + ?Sized>(tree: &T, node: T::Node) -> T::Node {
    if tree.tag(node) != "input" || has_box(tree, node) {
        return node;
    }
    let root = tree.root();
    let mut at = tree.parent(node);
    while let Some(here) = at {
        if here == root {
            break;
        }
        if has_box(tree, here) {
            return here;
        }
        at = tree.parent(here);
    }
    node
}

/// The focusables inside `within`, in document order.
pub fn focusables<T: FocusTree + ?Sized>(tree: &T, within: T::Node) -> Vec<T::Node> {
    descendants(tree, within)
        .into_iter()
        .filter(|&node| is_focusable(tree, node))
        .collect()
}

/// `node`'s box, by its stand-in.
pub fn rect<T: FocusTree + ?Sized>(tree: &T, node: T::Node) -> Rect {
    tree.rect(stand_in(tree, node))
}

/// A settings row: its label on the left, its control at the far end.
fn row<T: FocusTree + ?Sized>(tree: &T, node: T::Node) -> Option<T::Node> {
    closest(tree, node, |at| {
        tree.attr(at, "data-cs") == Some("setting-row")
    })
}

/// Where `node` is when moving from `from`. A control in a settings row
/// stands for its whole row, so a pad goes row by row; within one row, the
/// controls' own boxes tell them apart.
fn place<T: FocusTree + ?Sized>(tree: &T, node: T::Node, from: T::Node) -> Rect {
    match row(tree, node) {
        Some(own) if !contains(tree, own, from) => tree.rect(own),
        _ => rect(tree, node),
    }
}

/// Added for a target outside the row or column you're in, so one in it wins
/// however far along it is: across a list row to its button, not up to the
/// filter just above. Going up or down, only within the nearest line
/// ([`nearest_line`]).
const OFF_AXIS: f32 = 100_000.0;

/// How far `to` is from `from` in `direction`, or `None` if it isn't that
/// way. The gap along the direction counts once; a sideways offset counts
/// twice, and only targets in the row or column you're in come first.
fn distance(from: Rect, to: Rect, direction: Direction) -> Option<f32> {
    let centre = |low: f32, high: f32| (low + high) / 2.0;
    let (along, side) = match direction {
        Direction::Right => (
            to.left - from.right,
            gap(from.top, from.bottom, to.top, to.bottom),
        ),
        Direction::Left => (
            from.left - to.right,
            gap(from.top, from.bottom, to.top, to.bottom),
        ),
        Direction::Down => (
            to.top - from.bottom,
            gap(from.left, from.right, to.left, to.right),
        ),
        Direction::Up => (
            from.top - to.bottom,
            gap(from.left, from.right, to.left, to.right),
        ),
    };
    // Both edges must move that way, not just the centre: the next row's
    // title, a little wider than this one's, isn't to its right.
    let ahead = match direction {
        Direction::Right => {
            centre(to.left, to.right) > centre(from.left, from.right) && to.left > from.left
        }
        Direction::Left => {
            centre(to.left, to.right) < centre(from.left, from.right) && to.right < from.right
        }
        Direction::Down => {
            centre(to.top, to.bottom) > centre(from.top, from.bottom) && to.top > from.top
        }
        Direction::Up => {
            centre(to.top, to.bottom) < centre(from.top, from.bottom) && to.bottom < from.bottom
        }
    };
    if !ahead {
        return None;
    }
    let off = if side > 0.0 {
        OFF_AXIS + side * 2.0
    } else {
        0.0
    };
    Some(along.max(0.0) + off)
}

/// The space between two ranges; 0 when they overlap.
fn gap(a_low: f32, a_high: f32, b_low: f32, b_high: f32) -> f32 {
    0f32.max(b_low - a_high).max(a_low - b_high)
}

struct Target<N> {
    node: N,
    to: Rect,
    /// The section it's in beside focus, if any ([`reach`]).
    reach: Option<Rect>,
    d: f32,
    in_home: bool,
}

/// The outermost focus section around `target` that focus isn't in (a
/// shelf, the settings tabs), when it has a box.
fn reach<T: FocusTree + ?Sized>(tree: &T, target: T::Node, from: T::Node) -> Option<Rect> {
    let entered = entered(tree, target, from)?;
    has_box(tree, entered).then(|| tree.rect(entered))
}

/// The outermost section around `target` that doesn't hold `from`.
fn entered<T: FocusTree + ?Sized>(tree: &T, target: T::Node, from: T::Node) -> Option<T::Node> {
    let mut found = None;
    let mut at = section(tree, target);
    while let Some(here) = at {
        if contains(tree, here, from) {
            break;
        }
        found = Some(here);
        at = outer_section(tree, here);
    }
    found
}

/// Going up or down, the line of targets nearest that way: those level
/// with the nearest one. A target in the column focus is in wins within the
/// line, never over a line in between (from the hero, the shelf below it,
/// not a card two shelves down). A section beside the column focus is in
/// doesn't set the line (the settings tabs beside the panel); one across it
/// does (a shelf), and so does any target in no other section (a card in
/// focus's own shelf). Left and right keep every target: a list row goes
/// across to its button, however far.
fn nearest_line<N: Copy>(
    targets: Vec<Target<N>>,
    direction: Direction,
    from: Rect,
) -> Vec<Target<N>> {
    if direction.across() {
        return targets;
    }
    let down = direction == Direction::Down;
    let level: Vec<&Target<N>> = targets
        .iter()
        .filter(|target| {
            target
                .reach
                .is_none_or(|reach| gap(from.left, from.right, reach.left, reach.right) == 0.0)
        })
        .collect();
    let pool: Vec<&Target<N>> = if level.is_empty() {
        targets.iter().collect()
    } else {
        level
    };
    let mut first: Option<Rect> = None;
    for target in pool {
        let nearer = first.is_none_or(|first| {
            if down {
                target.to.top < first.top
            } else {
                target.to.bottom > first.bottom
            }
        });
        if nearer {
            first = Some(target.to);
        }
    }
    let Some(first) = first else {
        return Vec::new();
    };
    targets
        .into_iter()
        .filter(|target| {
            if down {
                target.to.top < first.bottom
            } else {
                target.to.bottom > first.top
            }
        })
        .collect()
}

/// The outermost focus section around `node` (the sidebar, the main
/// content), or `None` outside any.
fn outermost<T: FocusTree + ?Sized>(tree: &T, node: T::Node) -> Option<T::Node> {
    let mut found = None;
    let mut at = section(tree, node);
    while let Some(here) = at {
        found = Some(here);
        at = outer_section(tree, here);
    }
    found
}

/// The first of `within`'s descendants with `name`.
fn first_with<T: FocusTree + ?Sized>(tree: &T, within: T::Node, name: &str) -> Option<T::Node> {
    descendants(tree, within)
        .into_iter()
        .find(|&node| tree.attr(node, name).is_some())
}

/// The first of `within`'s descendants whose `name` is `value`.
fn first_where<T: FocusTree + ?Sized>(
    tree: &T,
    within: T::Node,
    name: &str,
    value: &str,
) -> Option<T::Node> {
    descendants(tree, within)
        .into_iter()
        .find(|&node| tree.attr(node, name) == Some(value))
}

/// Section memory: where focus last was in each section.
#[derive(Debug, Clone)]
pub struct Spatial<N> {
    memory: HashMap<N, N>,
}

impl<N> Default for Spatial<N> {
    fn default() -> Self {
        Self {
            memory: HashMap::new(),
        }
    }
}

impl<N: Copy + Eq + Hash> Spatial<N> {
    /// Remembers `node` as the last focus of every section around it.
    pub fn remember<T: FocusTree<Node = N> + ?Sized>(&mut self, tree: &T, node: N) {
        let mut at = section(tree, node);
        while let Some(here) = at {
            self.memory.insert(here, node);
            at = outer_section(tree, here);
        }
    }

    /// Where focus last was in section `of`, while it can still go there.
    pub fn remembered<T: FocusTree<Node = N> + ?Sized>(&self, tree: &T, of: N) -> Option<N> {
        let node = *self.memory.get(&of)?;
        (is_focusable(tree, node) && contains(tree, of, node)).then_some(node)
    }

    /// Forgets sections that have left the tree, so a reused node id isn't
    /// taken for one.
    pub fn prune<T: FocusTree<Node = N> + ?Sized>(&mut self, tree: &T) {
        self.memory.retain(|&of, _| {
            contains(tree, tree.root(), of) && tree.attr(of, "data-focus-section").is_some()
        });
    }

    /// The focusable `direction` of `from` in the current scope, entering
    /// another section where it last had focus.
    pub fn next<T: FocusTree<Node = N> + ?Sized>(
        &self,
        tree: &T,
        from: N,
        direction: Direction,
    ) -> Option<N> {
        // Stay in the area focus is in (the sidebar, the content) while it
        // has something that way; only then cross into another. A control at
        // the content's left edge isn't "above" the sidebar's last item.
        let home = outermost(tree, from);
        let mut ahead = Vec::new();
        for candidate in focusables(tree, scope(tree)) {
            if candidate == from {
                continue;
            }
            let to = place(tree, candidate, from);
            let Some(d) = distance(place(tree, from, candidate), to, direction) else {
                continue;
            };
            ahead.push(Target {
                node: candidate,
                to,
                reach: reach(tree, candidate, from),
                d,
                in_home: home.is_some_and(|home| contains(tree, home, candidate)),
            });
        }
        if ahead.iter().any(|target| target.in_home) {
            ahead.retain(|target| target.in_home);
        }
        let mut best = None;
        let mut score = f32::INFINITY;
        for target in nearest_line(ahead, direction, rect(tree, from)) {
            if target.d < score {
                best = Some(target.node);
                score = target.d;
            }
        }
        let best = best?;
        // Entering another section: the outermost one entered decides, so
        // going from the sidebar into the content returns to the content's
        // last focus.
        Some(
            entered(tree, best, from)
                .and_then(|section| self.remembered(tree, section))
                .unwrap_or(best),
        )
    }

    /// Where focus starts when nothing has it: the content's last focus, then
    /// the view's default (`data-focus-default`), then its first focusable,
    /// then anything in scope.
    pub fn entry<T: FocusTree<Node = N> + ?Sized>(&self, tree: &T) -> Option<N> {
        let within = scope(tree);
        if within == tree.root()
            && let Some(main) = first_where(tree, within, "data-focus-section", "main")
            && let Some(found) = self.start_in(tree, main)
        {
            return Some(found);
        }
        self.start_in(tree, within)
    }

    /// Where focus starts in the view (the `main` focus section) when the
    /// window has no overlay: `None` when there's no view to start in (an
    /// overlay, a screen without one), `Some(None)` while the view has
    /// nothing to focus yet.
    pub fn view_entry<T: FocusTree<Node = N> + ?Sized>(&self, tree: &T) -> Option<Option<N>> {
        let within = scope(tree);
        if within != tree.root() {
            return None;
        }
        let main = first_where(tree, within, "data-focus-section", "main")?;
        Some(self.start_in(tree, main))
    }

    /// The remembered focus in `within`, else its default, else its first
    /// focusable.
    pub fn start_in<T: FocusTree<Node = N> + ?Sized>(&self, tree: &T, within: N) -> Option<N> {
        self.remembered(tree, within)
            .or_else(|| {
                first_with(tree, within, "data-focus-default")
                    .filter(|&preferred| is_focusable(tree, preferred))
            })
            .or_else(|| focusables(tree, within).into_iter().next())
    }
}

/// A description of an element that survives the view being rendered
/// again: its `data-focus-key`, else its place among its section's
/// focusables.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FocusMark {
    pub key: Option<String>,
    pub section: Option<String>,
    pub index: Option<usize>,
}

pub fn mark<T: FocusTree + ?Sized>(tree: &T, node: T::Node) -> FocusMark {
    let key = closest(tree, node, |at| tree.attr(at, "data-focus-key").is_some())
        .and_then(|keyed| tree.attr(keyed, "data-focus-key"))
        .map(str::to_owned);
    let around = section(tree, node);
    let list = focusables(tree, around.unwrap_or(tree.root()));
    FocusMark {
        key,
        section: around
            .and_then(|around| tree.attr(around, "data-focus-section"))
            .map(str::to_owned),
        index: list.iter().position(|&at| at == node),
    }
}

pub fn find<T: FocusTree + ?Sized>(tree: &T, at: &FocusMark) -> Option<T::Node> {
    let root = tree.root();
    if let Some(key) = &at.key
        && let Some(keyed) = first_where(tree, root, "data-focus-key", key)
    {
        let found = if is_focusable(tree, keyed) {
            Some(keyed)
        } else {
            focusables(tree, keyed).into_iter().next()
        };
        if found.is_some() {
            return found;
        }
    }
    let around = match &at.section {
        Some(name) => first_where(tree, root, "data-focus-section", name)?,
        None => root,
    };
    focusables(tree, around).get(at.index?).copied()
}
