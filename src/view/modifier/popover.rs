use core::time::Duration;

use crate::{
    animation::Animation,
    event::{Event, EventContext, EventResult, TouchResult},
    focus::{
        BoundaryBehavior, DefaultFocus, FocusAction, FocusDirection, Seek, SeekResult, SeekTarget,
    },
    layout::{HorizontalAlignment, ResolvedLayout, VerticalAlignment},
    primitives::Point,
    render::{Animate, IntrinsicShape, TransitionOption},
    view::{ViewLayout, ViewMarker},
};

use embedded_touch::{Phase, Touch};

/// The decision to either retain or dismiss the active popover on blur
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dismissal {
    /// Dismiss the popover, returning focus to the underlying view.
    Dismiss,
    /// Keep the overlay focused;
    Retain,
}

/// A callback invoked when a popover's overlay receives a `Blur` event, or a
/// tap outside it.
///
/// The handler may mutate captured state (e.g. clearing the value that drives
/// the overlay's visibility) and then returns a [`Dismissal`] to indicate
/// whether the overlay should actually be dismissed.
pub trait OnBlur<Captures: ?Sized> {
    /// Called when the overlay is blurred. Returns the dismissal decision.
    fn on_blur(&self, captures: &mut Captures) -> Dismissal;
}

impl<C: ?Sized, F> OnBlur<C> for F
where
    F: Fn(&mut C) -> Dismissal,
{
    fn on_blur(&self, captures: &mut C) -> Dismissal {
        (self)(captures)
    }
}

/// An [`OnBlur`] implementation that does not dismiss on blur. This is the
/// default behavior.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoDismiss;

impl<C: ?Sized> OnBlur<C> for NoDismiss {
    fn on_blur(&self, _captures: &mut C) -> Dismissal {
        Dismissal::Retain
    }
}

