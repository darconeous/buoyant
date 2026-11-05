use core::time::Duration;

use crate::{
    environment::LayoutEnvironment,
    event::{Event, EventResult, TouchResult},
    focus::{Seek, SeekResult},
    layout::ResolvedLayout,
    primitives::{Point, ProposedDimensions},
    render,
    view::{ViewLayout, ViewMarker},
};

use embedded_touch::Touch;

/// Fades its content in when a value changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FadeOnChange<V, T> {
    inner: V,
    value: T,
    duration: Duration,
}

impl<V: ViewMarker, T> FadeOnChange<V, T> {
    #[allow(missing_docs)]
    #[must_use]
    pub const fn new(inner: V, value: T, duration: Duration) -> Self {
        Self {
            inner,
            value,
            duration,
        }
    }
}

/// The value last seen, the fade it began, and the content's state.
#[derive(Debug, Clone)]
pub struct FadeOnChangeState<T, S> {
    value: Option<T>,
    started: Option<Duration>,
    inner: S,
}

impl<V, T> ViewMarker for FadeOnChange<V, T>
where
    V: ViewMarker,
{
    type Renderables = render::FadeIn<V::Renderables>;
    type Transition = V::Transition;
}

impl<Captures: ?Sized, V, T> ViewLayout<Captures> for FadeOnChange<V, T>
where
    V: ViewLayout<Captures>,
    T: PartialEq + Clone + 'static,
{
    type Sublayout = V::Sublayout;
    type State = FadeOnChangeState<T, V::State>;
    type FocusTree = V::FocusTree;

    fn priority(&self) -> i8 {
        self.inner.priority()
    }

    fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    fn transition(&self) -> Self::Transition {
        self.inner.transition()
    }

    fn build_state(&self, captures: &mut Captures) -> Self::State {
        FadeOnChangeState {
            value: None,
            started: None,
            inner: self.inner.build_state(captures),
        }
    }

    fn layout(
        &self,
        offer: &ProposedDimensions,
        env: &impl LayoutEnvironment,
        captures: &mut Captures,
        state: &mut Self::State,
    ) -> ResolvedLayout<Self::Sublayout> {
        self.inner.layout(offer, env, captures, &mut state.inner)
    }

    fn render_tree(
        &self,
        layout: &Self::Sublayout,
        origin: Point,
        env: &impl LayoutEnvironment,
        captures: &mut Captures,
        state: &mut Self::State,
    ) -> Self::Renderables {
        // First seen, the content is simply there: only a change fades.
        if state.value.as_ref() != Some(&self.value) {
            if state.value.is_some() {
                state.started = Some(env.app_time());
            }
            state.value = Some(self.value.clone());
        }
        render::FadeIn {
            subtree: self
                .inner
                .render_tree(layout, origin, env, captures, &mut state.inner),
            started: state.started,
            duration: self.duration,
        }
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
        self.inner.handle_event(
            event,
            context,
            &mut render_tree.subtree,
            captures,
            &mut state.inner,
            focus,
        )
    }

    fn seek(
        &self,
        seek: &Seek,
        render_tree: &Self::Renderables,
        state: &Self::State,
        focus: Option<&Self::FocusTree>,
    ) -> SeekResult<Self::FocusTree> {
        self.inner
            .seek(seek, &render_tree.subtree, &state.inner, focus)
    }

    fn handle_touch(
        &self,
        touch: &Touch,
        context: &crate::event::EventContext,
        render_tree: &mut Self::Renderables,
        captures: &mut Captures,
        state: &mut Self::State,
    ) -> TouchResult<Self::FocusTree> {
        self.inner.handle_touch(
            touch,
            context,
            &mut render_tree.subtree,
            captures,
            &mut state.inner,
        )
    }
}
