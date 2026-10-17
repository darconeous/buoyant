use crate::{
    environment::LayoutEnvironment,
    event::{Event, EventContext, EventResult, Key, KeyResponse, TouchResult},
    focus::{Seek, SeekResult},
    layout::ResolvedLayout,
    primitives::{Point, ProposedDimensions},
    view::{ViewLayout, ViewMarker},
};

use embedded_touch::Touch;

/// Handles the keys its subtree leaves unhandled.
///
/// Keys go to the focused element first; the handler sees a key only when
/// that element and everything between it and this view defer it. Only key
/// presses reach the handler: releases pass through untouched, so one press
/// never acts twice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnKey<T, F> {
    child: T,
    handler: F,
}

impl<T: ViewMarker, F> OnKey<T, F> {
    #[must_use]
    pub const fn new(child: T, handler: F) -> Self {
        Self { child, handler }
    }
}

impl<T: ViewMarker, F> ViewMarker for OnKey<T, F> {
    type Renderables = T::Renderables;
    type Transition = T::Transition;
}

impl<Captures: ?Sized, T: ViewLayout<Captures>, F> ViewLayout<Captures> for OnKey<T, F>
where
    F: Fn(Key, &mut Captures) -> KeyResponse,
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
        let result = self
            .child
            .handle_event(event, context, render_tree, captures, state, focus);
        let Event::KeyDown(key) = event else {
            return result;
        };
        if result.is_handled() {
            return result;
        }
        match (self.handler)(*key, captures) {
            KeyResponse::Ignored => EventResult::Deferred,
            KeyResponse::Handled => {
                context.request_view_rebuild();
                EventResult::handled_unfocused()
            }
            KeyResponse::Redirect => {
                context.request_view_rebuild();
                context.request_redirect();
                EventResult::handled_unfocused()
            }
        }
    }

    fn seek(
        &self,
        seek: &Seek,
        render_tree: &Self::Renderables,
        state: &Self::State,
        focus: Option<&Self::FocusTree>,
    ) -> SeekResult<Self::FocusTree> {
        self.child.seek(seek, render_tree, state, focus)
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