#[derive(Debug, Clone)]
pub struct Popover<Inner, Overlay, Action = NoDismiss> {
    inner: Inner,
    overlay: Option<Overlay>,
    boundary_behavior: BoundaryBehavior,
    on_blur: Action,
    animation: Animation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusTree<T, U> {
    /// The inner view's focus state (always preserved)
    pub inner: T,
    /// The overlay's focus state (when overlay is active)
    pub overlay: Option<U>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PopoverState<T, U, F> {
    pub inner_state: T,
    pub overlay_state: Option<U>,
    /// The overlay was dismissed and focus returned to the inner view, but
    /// the view has not been rebuilt without it yet.
    pub dismissed: bool,
    /// Where focus is in the inner view while the overlay holds it, so a
    /// touch that moves focus within the overlay, or dismisses it, keeps it.
    opener: Option<F>,
    /// A touch that started outside the open overlay, which dismisses the
    /// overlay if it ends outside too.
    outside_touch: Option<u8>,
}

impl<T: DefaultFocus, U: DefaultFocus> DefaultFocus for FocusTree<T, U> {
    fn default_first() -> Self {
        Self {
            inner: DefaultFocus::default_first(),
            overlay: None,
        }
    }

    fn default_last() -> Self {
        Self {
            inner: DefaultFocus::default_last(),
            overlay: None,
        }
    }
}

impl<Inner, Overlay> Popover<Inner, Overlay, NoDismiss>
where
    Inner: ViewMarker,
    Overlay: ViewMarker,
{
    #[must_use]
    pub fn new<T, ViewFn>(inner: Inner, value: Option<&T>, view_fn: ViewFn) -> Self
    where
        ViewFn: for<'b> FnOnce(&'b T) -> Overlay,
        T: Clone,
    {
        let overlay = value.map(view_fn);
        Self {
            inner,
            overlay,
            boundary_behavior: BoundaryBehavior::Stop,
            on_blur: NoDismiss,
            animation: Animation::ease_out(Duration::from_millis(300)),
        }
    }
}

impl<Inner, Overlay, Action> Popover<Inner, Overlay, Action>
where
    Inner: ViewMarker,
    Overlay: ViewMarker,
{
    /// Modifies the behavior when attempting to move focus beyond the ends of the overlay.
    #[must_use]
    pub fn with_boundary_behavior(mut self, behavior: BoundaryBehavior) -> Self {
        self.boundary_behavior = behavior;
        self
    }

    /// Installs a callback invoked when the overlay receives a `Blur` event,
    /// or is tapped outside. This should be used to update state backing the
    /// popover and decide whether the overlay should be dismissed.
    ///
    /// Returning [`Dismissal::Dismiss`] drops the overlay's focus subtree,
    /// returns focus to where it was in the inner view, and requests a view
    /// rebuild; returning [`Dismissal::Retain`] keeps the
    /// overlay mounted and focused.
    ///
    /// `Teardown` events do not invoke the callback: the overlay's focus tree
    /// is simply dropped along with the rest of the stale focus tree.
    #[must_use]
    pub fn on_blur<F: Fn(&mut C) -> Dismissal, C>(self, on_blur: F) -> Popover<Inner, Overlay, F>
    where
        Self: ViewLayout<C>,
    {
        Popover {
            inner: self.inner,
            overlay: self.overlay,
            boundary_behavior: self.boundary_behavior,
            on_blur,
            animation: self.animation,
        }
    }

    /// Sets a custom animation curve on the popover presentation and dismissal.
    #[must_use]
    pub fn with_animation(self, animation: Animation) -> Self {
        Self { animation, ..self }
    }

    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    fn handle_overlay_focus_event<Captures: ?Sized>(
        &self,
        overlay_view: &Overlay,
        focus_event: FocusAction,
        group: crate::focus::FocusGroup,
        context: &EventContext,
        render_tree: &mut <Self as ViewMarker>::Renderables,
        captures: &mut Captures,
        state: &mut PopoverState<Inner::State, Overlay::State, Inner::FocusTree>,
        focus: &mut FocusTree<Inner::FocusTree, Overlay::FocusTree>,
    ) -> EventResult
    where
        Inner: ViewLayout<Captures>,
        Overlay: ViewLayout<Captures>,
        Action: OnBlur<Captures>,
        Inner::Renderables: IntrinsicShape,
    {
        state.opener = Some(focus.inner.clone());
        let subfocus = focus
            .overlay
            .get_or_insert_with(DefaultFocus::default_first);
        let overlay_state = state
            .overlay_state
            .get_or_insert_with(|| overlay_view.build_state(captures));

        let TransitionOption::Some { subtree, .. } = &mut render_tree.1.subtree else {
            // Open but not rendered yet. Focus still belongs to the overlay,
            // so nothing behind it is offered the event.
            return match focus_event {
                FocusAction::Teardown => EventResult::Deferred,
                _ => EventResult::handled_unfocused(),
            };
        };

        let result = overlay_view.handle_event(
            &Event::Focus {
                action: focus_event,
                group,
            },
            context,
            subtree,
            captures,
            overlay_state,
            subfocus,
        );

        if result != EventResult::Deferred {
            return result;
        }

        // The overlay's focused subtree did not handle the event.
        match focus_event {
            FocusAction::Blur => {
                match self.on_blur.on_blur(captures) {
                    Dismissal::Dismiss => {
                        // Drop the overlay focus subtree and return focus to
                        // where it was in the inner view, so the result
                        // names the opener.
                        focus.overlay = None;
                        state.dismissed = true;
                        context.request_view_rebuild();
                        self.inner.handle_event(
                            &Event::Focus {
                                action: FocusAction::Focus(FocusDirection::Forward),
                                group,
                            },
                            context,
                            &mut render_tree.0,
                            captures,
                            &mut state.inner_state,
                            &mut focus.inner,
                        )
                    }
                    Dismissal::Retain => EventResult::handled_unfocused(),
                }
            }
            FocusAction::Teardown => EventResult::Deferred,
            _ => {
                let is_forward = focus_event.direction() == Some(FocusDirection::Forward);
                // Focus stays in the overlay.
                let acquire_direction = if let FocusAction::Toward { .. } = focus_event {
                    // A directional move never wraps: focus returns to the
                    // element at the edge it reached.
                    if is_forward {
                        FocusDirection::Backward
                    } else {
                        FocusDirection::Forward
                    }
                } else {
                    // Anything else wraps around to the other end.
                    *subfocus = if is_forward {
                        DefaultFocus::default_first()
                    } else {
                        DefaultFocus::default_last()
                    };
                    if is_forward {
                        FocusDirection::Forward
                    } else {
                        FocusDirection::Backward
                    }
                };
                let result = overlay_view.handle_event(
                    &Event::Focus {
                        action: FocusAction::Focus(acquire_direction),
                        group,
                    },
                    context,
                    subtree,
                    captures,
                    overlay_state,
                    subfocus,
                );
                // An overlay with nothing focusable still holds focus: the
                // views behind it are not offered it.
                if result.is_handled() {
                    result
                } else {
                    EventResult::handled_unfocused()
                }
            }
        }
    }

    /// A tap outside the open overlay, which dismisses it as a blur does.
    fn tap_outside<Captures: ?Sized>(
        &self,
        context: &EventContext,
        captures: &mut Captures,
        state: &mut PopoverState<Inner::State, Overlay::State, Inner::FocusTree>,
    ) -> TouchResult<FocusTree<Inner::FocusTree, Overlay::FocusTree>>
    where
        Inner: ViewLayout<Captures>,
        Overlay: ViewLayout<Captures>,
        Action: OnBlur<Captures>,
    {
        match self.on_blur.on_blur(captures) {
            Dismissal::Dismiss => {
                state.dismissed = true;
                context.request_view_rebuild();
                TouchResult::Focused(FocusTree {
                    inner: state
                        .opener
                        .take()
                        .unwrap_or_else(DefaultFocus::default_first),
                    overlay: None,
                })
            }
            Dismissal::Retain => TouchResult::Handled,
        }
    }
}

impl<Inner: ViewMarker, Overlay: ViewMarker, Action> ViewMarker
    for Popover<Inner, Overlay, Action>
{
    type Renderables = (
        Inner::Renderables,
        Animate<TransitionOption<Overlay::Renderables, Overlay::Transition>, bool>,
    );

    type Transition = Inner::Transition;
}

impl<Captures, Inner, Overlay, Action> ViewLayout<Captures> for Popover<Inner, Overlay, Action>
where
    Captures: ?Sized,
    Inner: ViewLayout<Captures>,
    Overlay: ViewLayout<Captures>,
    Action: OnBlur<Captures>,
    Inner::Renderables: IntrinsicShape,
    Overlay::Renderables: IntrinsicShape,
{
    type State = PopoverState<Inner::State, Overlay::State, Inner::FocusTree>;
    type Sublayout = ResolvedLayout<Inner::Sublayout>;
    type FocusTree = FocusTree<Inner::FocusTree, Overlay::FocusTree>;

    fn transition(&self) -> Self::Transition {
        self.inner.transition()
    }

    fn priority(&self) -> i8 {
        self.inner.priority()
    }

    fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    fn build_state(&self, captures: &mut Captures) -> Self::State {
        Self::State {
            inner_state: self.inner.build_state(captures),
            overlay_state: self.overlay.as_ref().map(|o| o.build_state(captures)),
            dismissed: false,
            opener: None,
            outside_touch: None,
        }
    }

    fn layout(
        &self,
        offer: &crate::primitives::ProposedDimensions,
        env: &impl crate::environment::LayoutEnvironment,
        captures: &mut Captures,
        state: &mut Self::State,
    ) -> ResolvedLayout<Self::Sublayout> {
        self.inner
            .layout(offer, env, captures, &mut state.inner_state)
            .nested()
    }

    fn render_tree(
        &self,
        layout: &Self::Sublayout,
        origin: crate::primitives::Point,
        env: &impl crate::environment::LayoutEnvironment,
        captures: &mut Captures,
        state: &mut Self::State,
    ) -> Self::Renderables {
        let inner_tree = self.inner.render_tree(
            &layout.sublayouts,
            origin,
            env,
            captures,
            &mut state.inner_state,
        );
        // A rebuilt view settles any dismissal: an overlay still present is
        // open again and takes focus back.
        state.dismissed = false;
        let overlay_tree = if let Some(overlay_view) = &self.overlay {
            let overlay_state = state
                .overlay_state
                .get_or_insert_with(|| overlay_view.build_state(captures));
            let overlay_layout =
                overlay_view.layout(&layout.resolved_size.into(), env, captures, overlay_state);
            let origin = origin
                + Point::new(
                    HorizontalAlignment::Center.align(
                        layout.resolved_size.width.into(),
                        overlay_layout.resolved_size.width.into(),
                    ),
                    VerticalAlignment::Center.align(
                        layout.resolved_size.height.into(),
                        overlay_layout.resolved_size.height.into(),
                    ),
                );
            TransitionOption::new_some(
                overlay_view.render_tree(
                    &overlay_layout.sublayouts,
                    origin,
                    env,
                    captures,
                    overlay_state,
                ),
                layout.resolved_size.into(),
                overlay_view.transition(),
            )
        } else {
            state.overlay_state = None;
            state.opener = None;
            state.outside_touch = None;
            TransitionOption::None
        };
        (
            inner_tree,
            Animate::new(
                overlay_tree,
                self.animation.clone(),
                env.app_time(),
                self.overlay.is_some(),
            ),
        )
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
        // Focus events go where the focus tree says focus is.
        if let Event::Focus {
            action: focus_event,
            group,
        } = event
        {
            if let Some(overlay_view) = self.overlay.as_ref().filter(|_| !state.dismissed) {
                if focus.overlay.is_none() && *focus_event != FocusAction::Teardown {
                    // The overlay opened while focus was in the inner view.
                    // Release the inner view, then move focus into the overlay.
                    _ = self.inner.handle_event(
                        &Event::Focus {
                            action: FocusAction::Teardown,
                            group: *group,
                        },
                        context,
                        &mut render_tree.0,
                        captures,
                        &mut state.inner_state,
                        &mut focus.inner,
                    );
                    let direction = match focus_event {
                        FocusAction::Focus(direction) => *direction,
                        _ => FocusDirection::Forward,
                    };
                    return self.handle_overlay_focus_event(
                        overlay_view,
                        FocusAction::Focus(direction),
                        *group,
                        context,
                        render_tree,
                        captures,
                        state,
                        focus,
                    );
                }
                if focus.overlay.is_some() {
                    return self.handle_overlay_focus_event(
                        overlay_view,
                        *focus_event,
                        *group,
                        context,
                        render_tree,
                        captures,
                        state,
                        focus,
                    );
                }
            } else {
                // No overlay is open: focus is in the inner view.
                focus.overlay = None;
            }
            if self.overlay.is_none() {
                state.overlay_state = None;
            }

            return self.inner.handle_event(
                &Event::Focus {
                    action: *focus_event,
                    group: *group,
                },
                context,
                &mut render_tree.0,
                captures,
                &mut state.inner_state,
                &mut focus.inner,
            );
        }

        let overlay = self.overlay.as_ref().filter(|_| !state.dismissed);
        match (overlay, &mut render_tree.1.subtree) {
            (Some(overlay_view), TransitionOption::Some { subtree, .. }) => {
                let overlay_state = state
                    .overlay_state
                    .get_or_insert_with(|| overlay_view.build_state(captures));
                overlay_view.handle_event(
                    event,
                    context,
                    subtree,
                    captures,
                    overlay_state,
                    focus.overlay.get_or_insert(DefaultFocus::default_first()),
                )
            }
            (Some(_), TransitionOption::None) => {
                // Overlay is present but not rendered yet - defer events until it actually appears
                EventResult::Deferred
            }
            _ => self.inner.handle_event(
                event,
                context,
                &mut render_tree.0,
                captures,
                &mut state.inner_state,
                &mut focus.inner,
            ),
        }
    }

    fn seek(
        &self,
        seek: &Seek,
        render_tree: &Self::Renderables,
        state: &Self::State,
        focus: Option<&Self::FocusTree>,
    ) -> SeekResult<Self::FocusTree> {
        let inner_focus = focus.map(|focus| &focus.inner);
        let open = self.overlay.as_ref().filter(|_| !state.dismissed);
        let (Some(overlay_view), TransitionOption::Some { subtree, .. }, Some(overlay_state)) =
            (open, &render_tree.1.subtree, state.overlay_state.as_ref())
        else {
            return self
                .inner
                .seek(seek, &render_tree.0, &state.inner_state, inner_focus)
                .map(|inner| FocusTree {
                    inner,
                    overlay: None,
                });
        };

        let overlay_focus = focus.and_then(|focus| focus.overlay.as_ref());
        // A path into the overlay keeps where focus was in the inner view, so
        // dismissing the overlay returns there.
        let into_overlay = |overlay: Overlay::FocusTree| FocusTree {
            inner: inner_focus
                .cloned()
                .unwrap_or_else(DefaultFocus::default_first),
            overlay: Some(overlay),
        };
        // An overlay with nothing focusable still takes focus.
        let enter = || match overlay_view.seek(
            &seek.with_target(SeekTarget::Any),
            subtree,
            overlay_state,
            None,
        ) {
            found @ SeekResult::Found { .. } => found.map(into_overlay),
            _ => SeekResult::found(into_overlay(DefaultFocus::default_first())),
        };

        match (
            seek.target,
            overlay_view.seek(seek, subtree, overlay_state, overlay_focus),
        ) {
            (_, found @ SeekResult::Found { .. }) => found.map(into_overlay),
            (_, SeekResult::Current) => SeekResult::Current,
            // Focus is already in this overlay, and no modal inside it is open.
            (SeekTarget::Modal, _) if overlay_focus.is_some() => SeekResult::Current,
            (SeekTarget::Modal | SeekTarget::Any, _) => enter(),
            // Nothing outside an open overlay can be focused.
            (SeekTarget::Key(_) | SeekTarget::At(_), _) => SeekResult::Blocked,
        }
    }

    fn handle_touch(
        &self,
        touch: &Touch,
        context: &EventContext,
        render_tree: &mut Self::Renderables,
        captures: &mut Captures,
        state: &mut Self::State,
    ) -> TouchResult<Self::FocusTree> {
        // An open overlay takes every touch: nothing behind it is touched.
        if let Some(overlay_view) = self.overlay.as_ref().filter(|_| !state.dismissed)
            && let TransitionOption::Some { subtree, .. } = &mut render_tree.1.subtree
        {
            let inside = subtree.content_shape().contains(touch.location.into());
            if state.outside_touch == Some(touch.id) {
                return match touch.phase {
                    Phase::Ended => {
                        state.outside_touch = None;
                        if inside {
                            TouchResult::Handled
                        } else {
                            self.tap_outside(context, captures, state)
                        }
                    }
                    Phase::Cancelled => {
                        state.outside_touch = None;
                        TouchResult::Handled
                    }
                    Phase::Started => {
                        state.outside_touch = (!inside).then_some(touch.id);
                        TouchResult::Handled
                    }
                    Phase::Moved | Phase::Hovering(_) => TouchResult::Handled,
                };
            }
            let overlay_state = state
                .overlay_state
                .get_or_insert_with(|| overlay_view.build_state(captures));
            return match overlay_view.handle_touch(touch, context, subtree, captures, overlay_state)
            {
                TouchResult::Focused(f) => TouchResult::Focused(FocusTree {
                    inner: state
                        .opener
                        .clone()
                        .unwrap_or_else(DefaultFocus::default_first),
                    overlay: Some(f),
                }),
                TouchResult::Handled => TouchResult::Handled,
                TouchResult::Deferred if touch.phase == Phase::Started && !inside => {
                    state.outside_touch = Some(touch.id);
                    TouchResult::Handled
                }
                TouchResult::Deferred => TouchResult::Deferred,
            };
        }
        match self.inner.handle_touch(
            touch,
            context,
            &mut render_tree.0,
            captures,
            &mut state.inner_state,
        ) {
            TouchResult::Focused(f) => TouchResult::Focused(FocusTree {
                inner: f,
                overlay: None,
            }),
            TouchResult::Handled => TouchResult::Handled,
            TouchResult::Deferred => TouchResult::Deferred,
        }
    }
}
