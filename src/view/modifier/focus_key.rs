use crate::{
    environment::LayoutEnvironment,
    event::{Event, EventContext, EventResult, TouchResult},
    focus::{DefaultFocus, FocusKey},
    layout::ResolvedLayout,
    primitives::{Point, ProposedDimensions},
    view::{ViewLayout, ViewMarker},
};

use embedded_touch::Touch;

/// Names the focusable element it wraps with a [`FocusKey`].
///
/// Results that focus the element carry the key, unless an element nearer
/// to the focus already named itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusKeyed<T> {
    child: T,
    key: FocusKey,
}

impl<T: ViewMarker> FocusKeyed<T> {
    #[must_use]
    pub const fn new(child: T, key: FocusKey) -> Self {
        Self { child, key }
    }
}

impl<T: ViewMarker> ViewMarker for FocusKeyed<T> {
    type Renderables = T::Renderables;
    type Transition = T::Transition;
}

impl<Captures: ?Sized, T: ViewLayout<Captures>> ViewLayout<Captures> for FocusKeyed<T>
where
    T::FocusTree: DefaultFocus,
{
    type Sublayout = T::Sublayout;
    type State = T::State;
    type FocusTree = T::FocusTree;

    fn priority(&self) -> i8 {
        self.child.priority()
    }

    fn is_empty(&self) -> bool {
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
        self.child.layout(offer, env, captures, state)
    }

    fn render_tree(
        &self,
        layout: &Self::Sublayout,
        origin: Point,
        env: &impl LayoutEnvironment,
        captures: &mut Captures,
        state: &mut Self::State,
    ) -> Self::Renderables {
        self.child.render_tree(layout, origin, env, captures, state)
    }

    fn handle_event(
        &self,
        event: &Event,
        context: &EventContext,
        render_tree: &mut Self::Renderables,
        captures: &mut Captures,
        state: &mut Self::State,
        focus: &mut Self::FocusTree,
    ) -> EventResult {
        self.child
            .handle_event(event, context, render_tree, captures, state, focus)
            .with_key(self.key)
    }

    fn handle_touch(
        &self,
        touch: &Touch,
        context: &EventContext,
        render_tree: &mut Self::Renderables,
        captures: &mut Captures,
        state: &mut Self::State,
    ) -> TouchResult<Self::FocusTree> {
        self.child
            .handle_touch(touch, context, render_tree, captures, state)
    }
}
