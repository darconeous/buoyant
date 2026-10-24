use crate::{
    environment::LayoutEnvironment,
    event::{EventResult, TouchResult},
    focus::{Seek, SeekResult},
    layout::ResolvedLayout,
    primitives::{Point, ProposedDimensions},
    view::{ViewLayout, ViewMarker},
};

use embedded_touch::Touch;

/// A view that uses the layout of the child view, but renders nothing
#[derive(Debug, Clone)]
pub struct Hidden<T> {
    child: T,
}

impl<T: ViewMarker> Hidden<T> {
    pub const fn new(child: T) -> Self {
        Self { child }
    }
}

impl<T> ViewMarker for Hidden<T>
where
    T: ViewMarker,
{
    type Renderables = (); // Render nothing
    type Transition = T::Transition;
}

impl<Captures: ?Sized, T> ViewLayout<Captures> for Hidden<T>
where
    T: ViewLayout<Captures>,
{
    type Sublayout = ();
    type State = T::State;
    type FocusTree = ();

    fn priority(&self) -> i8 {
        self.child.priority()
    }

    fn is_empty(&self) -> bool {
        // This is still useful because the empty property affects stack spacing
        self.child.is_empty()
    }

    fn transition(&self) -> Self::Transition {
        self.child.transition()
    }

    fn build_state(&self, captures: &mut Captures) -> Self::State {
        self.child.build_state(captures)
    }
    fn layout(
        &self,
        offer: &ProposedDimensions,
        env: &impl LayoutEnvironment,
        captures: &mut Captures,
        state: &mut Self::State,
    ) -> ResolvedLayout<Self::Sublayout> {
        // Laid out, never drawn: nothing in it may move the scroll view.
        let env = crate::environment::ViewportEnvironment::without_drift(env);
        let layout = self.child.layout(offer, &env, captures, state);
        ResolvedLayout {
            sublayouts: (),
            resolved_size: layout.resolved_size,
        }
    }

    fn render_tree(
        &self,
        _layout: &Self::Sublayout,
        _origin: Point,
        _env: &impl LayoutEnvironment,
        _captures: &mut Captures,
        _state: &mut Self::State,
    ) -> Self::Renderables {
    }

    fn handle_event(
        &self,
        _event: &crate::view::Event,
        _context: &crate::event::EventContext,
        _render_tree: &mut Self::Renderables,
        _captures: &mut Captures,
        _state: &mut Self::State,
        _focus: &mut Self::FocusTree,
    ) -> EventResult {
        EventResult::Deferred
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
        _context: &crate::event::EventContext,
        _render_tree: &mut Self::Renderables,
        _captures: &mut Captures,
        _state: &mut Self::State,
    ) -> TouchResult<Self::FocusTree> {
        TouchResult::Deferred
    }
}
