use crate::{
    environment::LayoutEnvironment,
    event::{Event, EventResult, TouchResult},
    focus::{Seek, SeekResult},
    layout::ResolvedLayout,
    primitives::{Point, ProposedDimensions},
    view::{ViewLayout, ViewMarker},
};

use embedded_touch::Touch;

/// Says how much of a row's top is room above what it shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowInset<V> {
    inner: V,
    inset: u32,
}

impl<V: ViewMarker> RowInset<V> {
    #[allow(missing_docs)]
    #[must_use]
    pub const fn new(inner: V, inset: u32) -> Self {
        Self { inner, inset }
    }
}

impl<V> ViewMarker for RowInset<V>
where
    V: ViewMarker,
{
    type Renderables = V::Renderables;
    type Transition = V::Transition;
}

impl<Captures: ?Sized, V> ViewLayout<Captures> for RowInset<V>
where
    V: ViewLayout<Captures>,
{
    type Sublayout = V::Sublayout;
    type State = V::State;
    type FocusTree = V::FocusTree;

    fn priority(&self) -> i8 {
        self.inner.priority()
    }

    fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    fn row_inset(&self) -> u32 {
        self.inset
    }

    fn transition(&self) -> Self::Transition {
        self.inner.transition()
    }

    fn build_state(&self, captures: &mut Captures) -> Self::State {
        self.inner.build_state(captures)
    }

    fn layout(
        &self,
        offer: &ProposedDimensions,
        env: &impl LayoutEnvironment,
        captures: &mut Captures,
        state: &mut Self::State,
    ) -> ResolvedLayout<Self::Sublayout> {
        self.inner.layout(offer, env, captures, state)
    }

    fn render_tree(
        &self,
        layout: &Self::Sublayout,
        origin: Point,
        env: &impl LayoutEnvironment,
        captures: &mut Captures,
        state: &mut Self::State,
    ) -> Self::Renderables {
        self.inner.render_tree(layout, origin, env, captures, state)
    }

    fn handle_event(
        &self,
        event: &Event,
        context: &crate::event::EventContext,
        render_tree: &mut Self::Renderables,
        captures: &mut Captures,
        state: &mut Self::State,
        focus: &mut Self::FocusTree,
    ) -> EventResult {
        self.inner
            .handle_event(event, context, render_tree, captures, state, focus)
    }

    fn seek(
        &self,
        seek: &Seek,
        render_tree: &Self::Renderables,
        state: &Self::State,
        focus: Option<&Self::FocusTree>,
    ) -> SeekResult<Self::FocusTree> {
        self.inner.seek(seek, render_tree, state, focus)
    }

    fn handle_touch(
        &self,
        touch: &Touch,
        context: &crate::event::EventContext,
        render_tree: &mut Self::Renderables,
        captures: &mut Captures,
        state: &mut Self::State,
    ) -> TouchResult<Self::FocusTree> {
        self.inner
            .handle_touch(touch, context, render_tree, captures, state)
    }
}
