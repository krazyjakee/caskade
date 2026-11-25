//! Command bindings: who handles an action.
//!
//! A binding belongs to an element (a section, a view) or to nobody (the
//! route or the app). The innermost binding around the focused element wins;
//! among bindings at the same place, the newest wins.

use crate::Action;
use crate::tree::{FocusTree, contains, depth};
use std::collections::HashMap;
use std::rc::Rc;

/// What a binding does; it returns false to let the next binding (or the
/// default) handle the action.
pub type Run = Rc<dyn Fn() -> bool>;

#[derive(Clone)]
pub struct Binding<N> {
    pub action: Action,
    /// Shown in the button-hint bar; unlabelled bindings aren't hinted.
    pub label: Option<String>,
    /// The element the binding belongs to; `None` for the whole view.
    pub owner: Option<N>,
    pub run: Run,
    /// Also applies inside an open overlay (a menu, a dialog), after the
    /// overlay's own bindings: Select and Back, the guide button.
    pub global: bool,
}

impl<N> Binding<N> {
    /// A view-wide binding for `action`.
    pub fn new(action: Action, run: impl Fn() -> bool + 'static) -> Self {
        Self {
            action,
            label: None,
            owner: None,
            run: Rc::new(run),
            global: false,
        }
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn owner(mut self, owner: N) -> Self {
        self.owner = Some(owner);
        self
    }

    pub fn global(mut self) -> Self {
        self.global = true;
        self
    }
}

/// A binding's handle, to take it away again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BindingId(u64);

/// An action's label in the hint bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveBinding {
    pub action: Action,
    pub label: String,
}

struct Entry<N> {
    /// Monotonic registration sequence, so tie-breaking is deterministic
    /// regardless of map iteration order.
    order: u64,
    binding: Binding<N>,
}

/// The bindings in force, indexed by [`BindingId`] for O(1) updates and
/// removals while preserving registration order for tie-breaking.
pub struct Bindings<N> {
    entries: HashMap<BindingId, Entry<N>>,
    next: u64,
    /// Counts changes, so hints know when to look again.
    revision: u64,
}

impl<N> Default for Bindings<N> {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            next: 0,
            revision: 0,
        }
    }
}

impl<N: Copy + Eq> Bindings<N> {
    pub fn add(&mut self, binding: Binding<N>) -> BindingId {
        let order = self.next;
        let id = BindingId(order);
        self.next += 1;
        self.entries.insert(id, Entry { order, binding });
        self.revision += 1;
        id
    }

    pub fn remove(&mut self, id: BindingId) {
        if self.entries.remove(&id).is_some() {
            self.revision += 1;
        }
    }

    /// Updates a binding's owner, label and global flag together; returns
    /// whether any field changed.
    pub fn update(
        &mut self,
        id: BindingId,
        owner: Option<N>,
        label: Option<String>,
        global: bool,
    ) -> bool {
        let Some(entry) = self.entries.get_mut(&id) else {
            return false;
        };
        let binding = &mut entry.binding;
        let mut changed = false;
        if binding.owner != owner {
            binding.owner = owner;
            changed = true;
        }
        if binding.label != label {
            binding.label = label;
            changed = true;
        }
        if binding.global != global {
            binding.global = global;
            changed = true;
        }
        if changed {
            self.revision += 1;
        }
        changed
    }

    /// Moves a binding to another element, as when its owner renders again.
    pub fn set_owner(&mut self, id: BindingId, owner: Option<N>) {
        if let Some(entry) = self.entries.get_mut(&id)
            && entry.binding.owner != owner
        {
            entry.binding.owner = owner;
            self.revision += 1;
        }
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Bindings for `action` (all of them for `None`), the one to try first
    /// first. Inside an open overlay (`within`), only the overlay's own
    /// bindings and global ones count.
    fn ordered<T: FocusTree<Node = N> + ?Sized>(
        &self,
        tree: &T,
        focused: Option<N>,
        within: Option<N>,
        action: Option<Action>,
    ) -> Vec<&Binding<N>> {
        let level = |binding: &Binding<N>| -> Option<usize> {
            let owner = binding.owner;
            if let Some(within) = within
                && !binding.global
                && !owner.is_some_and(|owner| contains(tree, within, owner))
            {
                return None;
            }
            let Some(owner) = owner else {
                return Some(0);
            };
            if !focused.is_some_and(|focused| contains(tree, owner, focused)) {
                return None;
            }
            Some(depth(tree, owner) + 1)
        };
        let mut found: Vec<(usize, u64, &Binding<N>)> = self
            .entries
            .values()
            .filter(|entry| action.is_none_or(|action| entry.binding.action == action))
            .filter_map(|entry| Some((level(&entry.binding)?, entry.order, &entry.binding)))
            .collect();
        found.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        found.into_iter().map(|(_, _, binding)| binding).collect()
    }

    /// The handlers for `action`, in the order to try them. Run them
    /// outside any borrow of the registry, since a handler may change it.
    pub fn handlers<T: FocusTree<Node = N> + ?Sized>(
        &self,
        tree: &T,
        action: Action,
        focused: Option<N>,
        within: Option<N>,
    ) -> Vec<Run> {
        self.ordered(tree, focused, within, Some(action))
            .into_iter()
            .map(|binding| binding.run.clone())
            .collect()
    }

    /// What each action would do now, for hints: the first labelled binding
    /// per action.
    pub fn active<T: FocusTree<Node = N> + ?Sized>(
        &self,
        tree: &T,
        focused: Option<N>,
        within: Option<N>,
    ) -> Vec<ActiveBinding> {
        let mut seen: Vec<ActiveBinding> = Vec::new();
        for binding in self.ordered(tree, focused, within, None) {
            if let Some(label) = &binding.label
                && !seen.iter().any(|active| active.action == binding.action)
            {
                seen.push(ActiveBinding {
                    action: binding.action,
                    label: label.clone(),
                });
            }
        }
        seen
    }
}

/// Runs `handlers` until one handles the action.
pub fn run(handlers: &[Run]) -> bool {
    handlers.iter().any(|run| run())
}
