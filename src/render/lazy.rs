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

impl<T, V> Lazy<T, V> {
    /// Whether the view, drawn at `origin`, reaches into what
    /// `render_target` draws. One that doesn't is neither built nor drawn:
    /// building it lays it out again, which for a long text is most of the
    /// cost of a frame it never appears in. A view drawing beyond its own
    /// bounds loses what it draws there while its bounds are out of sight.
    fn shown<C>(
        render_target: &impl RenderTarget<ColorFormat = C>,
        origin: Point,
        size: Dimensions,
    ) -> bool {
        Rectangle::new(origin, size.into()).intersects(&render_target.clip_rect())
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
        if Self::shown(render_target, self.origin, self.size) {
            self.tree(self.origin).render(render_target, style);
        }
    }

    fn render_animated(
        render_target: &mut impl RenderTarget<ColorFormat = C>,
        source: &Self,
        target: &Self,
        style: &C,
        domain: &AnimationDomain,
    ) {
        let origin = Point::interpolate(source.origin, target.origin, domain.factor);
        if Self::shown(render_target, origin, target.size) {
            target.tree(origin).render(render_target, style);
        }
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

#[cfg(test)]
mod tests {
    use core::sync::atomic::{AtomicUsize, Ordering};

    use super::Lazy;
    use crate::{
        font::CharacterBufferFont,
        primitives::{Point, Size},
        render::Render,
        render_target::{FixedTextBuffer, RenderTarget as _},
        view::prelude::*,
    };

    static FONT: CharacterBufferFont = CharacterBufferFont;
    static BUILT: AtomicUsize = AtomicUsize::new(0);

    fn word(text: &&'static str) -> impl View<char, ()> + use<> {
        BUILT.fetch_add(1, Ordering::Relaxed);
        Text::new(*text, &FONT)
    }

    #[test]
    fn a_lazy_view_out_of_sight_is_never_built() {
        let mut buffer = FixedTextBuffer::<6, 2>::default();
        let size = Size::new(5, 1).into();
        let lazy = |y| Lazy::new(Point::new(0, y), size, "hello", word);

        for y in [-1, 2, 40] {
            lazy(y).render(&mut buffer, &' ');
        }
        assert_eq!(BUILT.load(Ordering::Relaxed), 0);
        assert!(buffer.text.iter().flatten().all(|&c| c == ' '));

        // Inside a clip that leaves the first line out, only the second is drawn.
        buffer.with_layer(
            |layer| {
                layer.clip(&crate::primitives::geometry::Rectangle::new(
                    Point::new(0, 1),
                    Size::new(6, 1),
                ))
            },
            |buffer| {
                lazy(0).render(buffer, &' ');
                lazy(1).render(buffer, &' ');
            },
        );
        assert_eq!(BUILT.load(Ordering::Relaxed), 1);
        assert_eq!(buffer.text[0], [' '; 6]);
        assert_eq!(buffer.text[1], ['h', 'e', 'l', 'l', 'o', ' ']);
    }
}
