use embedded_touch::Touch;

use crate::{
    event::{EventContext, EventResult, TouchResult},
    focus::{Seek, SeekResult},
    layout::ResolvedLayout,
    primitives::{Dimensions, Point, ProposedDimensions},
    render,
    transition::Opacity,
    view::{Event, ViewLayout, ViewMarker},
};

/// A view whose render tree holds only the data it is built from.
///
/// The view is built from `data` and laid out as usual, but its render tree
/// is built each time it is drawn and dropped afterward. The tree the app
/// keeps holds `data`, its position, and its size instead, which suits a
/// detailed, non-interactive view repeated many times over, such as a badge
/// on every row of a long list, whose render trees would otherwise take more
/// memory than everything else on the screen.
///
/// The view takes no input, is drawn without animation of its own, and is
/// laid out again in the size it first resolved to, so its layout must
/// resolve the same way when offered that size. Its content changes without
/// a transition; only its position animates. It is built only where its
/// bounds reach into what is being drawn, so it must draw within them.
///
/// # Examples
///
/// ```
/// use buoyant::view::prelude::*;
/// use buoyant::view::Lazy;
/// use embedded_graphics::pixelcolor::Rgb565;
///
/// fn dots(count: &u8) -> impl View<Rgb565, ()> + use<> {
///     let dot = |shown: bool| shown.then(|| Circle.frame_sized(6, 6));
///     HStack::new((dot(*count > 0), dot(*count > 1), dot(*count > 2))).with_spacing(2)
/// }
///
/// let view = Lazy::new(2u8, dots);
/// ```
pub struct Lazy<T, V> {
    data: T,
    build: fn(&T) -> V,
}

// By hand, so that neither needs the view to implement it.
impl<T: Clone, V> Clone for Lazy<T, V> {
    fn clone(&self) -> Self {
        Self::new(self.data.clone(), self.build)
    }
}

impl<T: core::fmt::Debug, V> core::fmt::Debug for Lazy<T, V> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Lazy")
            .field("data", &self.data)
            .finish_non_exhaustive()
    }
}

impl<T, V> Lazy<T, V> {
    /// Creates a view that `build` makes from `data` when it is laid out and
    /// each time it is drawn.
    #[must_use]
    pub const fn new(data: T, build: fn(&T) -> V) -> Self {
        Self { data, build }
    }
}

impl<T: Clone, V> ViewMarker for Lazy<T, V> {
    type Renderables = render::Lazy<T, V>;
    type Transition = Opacity;
}

impl<Captures: ?Sized, T: Clone, V: ViewLayout<()>> ViewLayout<Captures> for Lazy<T, V> {
    type Sublayout = Dimensions;
    type State = ();
    type FocusTree = ();

    fn transition(&self) -> Self::Transition {
        Opacity
    }

    fn build_state(&self, _captures: &mut Captures) -> Self::State {}

    fn layout(
        &self,
        offer: &ProposedDimensions,
        env: &impl crate::environment::LayoutEnvironment,
        _captures: &mut Captures,
        _state: &mut Self::State,
    ) -> ResolvedLayout<Self::Sublayout> {
        let view = (self.build)(&self.data);
        let mut state = view.build_state(&mut ());
        // Drawn later, apart from the scroll view: it may not move it.
        let env = crate::environment::ViewportEnvironment::without_drift(env);
        let size = view.layout(offer, &env, &mut (), &mut state).resolved_size;
        ResolvedLayout {
            resolved_size: size,
            sublayouts: size,
        }
    }

    fn render_tree(
        &self,
        layout: &Self::Sublayout,
        origin: Point,
        _env: &impl crate::environment::LayoutEnvironment,
        _captures: &mut Captures,
        _state: &mut Self::State,
    ) -> Self::Renderables {
        render::Lazy::new(origin, *layout, self.data.clone(), self.build)
    }

    fn handle_event(
        &self,
        _event: &Event,
        _context: &EventContext,
        _render_tree: &mut Self::Renderables,
        _captures: &mut Captures,
        _state: &mut Self::State,
        _focus: &mut Self::FocusTree,
    ) -> EventResult {
        EventResult::default()
    }

    fn seek(
        &self,
        _seek: &Seek,
        _render_tree: &Self::Renderables,
        _state: &Self::State,
        _focus: Option<&Self::FocusTree>,
    ) -> SeekResult<Self::FocusTree> {
        SeekResult::Absent
    }

    fn handle_touch(
        &self,
        _touch: &Touch,
        _context: &EventContext,
        _render_tree: &mut Self::Renderables,
        _captures: &mut Captures,
        _state: &mut Self::State,
    ) -> TouchResult<Self::FocusTree> {
        TouchResult::Deferred
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::environment::DefaultEnvironment;
    use crate::view::prelude::*;

    #[allow(
        clippy::trivially_copy_pass_by_ref,
        reason = "the signature Lazy takes"
    )]
    fn badge(count: &u8) -> impl ViewLayout<()> + use<> {
        HStack::new((
            Rectangle.frame_sized(4, 4),
            (*count > 1).then(|| Rectangle.frame_sized(4, 4)),
        ))
        .with_spacing(2)
    }

    #[test]
    fn keeps_only_its_data() {
        let view = Lazy::new(2u8, badge);
        let env = DefaultEnvironment::non_animated();
        let offer = ProposedDimensions::new(40, 40);
        let layout = view.layout(&offer, &env, &mut (), &mut ());
        assert_eq!(layout.resolved_size, Dimensions::new(10, 4));
        let tree = view.render_tree(&layout.sublayouts, Point::new(3, 5), &env, &mut (), &mut ());
        assert_eq!(tree.origin, Point::new(3, 5));
        assert_eq!(tree.data, 2);
        assert_eq!(tree.size, layout.resolved_size);
    }
}
