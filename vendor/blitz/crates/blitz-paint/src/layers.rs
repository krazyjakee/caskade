use anyrender::{Filter, PaintScene};
use kurbo::{Affine, Point, Rect, Shape};
use peniko::{BlendMode, Fill};
use std::{cell::Cell, sync::Arc};

const LAYER_LIMIT: u32 = 1024;

#[derive(Default)]
pub(crate) struct LayerManager {
    layers_used: Cell<u32>,
    layer_depth: Cell<u32>,
    layers_wanted: Cell<u32>,

    #[allow(unused)] // Only used for debugging. Enabled as required.
    layer_depth_used: Cell<u32>,

    /// When only this rectangle (scene coordinates) is repainted, a layer whose
    /// clip covers it can clip to it instead: the same pixels, and renderers
    /// whose cost grows with a layer's area do far less work.
    pub(crate) damage: Cell<Option<Rect>>,
}

/// Whether `shape` (under `transform`) covers all of `rect`. Exact for convex
/// shapes (rectangles, rounded rectangles, ellipses), which clips are.
pub(crate) fn covers(shape: &impl Shape, transform: Affine, rect: Rect) -> bool {
    if transform.determinant().abs() < 1e-12 {
        return false;
    }
    let inverse = transform.inverse();
    if let Some(shape_rect) = shape.as_rect() {
        return shape_rect.contains_rect(inverse.transform_rect_bbox(rect))
            && transform.as_coeffs()[1] == 0.0
            && transform.as_coeffs()[2] == 0.0;
    }
    // Box paths are left open (renderers fill them closed): close every
    // subpath, or their winding counts as zero everywhere.
    let mut closed = kurbo::BezPath::new();
    let mut open = false;
    for el in shape.path_elements(0.1) {
        match el {
            kurbo::PathEl::MoveTo(_) if open => {
                closed.close_path();
                closed.push(el);
            }
            kurbo::PathEl::ClosePath => {
                open = false;
                closed.push(el);
            }
            _ => {
                open = true;
                closed.push(el);
            }
        }
    }
    if open {
        closed.close_path();
    }
    [
        Point::new(rect.x0, rect.y0),
        Point::new(rect.x1, rect.y0),
        Point::new(rect.x0, rect.y1),
        Point::new(rect.x1, rect.y1),
    ]
    .into_iter()
    .all(|corner| closed.contains(inverse * corner))
}

impl LayerManager {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn maybe_with_layer<S: PaintScene, F: FnOnce(&mut S)>(
        &self,
        scene: &mut S,
        condition: bool,
        blend_mode: BlendMode,
        opacity: f32,
        transform: Affine,
        shape: &impl Shape,
        filter: Option<Arc<Filter>>,
        backdrop_filter: Option<Arc<Filter>>,
        paint_layer: F,
    ) {
        let layer_used = self.maybe_push_layer(
            scene,
            condition,
            blend_mode,
            opacity,
            transform,
            shape,
            filter,
            backdrop_filter,
        );
        paint_layer(scene);
        self.maybe_pop_layer(scene, layer_used);
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn maybe_push_layer(
        &self,
        scene: &mut impl PaintScene,
        condition: bool,
        blend_mode: BlendMode,
        opacity: f32,
        transform: Affine,
        shape: &impl Shape,
        filter: Option<Arc<Filter>>,
        backdrop_filter: Option<Arc<Filter>>,
    ) -> bool {
        if !condition {
            return false;
        }
        self.layers_wanted.update(|x| x + 1);

        // Check if clips are above limit
        let layers_available = self.layers_used.get() <= LAYER_LIMIT;
        if !layers_available {
            return false;
        }

        // A layer clipped to more than the damage only draws the damage: clip it
        // there. Filters read beyond the clip, so they keep their own.
        let damage = self.damage.get().filter(|damage| {
            filter.is_none() && backdrop_filter.is_none() && covers(shape, transform, *damage)
        });
        // Actually push the layer
        if blend_mode == BlendMode::default()
            && opacity == 1.0
            && filter.is_none()
            && backdrop_filter.is_none()
        {
            match damage {
                Some(damage) => scene.push_clip_layer(Fill::NonZero, Affine::IDENTITY, &damage),
                None => scene.push_clip_layer(Fill::NonZero, transform, shape),
            }
        } else {
            match damage {
                Some(damage) => scene.push_layer(
                    Fill::NonZero,
                    blend_mode,
                    opacity,
                    Affine::IDENTITY,
                    &damage,
                    filter,
                    backdrop_filter,
                ),
                None => scene.push_layer(
                    Fill::NonZero,
                    blend_mode,
                    opacity,
                    transform,
                    shape,
                    filter,
                    backdrop_filter,
                ),
            }
        };

        // Update accounting
        self.layers_used.update(|x| x + 1);
        self.layer_depth.update(|x| x + 1);
        self.layer_depth.update(|x| x.max(self.layer_depth.get()));

        true
    }

    /// The damage rectangle when a filterless layer with this clip should
    /// clip to it instead (see [`LayerManager::damage`]).
    pub(crate) fn damage_clip(&self, transform: Affine, shape: &impl Shape) -> Option<Rect> {
        self.damage
            .get()
            .filter(|damage| covers(shape, transform, *damage))
    }

    pub(crate) fn maybe_pop_layer(&self, scene: &mut impl PaintScene, condition: bool) {
        if condition {
            scene.pop_layer();
            self.layer_depth.update(|x| x - 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyrender::{Scene, recording::RenderCommand};
    use kurbo::Rect;
    use peniko::{Compose, Mix};

    #[test]
    fn blend_layers_are_not_replaced_by_clips() {
        for blend in [
            Mix::Multiply.into(),
            BlendMode::new(Mix::Normal, Compose::PlusLighter),
        ] {
            let manager = LayerManager::default();
            let mut scene = Scene::new();
            manager.maybe_with_layer(
                &mut scene,
                true,
                blend,
                1.0,
                Affine::IDENTITY,
                &Rect::ZERO,
                None,
                None,
                |_| {},
            );
            assert!(
                matches!(&scene.commands[0], RenderCommand::PushLayer(layer) if layer.blend == blend)
            );
            assert!(matches!(scene.commands[1], RenderCommand::PopLayer));
        }
    }

    #[test]
    fn normal_layers_clip_unless_isolation_is_forced() {
        for (opacity, isolated) in [(1.0, false), (1.0 - f32::EPSILON, true)] {
            let manager = LayerManager::default();
            let mut scene = Scene::new();
            manager.maybe_with_layer(
                &mut scene,
                true,
                BlendMode::default(),
                opacity,
                Affine::IDENTITY,
                &Rect::ZERO,
                None,
                None,
                |_| {},
            );
            assert_eq!(
                matches!(scene.commands[0], RenderCommand::PushLayer(_)),
                isolated
            );
            assert!(matches!(scene.commands[1], RenderCommand::PopLayer));
        }
    }

    #[test]
    fn disabled_or_exhausted_layers_do_not_pop() {
        let manager = LayerManager::default();
        let mut scene = Scene::new();
        for condition in [false, true] {
            manager.layers_used.set(LAYER_LIMIT + 1);
            manager.maybe_with_layer(
                &mut scene,
                condition,
                Mix::Multiply.into(),
                1.0,
                Affine::IDENTITY,
                &Rect::ZERO,
                None,
                None,
                |_| {},
            );
        }
        assert!(scene.commands.is_empty());
    }
}
