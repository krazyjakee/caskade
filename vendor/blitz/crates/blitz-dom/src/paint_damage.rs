//! Damage-limited repaint of animation frames.
//!
//! While a CSS animation or transition runs, a frame whose only cause is the
//! animation tick changes nothing but the animating elements. [`BaseDocument::resolve`]
//! works out what such a frame changed ([`PaintDamage`]): nothing (a `steps()`
//! animation between steps), a rectangle (the union of where the animating
//! elements painted last frame and where they paint now), or everything
//! (anything that changed layout, or that this module can't bound). An
//! embedder that only ticked animations since the last frame can then repaint
//! just that rectangle; any other cause of a frame still repaints it all.
//!
//! The painter records, for each animating element it visits, the transform
//! and clip its parent painted it with ([`PaintRecord`]). Nothing above an
//! animating element moves in an animation-only frame, so its new bounds
//! follow from that record and its new layout and transform.

use std::collections::{HashMap, HashSet};

use blitz_traits::node_id::NodeId;
use kurbo::{Affine, Rect, Vec2};
use style::selector_parser::RestyleDamage;

use crate::{BaseDocument, node::Node};

/// What changed on screen in the last [`resolve`](BaseDocument::resolve), for
/// a frame caused only by animations ticking.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PaintDamage {
    /// Nothing visible changed: the previous frame is still correct.
    None,
    /// Only this rectangle changed, in window pixels as the painter places
    /// them (before its x/y offset).
    Rect(Rect),
    /// Repaint the whole window.
    Full,
}

/// Where the painter drew an animating element in the last frame.
#[derive(Clone, Copy, Debug)]
pub struct PaintRecord {
    /// The parent's transform to window pixels, as the painter applied it.
    pub parent_transform: Affine,
    /// The clip rectangle the element was painted (or culled) within.
    pub clip_rect: Rect,
    /// The window-pixel rectangle the element's painting covered, clipped.
    pub bounds: Rect,
}

/// Damage no worse than this (transforms, text decorations, repaints) leaves
/// layout alone.
const PAINT_ONLY: RestyleDamage = RestyleDamage::RECALCULATE_OVERFLOW;

/// Anti-aliased edges reach a little past the geometry.
const AA_MARGIN: f64 = 2.0;

impl Node {
    /// How far this element's painting reaches beyond its border box, in
    /// window pixels: outset box shadows, outlines, text shadows and filters.
    pub fn ink_outset(&self, scale: f64) -> f64 {
        let Some(style) = self.primary_styles() else {
            return 0.0;
        };
        let mut outset: f64 = 0.0;
        for shadow in style.get_effects().box_shadow.0.iter().filter(|s| !s.inset) {
            let base = &shadow.base;
            let offset = base.horizontal.px().abs().max(base.vertical.px().abs()) as f64;
            outset = outset.max(offset + shadow.spread.px() as f64 + base.blur.px() as f64 * 2.5);
        }
        let outline = style.get_outline();
        if !outline.outline_style.none_or_hidden() {
            outset = outset.max(
                outline.outline_width.0.to_f64_px() + outline.outline_offset.to_f64_px().max(0.0),
            );
        }
        for shadow in style.get_inherited_text().text_shadow.0.iter() {
            let offset = shadow.horizontal.px().abs().max(shadow.vertical.px().abs()) as f64;
            outset = outset.max(offset + shadow.blur.px() as f64 * 2.5);
        }
        if !style.get_effects().filter.0.is_empty() {
            // Blurs and drop shadows: bound them generously rather than exactly.
            outset = outset.max(64.0);
        }
        outset * scale
    }

    /// The window-pixel bounds of this element's painting, drawn under
    /// `parent_transform` (the painter's `screen_bbox`, plus ink overflow).
    pub fn paint_bounds(&self, parent_transform: Affine, scale: f64) -> Rect {
        let layout = self.final_layout();
        let transform = parent_transform
            * Affine::translate(
                Vec2::new(layout.location.x as f64, layout.location.y as f64) * scale,
            )
            * self.transform().as_deref().copied().unwrap_or_default();
        let border_box = Rect::new(
            0.0,
            0.0,
            layout.size.width as f64 * scale,
            layout.size.height as f64 * scale,
        );
        let ink = self.ink_outset(scale) + AA_MARGIN;
        transform
            .transform_rect_bbox(self.scrollable_overflow().union(border_box))
            .inflate(ink, ink)
    }
}

/// Damage bookkeeping carried by a [`BaseDocument`].
#[derive(Default)]
pub(crate) struct DamageState {
    /// Elements (and pseudo-elements) with running animations or transitions.
    pub(crate) animating: HashSet<NodeId>,
    /// Where the painter drew each animating element last.
    pub(crate) records: std::sync::Mutex<HashMap<NodeId, PaintRecord>>,
    /// Elements with damage of their own in this resolve, before propagation.
    pub(crate) damaged: Vec<(NodeId, RestyleDamage)>,
    /// Something this module can't bound changed in this resolve.
    pub(crate) force_full: bool,
    /// The result of the last resolve.
    pub(crate) last: Option<PaintDamage>,
    /// When the running animations next change, if they only change in steps.
    pub(crate) next_change: Option<f64>,
}

