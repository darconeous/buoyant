use crate::{
    environment::DefaultEnvironment,
    primitives::{Dimensions, Interpolate, Point, geometry::Rectangle},
    render_target::RenderTarget,
    view::ViewLayout,
};

use super::{AnimatedJoin, AnimationDomain, ContentShape, IntrinsicShape, Render};

/// A render tree item that holds what a view is built from, and builds that
/// view's own render tree only while drawing it.
#[non_exhaustive]
pub struct Lazy<T, V> {
    pub origin: Point,
    pub size: Dimensions,
    pub data: T,
    pub build: fn(&T) -> V,
}

impl<T: Clone, V> Clone for Lazy<T, V> {
    fn clone(&self) -> Self {
        Self {
            origin: self.origin,
            size: self.size,
            data: self.data.clone(),
            build: self.build,
        }
    }
}

impl<T, V> Lazy<T, V> {
    pub const fn new(origin: Point, size: Dimensions, data: T, build: fn(&T) -> V) -> Self {
        Self {
            origin,
            size,
            data,
            build,
        }
    }
}

impl<T, V: ViewLayout<()>> Lazy<T, V> {
    /// Lays the view out again in the size it resolved to, and builds its
    /// render tree at `origin`.
    fn tree(&self, origin: Point) -> V::Renderables {
        let view = (self.build)(&self.data);
        let env = DefaultEnvironment::non_animated();
        let mut state = view.build_state(&mut ());
        let layout = view.layout(&self.size.into(), &env, &mut (), &mut state);
        view.render_tree(&layout.sublayouts, origin, &env, &mut (), &mut state)
    }
}

impl<T, V> AnimatedJoin for Lazy<T, V> {
    fn join_from(&mut self, source: &Self, domain: &AnimationDomain) {
        // The content jumps; only its position moves.
        self.origin = Point::interpolate(source.origin, self.origin, domain.factor);
    }
}

impl<T, V> IntrinsicShape for Lazy<T, V> {
    fn content_shape(&self) -> ContentShape {
        Rectangle::new(self.origin, self.size.into()).into()
    }
}

impl<T, V, C> Render<C> for Lazy<T, V>
where
    V: ViewLayout<(), Renderables: Render<C>>,
{
    fn render(&self, render_target: &mut impl RenderTarget<ColorFormat = C>, style: &C) {
        self.tree(self.origin).render(render_target, style);
    }

    fn render_animated(
        render_target: &mut impl RenderTarget<ColorFormat = C>,
        source: &Self,
        target: &Self,
        style: &C,
        domain: &AnimationDomain,
    ) {
        let origin = Point::interpolate(source.origin, target.origin, domain.factor);
        target.tree(origin).render(render_target, style);
    }
}

impl<T: core::fmt::Debug, V> core::fmt::Debug for Lazy<T, V> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Lazy")
            .field("origin", &self.origin)
            .field("size", &self.size)
            .field("data", &self.data)
            .finish_non_exhaustive()
    }
}
