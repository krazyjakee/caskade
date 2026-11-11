//! Choosing several games in the library (#99), as the webview's
//! `components/gameSelection/selection.tsx`: games are chosen by id, so
//! one scrolled out of the grid stays chosen; Shift extends from the last
//! game clicked over the games loaded between; the bar under the grid acts
//! on them.

use dioxus_native::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

/// The games the grid has loaded, by index: `None` where a page isn't.
type GameAt = Rc<RefCell<Box<dyn Fn(usize) -> Option<String>>>>;

/// One view's selection: chosen games, whether it's on, and where a Shift
/// range starts.
#[derive(Clone, Copy, PartialEq)]
pub struct GameSelection {
    chosen: Signal<Vec<String>>,
    on: Signal<bool>,
    anchor: Signal<Option<usize>>,
    /// The grid's lookup, refreshed as it renders.
    game_at: Signal<GameAt>,
    /// Bumped by a pad's Start: the bar opens the bulk editor.
    edits: Signal<u64>,
}

impl GameSelection {
    pub fn active(&self) -> bool {
        (self.on)()
    }

    /// The chosen games, in the order they were chosen.
    pub fn ids(&self) -> Vec<String> {
        self.chosen.read().clone()
    }

    pub fn count(&self) -> usize {
        self.chosen.read().len()
    }

    /// Whether `id` is chosen; `None` when choosing is off.
    pub fn state(&self, id: &str) -> Option<bool> {
        self.active()
            .then(|| self.chosen.read().iter().any(|chosen| chosen == id))
    }

    pub fn start(&mut self) {
        self.on.set(true);
    }

    /// Off, with nothing chosen.
    pub fn stop(&mut self) {
        self.on.set(false);
        self.chosen.write().clear();
        self.anchor.set(None);
    }

    /// Nothing chosen; still on.
    pub fn clear(&mut self) {
        self.chosen.write().clear();
        self.anchor.set(None);
    }

    /// Chooses `id`, or unchooses it; a Shift range starts at `index`.
    pub fn toggle(&mut self, id: &str, index: Option<usize>) {
        {
            let mut chosen = self.chosen.write();
            match chosen.iter().position(|chosen| chosen == id) {
                Some(at) => {
                    chosen.remove(at);
                }
                None => chosen.push(id.to_owned()),
            }
        }
        if index.is_some() {
            self.anchor.set(index);
        }
        self.on.set(true);
    }

    /// Shift: adds the loaded games from the last one clicked to `index`
    /// (never takes any away); just that one with nothing clicked yet.
    pub fn extend(&mut self, index: usize) {
        let from = self.anchor.peek().unwrap_or(index);
        let ids: Vec<String> = {
            let game_at = self.game_at.peek();
            let game_at = game_at.borrow();
            (from.min(index)..=from.max(index))
                .filter_map(&**game_at)
                .collect()
        };
        self.add(ids);
        self.anchor.set(Some(index));
    }

    /// Chooses `ids` too.
    pub fn add(&mut self, ids: impl IntoIterator<Item = String>) {
        {
            let mut chosen = self.chosen.write();
            for id in ids {
                if !chosen.contains(&id) {
                    chosen.push(id);
                }
            }
        }
        self.on.set(true);
    }

    /// The games loaded so far among the view's `total`.
    pub fn loaded(&self, total: usize) -> Vec<String> {
        let game_at = self.game_at.peek();
        let game_at = game_at.borrow();
        (0..total).filter_map(&**game_at).collect()
    }

    /// The grid's lookup of the games it has loaded.
    pub fn set_game_at(&self, game_at: impl Fn(usize) -> Option<String> + 'static) {
        *self.game_at.peek().borrow_mut() = Box::new(game_at);
    }

    /// A pad's Start: the bulk editor, when something is chosen.
    pub fn edit(&mut self) {
        if self.count() > 0 {
            self.edits += 1;
        }
    }

    /// Bumped by [`edit`](Self::edit).
    pub fn edits(&self) -> u64 {
        (self.edits)()
    }
}

/// A new selection for a view, offered to its grid and bar.
pub fn use_selection_provider() -> GameSelection {
    use_context_provider(|| GameSelection {
        chosen: Signal::new(Vec::new()),
        on: Signal::new(false),
        anchor: Signal::new(None),
        game_at: Signal::new(Rc::new(RefCell::new(Box::new(|_| None)))),
        edits: Signal::new(0),
    })
}

/// The view's selection, where it has one.
pub fn use_selection() -> Option<GameSelection> {
    try_use_context::<GameSelection>()
}
