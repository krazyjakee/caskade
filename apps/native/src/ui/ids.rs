//! Ids for `aria-labelledby`, `aria-controls` and the like: unique in the
//! window, stable for a component's life.

use std::sync::atomic::{AtomicU64, Ordering};

use dioxus_native::prelude::*;

static NEXT: AtomicU64 = AtomicU64::new(1);

/// A new id starting with `prefix`, e.g. `cs-switch-7`.
pub fn new_id(prefix: &str) -> String {
    format!("cs-{prefix}-{}", NEXT.fetch_add(1, Ordering::Relaxed))
}

/// An id for this component instance, made on its first render.
pub fn use_id(prefix: &'static str) -> String {
    use_hook(|| new_id(prefix))
}