impl BaseDocument {
    /// What the last [`resolve`](Self::resolve) changed on screen, if the frame
    /// was caused only by animations ticking. Any other cause of a frame
    /// (input, DOM changes, scrolling, resizing, loaded resources) needs a
    /// full repaint whatever this says.
    pub fn paint_damage(&self) -> PaintDamage {
        self.damage_state.last.unwrap_or(PaintDamage::Full)
    }

    /// When (on the animation timeline passed to [`resolve`](Self::resolve))
    /// the document next needs a frame, if animations are all that is
    /// running and each changes only in steps (`steps()` timing): an embedder
    /// can wait until then instead of ticking every frame. `None` means tick
    /// every frame while [`is_animating`](Self::is_animating).
    pub fn next_animation_change(&self) -> Option<f64> {
        if self.has_canvas
            || self.subdoc_is_animating
            || self.scroll_animation != crate::scrolling::ScrollAnimationState::None
            || self.scrollbars_animating()
        {
            return None;
        }
        self.damage_state.next_change
    }

    /// Whether `node_id` has a running animation or transition (the painter
    /// records where it draws these).
    pub fn is_animating_node(&self, node_id: NodeId) -> bool {
        self.damage_state.animating.contains(&node_id)
    }

    /// Record where the painter drew an animating element.
    pub fn record_paint(&self, node_id: NodeId, record: PaintRecord) {
        if let Ok(mut records) = self.damage_state.records.lock() {
            records.insert(node_id, record);
        }
    }

    /// Where the painter last drew an animating element.
    pub fn paint_record(&self, node_id: NodeId) -> Option<PaintRecord> {
        self.damage_state
            .records
            .lock()
            .ok()?
            .get(&node_id)
            .copied()
    }

    /// Forget every paint record: a full repaint records them afresh.
    pub fn clear_paint_records(&self) {
        if let Ok(mut records) = self.damage_state.records.lock() {
            records.clear();
        }
    }

    /// Collect the nodes carrying damage of their own, before it is
    /// propagated to their ancestors.
    pub(crate) fn collect_own_damage(&mut self) {
        let mut damaged = std::mem::take(&mut self.damage_state.damaged);
        damaged.clear();
        let mut stack = vec![self.root_node().id];
        while let Some(id) = stack.pop() {
            let Some(node) = self.nodes.get(id) else {
                continue;
            };
            let damage = node.damage().unwrap_or_else(RestyleDamage::empty);
            if !damage.is_empty() {
                damaged.push((id, damage));
            }
            if damage.is_empty() && !node.has_damaged_descendants() && id != self.root_node().id {
                continue;
            }
            stack.extend(node.children.iter().copied());
            stack.extend(node.before());
            stack.extend(node.after());
        }
        self.damage_state.damaged = damaged;
    }

    /// The nearest element at or above `node_id` with a running animation.
    fn animating_ancestor(&self, mut node_id: NodeId) -> Option<NodeId> {
        loop {
            if self.damage_state.animating.contains(&node_id) {
                return Some(node_id);
            }
            node_id = self.nodes.get(node_id)?.parent?;
        }
    }

    /// Work out [`PaintDamage`] for this resolve (after layout and
    /// transforms).
    pub(crate) fn compute_paint_damage(&mut self) {
        let damage = self.paint_damage_inner();
        self.damage_state.last = Some(damage);
        self.damage_state.force_full = false;
    }

    fn paint_damage_inner(&self) -> PaintDamage {
        let state = &self.damage_state;
        if state.force_full
            || self.has_canvas
            || !self.sub_document_nodes.is_empty()
            || self.scroll_animation != crate::scrolling::ScrollAnimationState::None
            || self.scrollbars_animating()
        {
            return PaintDamage::Full;
        }
        if state.damaged.is_empty() {
            return PaintDamage::None;
        }
        let mut changed = HashSet::new();
        for &(node_id, damage) in &state.damaged {
            if !PAINT_ONLY.contains(damage) {
                return PaintDamage::Full;
            }
            match self.animating_ancestor(node_id) {
                Some(animating) => {
                    changed.insert(animating);
                }
                None => return PaintDamage::Full,
            }
        }
        let Ok(records) = state.records.lock() else {
            return PaintDamage::Full;
        };
        let scale = self.viewport.scale_f64();
        let mut rect: Option<Rect> = None;
        for node_id in changed {
            // Never painted (new, or inside a culled subtree): no bounds to trust.
            let (Some(record), Some(node)) = (records.get(&node_id), self.nodes.get(node_id))
            else {
                return PaintDamage::Full;
            };
            let now = node
                .paint_bounds(record.parent_transform, scale)
                .intersect(record.clip_rect);
            for part in [record.bounds, now] {
                if part.width() > 0.0 && part.height() > 0.0 {
                    rect = Some(rect.map_or(part, |r| r.union(part)));
                }
            }
        }
        rect.map_or(PaintDamage::None, PaintDamage::Rect)
    }
}
