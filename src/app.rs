//! [`App`] manages the view/render tree lifecycle and event handling.
//!
//! The [`Harness`] trait provides a set of convenience methods for navigating
//! and interacting with the UI, with default implementations built on top of
//! [`Harness::send`] and [`Harness::send_with_group`].

use core::mem::MaybeUninit;
use core::time::Duration;

use crate::{
    environment::DefaultEnvironment,
    event::{Event, EventContext, EventResult, TouchResult},
    focus::{
        DefaultFocus, Direction, FocusAction, FocusDirection, FocusGroup, FocusKey, FocusState,
        Role, RoleSet, Seek, SeekResult, SeekTarget,
    },
    primitives::{Point, Size, geometry::Rectangle, transform::LinearTransform},
    render::{AnimatedJoin, AnimationDomain, ContentShape, Render},
    render_target::{RenderTarget, SolidBrush, Stroke},
    view::{View, ViewLayout},
};
use embedded_touch::{Phase, Tool, Touch};

mod harness;
pub use harness::Harness;

mod tracked;
pub use tracked::Tracked;

mod trees;
pub use trees::Trees;

/// Manages the view/render tree lifecycle, focus state, and event handling.
///
/// [`App`] owns the view function, application state, and the source/target render
/// trees needed for animated transitions.
///
/// The [`Harness`] trait is implemented to provide a set of convenience methods for
/// sending events.
///
/// # Rendering
///
///
pub struct App<V, S, F>
where
    V: ViewLayout<S>,
{
    /// The application state
    state: S,
    view_fn: F,
    view: V,
    view_state: V::State,
    trees: Trees<V::Renderables>,
    focus_tree: V::FocusTree,
    env: DefaultEnvironment,
    /// Current focus shape for overlay rendering.
    focus_shape: ContentShape,
    /// The key of the focused element, when it has one.
    focused_key: Option<FocusKey>,
    /// The focus group the focused element belongs to.
    focus_group: FocusGroup,
    /// Where the application keeps its [`FocusState`].
    focus_binding: Option<fn(&mut S) -> &mut FocusState>,
    /// A request made with [`focus`](Self::focus) without a binding.
    focus_request: Option<FocusKey>,
    /// A redirected key is being delivered.
    redirecting: bool,
    /// The touch in progress, from when it started until it ends.
    touch: Option<Touch>,
    /// A touch cancelled while in progress, until it ends.
    cancelled: Option<u8>,
    roles: RoleSet,
    /// The size touch targets are touched as being at least.
    min_touch_target: Size,
    size: Size,
    elapsed: Duration,
    requires_redraw: bool,
    requires_rebuild: bool,
}

impl<V, S, F> core::fmt::Debug for App<V, S, F>
where
    V: ViewLayout<S> + core::fmt::Debug,
    V::State: core::fmt::Debug,
    V::Renderables: core::fmt::Debug,
    V::FocusTree: core::fmt::Debug,
    S: core::fmt::Debug,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("App")
            .field("state", &self.state)
            .field("env", &self.env)
            .field("focus_shape", &self.focus_shape)
            .field("roles", &self.roles)
            .field("size", &self.size)
            .field("elapsed", &self.elapsed)
            .field("requires_redraw", &self.requires_redraw)
            .field("requires_rebuild", &self.requires_rebuild)
            .finish_non_exhaustive()
    }
}

impl<V, S, F> App<V, S, F>
where
    V: ViewLayout<S>,
    V::FocusTree: DefaultFocus,
    V::Renderables: AnimatedJoin,
    F: Fn(&S) -> V,
{
    /// Creates a new `App` with the given initial state, display size, and view function.
    pub fn new(state: S, size: Size, view_fn: F) -> Self
    where
        S: 'static,
    {
        let view = (view_fn)(&state);
        let env = DefaultEnvironment::non_animated();
        let mut state = state;
        let mut view_state = view.build_state(&mut state);
        let layout = view.layout(&size.into(), &env, &mut state, &mut view_state);
        let source_tree = view.render_tree(
            &layout.sublayouts,
            Point::zero(),
            &env,
            &mut state,
            &mut view_state,
        );
        let target_tree = view.render_tree(
            &layout.sublayouts,
            Point::zero(),
            &env,
            &mut state,
            &mut view_state,
        );
        let focus_tree = DefaultFocus::default_first();
        let roles = Role::Button | Role::Container;

        Self {
            state,
            view_fn,
            view,
            view_state,
            trees: Trees::new(source_tree, target_tree),
            focus_tree,
            env,
            roles,
            focus_shape: ContentShape::Empty,
            focused_key: None,
            focus_group: crate::focus::GROUP_0,
            focus_binding: None,
            focus_request: None,
            redirecting: false,
            touch: None,
            cancelled: None,
            min_touch_target: Size::zero(),
            size,
            elapsed: Duration::default(),
            requires_redraw: true,
            requires_rebuild: false,
        }
    }

    /// Creates an `App` in `slot`, like [`new`](Self::new), and returns it.
    ///
    /// An `App` holds the state, the view, and two render trees, which
    /// together can be larger than a small device's stack. Returned by value,
    /// it is assembled on the stack and copied into place; this builds each
    /// part where it stays, so the stack holds only the layout and one
    /// render tree while it is built.
    #[allow(unsafe_code)]
    pub fn new_in(slot: &mut MaybeUninit<Self>, state: S, size: Size, view_fn: F) -> &mut Self
    where
        S: 'static,
    {
        // Does not compile when a field is added, so that it is written below.
        let _ = |app: Self| {
            let Self {
                state: _,
                view_fn: _,
                view: _,
                view_state: _,
                trees: _,
                focus_tree: _,
                env: _,
                focus_shape: _,
                focused_key: _,
                focus_group: _,
                focus_binding: _,
                focus_request: _,
                redirecting: _,
                touch: _,
                cancelled: _,
                roles: _,
                min_touch_target: _,
                size: _,
                elapsed: _,
                requires_redraw: _,
                requires_rebuild: _,
            } = app;
        };
        let app = slot.as_mut_ptr();
        // SAFETY: each field is written once, through a pointer to that field
        // alone, before anything reads it, and every field is written before
        // the slot is treated as initialized. The references taken along the
        // way are to distinct fields, so none alias. Should building panic,
        // the slot stays uninitialized and the fields written so far leak.
        unsafe {
            (&raw mut (*app).state).write(state);
            (&raw mut (*app).view_fn).write(view_fn);
            (&raw mut (*app).env).write(DefaultEnvironment::non_animated());
            let state = &mut (*app).state;
            let env = &(*app).env;
            (&raw mut (*app).view).write(((*app).view_fn)(state));
            let view = &(*app).view;
            (&raw mut (*app).view_state).write(view.build_state(state));
            let view_state = &mut (*app).view_state;
            let layout = view.layout(&size.into(), env, state, view_state);
            let trees = &raw mut (*app).trees;
            Trees::write_in(trees, || {
                view.render_tree(&layout.sublayouts, Point::zero(), env, state, view_state)
            });
            (&raw mut (*app).focus_tree).write(DefaultFocus::default_first());
            (&raw mut (*app).focus_shape).write(ContentShape::Empty);
            (&raw mut (*app).focused_key).write(None);
            (&raw mut (*app).focus_group).write(crate::focus::GROUP_0);
            (&raw mut (*app).focus_binding).write(None);
            (&raw mut (*app).focus_request).write(None);
            (&raw mut (*app).redirecting).write(false);
            (&raw mut (*app).touch).write(None);
            (&raw mut (*app).cancelled).write(None);
            (&raw mut (*app).roles).write(Role::Button | Role::Container);
            (&raw mut (*app).min_touch_target).write(Size::zero());
            (&raw mut (*app).size).write(size);
            (&raw mut (*app).elapsed).write(Duration::default());
            (&raw mut (*app).requires_redraw).write(true);
            (&raw mut (*app).requires_rebuild).write(false);
            slot.assume_init_mut()
        }
    }

    /// Sets the default roles for focus navigation.
    ///
    /// Roles determine which types of focusable elements (buttons, containers, etc.)
    /// respond to focus events.
    #[must_use]
    pub fn with_roles(mut self, roles: impl Into<RoleSet>) -> Self {
        self.set_roles(roles);
        self
    }

    /// Sets the default roles for focus navigation, as
    /// [`with_roles`](Self::with_roles) does.
    pub fn set_roles(&mut self, roles: impl Into<RoleSet>) {
        self.roles = roles.into();
    }

    /// Touches targets smaller than `size`, such as small buttons, as if
    /// they were grown to it around their centers, as Jetpack Compose's
    /// minimum touch target does. Layout and drawing are unchanged.
    ///
    /// A touch in a target's own area goes to that target; one that starts
    /// only in grown areas goes to the nearest of them. Touches from a
    /// pointer, such as a mouse, are precise and grow nothing. Off by
    /// default.
    #[must_use]
    pub fn with_min_touch_target(mut self, size: Size) -> Self {
        self.set_min_touch_target(size);
        self
    }

    /// Sets the minimum touch target, as
    /// [`with_min_touch_target`](Self::with_min_touch_target) does. A zero
    /// size turns it off.
    pub fn set_min_touch_target(&mut self, size: Size) {
        self.min_touch_target = size;
    }

    /// Initializes the focus tree to the last element.
    #[must_use]
    pub fn with_focus_at_end(mut self) -> Self {
        self.focus_tree = DefaultFocus::default_last();
        self
    }

    /// Keeps focus in application state, at the [`FocusState`] `binding`
    /// returns.
    ///
    /// The app records there the key of the element focus settles on, and
    /// acts on the requests made there. The binding is called afresh each
    /// time, so it can reach wherever the state currently is, such as the top
    /// screen of a navigation stack.
    #[must_use]
    pub fn with_focus_binding(mut self, binding: fn(&mut S) -> &mut FocusState) -> Self {
        self.set_focus_binding(binding);
        self
    }

    /// Keeps focus in application state, as
    /// [`with_focus_binding`](Self::with_focus_binding) does.
    pub fn set_focus_binding(&mut self, binding: fn(&mut S) -> &mut FocusState) {
        self.focus_binding = Some(binding);
    }

    /// Asks for focus to move to the element with `key`, and settles focus.
    ///
    /// This makes the same request as [`FocusState::request`]. Nothing
    /// changes if the element is not in the view or an open modal keeps
    /// focus from it.
    pub fn focus(&mut self, key: FocusKey) {
        match self.focus_binding {
            Some(binding) => binding(&mut self.state).request(key),
            None => self.focus_request = Some(key),
        }
        self.finalize_view();
    }

    /// Moves focus on screen toward `dir`, from the focused element.
    ///
    /// Stacks moving along their axis move as [`next`](Harness::next) and
    /// [`previous`](Harness::previous) do, scrolling where those would; across
    /// their axis they leave the move to whatever holds focus in them, and
    /// focus entering a stack across its axis lands on the child nearest the
    /// element it left. See [`FocusAction::Toward`].
    ///
    /// The move is made in the group that holds focus. One that leads
    /// nowhere leaves focus where it was: directional moves never wrap.
    pub fn move_toward(&mut self, dir: Direction) -> EventResult {
        let from = self
            .focus_shape
            .bounding_box()
            .unwrap_or_else(|| Rectangle::new(Point::zero(), Size::zero()));
        let result = self.send(FocusAction::Toward { dir, from }.into_event(self.focus_group));
        if result.is_handled() {
            return result;
        }
        // Back onto the element at the edge the move reached.
        match dir.focus_direction() {
            FocusDirection::Forward => self.focus_backward_group(self.focus_group),
            FocusDirection::Backward => self.focus_forward_group(self.focus_group),
        }
    }

    /// Moves focus to the element under `point`, as a long press does, and
    /// settles focus.
    ///
    /// Returns whether an element that can be focused is there. Nothing
    /// changes if there is none, or an open modal keeps focus from it.
    pub fn focus_at(&mut self, point: Point) -> bool {
        if !self.seek(SeekTarget::At(point)).is_found() {
            return false;
        }
        self.move_focus(SeekTarget::At(point));
        self.requires_redraw = true;
        self.finalize_view();
        true
    }

    /// Cancels the touch in progress, so that what it would do when it ends,
    /// such as a button's action, is not done.
    ///
    /// The rest of the touch, until it ends, goes unanswered.
    pub fn cancel_touch(&mut self) {
        if let Some(mut touch) = self.touch.take() {
            touch.phase = Phase::Cancelled;
            self.send_touch(&touch);
            self.cancelled = Some(touch.id);
        }
    }

    /// Returns a reference to the application state.
    #[must_use]
    pub fn state(&self) -> &S {
        &self.state
    }

    /// Returns a mutable reference to the application state.
    ///
    /// The state is wrapped in [`Tracked`] which will request a view rebuild on your behalf when
    /// accessing the inner value mutably.
    #[must_use]
    pub fn state_mut(&mut self) -> Tracked<'_, S> {
        Tracked::new(&mut self.state, &mut self.requires_rebuild)
    }

    /// The current display size.
    #[must_use]
    pub fn size(&self) -> Size {
        self.size
    }

    /// The current elapsed virtual time.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.elapsed
    }

    /// The content shape of the currently focused element.
    #[must_use]
    pub fn focus_shape(&self) -> &ContentShape {
        &self.focus_shape
    }

    /// The focus group of the currently focused element.
    #[must_use]
    pub fn focus_group(&self) -> FocusGroup {
        self.focus_group
    }

    /// Returns mutable references to both source and target render trees.
    #[must_use]
    pub fn render_trees(&self) -> &Trees<V::Renderables> {
        &self.trees
    }

    /// Returns mutable references to both source and target render trees.
    #[must_use]
    pub fn render_trees_mut(&mut self) -> &mut Trees<V::Renderables> {
        &mut self.trees
    }

    /// Advances the virtual clock by the given duration.
    ///
    /// Prefer setting time directly with [`set_time`](Self::set_time).
    pub fn advance_time(&mut self, duration: Duration) {
        self.elapsed += duration;
    }

    /// Sets the elapsed time since app start. This value is used to drive animation.
    pub fn set_time(&mut self, time: Duration) {
        self.elapsed = time;
    }

    /// Rebuilds the view and render trees.
    ///
    /// This is typically called automatically when an event triggers a view rebuild, but
    /// it may be necessary to call manually for state changes not tracked by Buoyant.
    ///
    /// See also: [`finalize_view`](Self::finalize_view).
    pub fn force_rebuild(&mut self) {
        let domain = AnimationDomain::top_level(self.elapsed);
        self.trees.swap();
        let (source_tree, target_tree) = self.trees.both_mut();
        source_tree.join_from(target_tree, &domain);
        self.build_target();
        self.settle_focus();
        self.requires_rebuild = false;
        self.requires_redraw = true;
    }

    /// Builds the view from the state, and lays it out into the target tree.
    fn build_target(&mut self) {
        self.view = (self.view_fn)(&self.state);
        self.env = DefaultEnvironment::new(self.elapsed);
        let layout = self.view.layout(
            &self.size.into(),
            &self.env,
            &mut self.state,
            &mut self.view_state,
        );
        *self.trees.target_mut() = self.view.render_tree(
            &layout.sublayouts,
            Point::zero(),
            &self.env,
            &mut self.state,
            &mut self.view_state,
        );
    }

    /// Resolves focus in the built view, records it in the binding, and
    /// rebuilds once more if views depending on focus need to follow it.
    fn settle_focus(&mut self) {
        let built_with = self.bound_focus();
        let moved = self.resolve_focus();
        self.bind_focus();
        // Only views given the focused key through a binding depend on it.
        let rebound = self.focus_binding.is_some() && self.focused_key != built_with;
        if !moved && !rebound {
            return;
        }
        // Focus is found again by key, so it stays put while views that
        // depend on it take their new shape.
        let settled = self.focused_key;
        self.build_target();
        self.resolve_focus();
        self.bind_focus();
        debug_assert_eq!(
            self.focused_key, settled,
            "focus moved again while settling"
        );
        self.requires_redraw = true;
    }

    /// Finds focus again in a newly built view, then acts on any request.
    ///
    /// Returns whether focus was moved rather than found where it was.
    fn resolve_focus(&mut self) -> bool {
        let request = match self.focus_binding {
            Some(binding) => binding(&mut self.state).take_request(),
            None => self.focus_request.take(),
        };

        // An open modal takes focus from anything outside it.
        let captured = self.seek(SeekTarget::Modal).is_found();
        if captured {
            self.move_focus(SeekTarget::Modal);
        }

        // A request that can be met moves focus straight to its target, so
        // nothing on the way, such as a scroll view following focus, sees
        // focus land anywhere else first. A request for what is already
        // focused changes nothing, and one that cannot be met leaves focus
        // where it is.
        if let Some(key) = request
            && Some(key) != self.focused_key
            && self.seek(SeekTarget::Key(key)).is_found()
        {
            self.move_focus(SeekTarget::Key(key));
            return true;
        }

        // Reobtain focus in the group that holds it. When the path it was on
        // leads nowhere now, as when a focused list emptied, search again
        // from the start, so focus returns when content does.
        if !captured && !self.focus_forward_group(self.focus_group).is_handled() {
            self.focus_tree = DefaultFocus::default_first();
            _ = self.focus_forward_group(self.focus_group);
        }
        captured
    }

    /// Looks for `target` in the laid-out view.
    fn seek(&self, target: SeekTarget) -> SeekResult<V::FocusTree> {
        self.view.seek(
            &Seek::new(target, self.roles),
            self.trees.target(),
            &self.view_state,
            Some(&self.focus_tree),
        )
    }

    /// Moves focus to `target`, which was found: the focused element is
    /// torn down in its group, and the target acquires focus in its own.
    fn move_focus(&mut self, target: SeekTarget) {
        _ = self.send(Event::Focus {
            action: FocusAction::Teardown,
            group: self.focus_group,
        });
        // The path is taken after the teardown, so the parts of the old one
        // it keeps, such as where a popover returns focus, are released.
        let SeekResult::Found { tree, group } = self.seek(target) else {
            debug_assert!(false, "a teardown hid the focus target");
            return;
        };
        self.focus_tree = tree;
        self.focus_group = group;
        _ = self.focus_forward_group(group);
    }

    /// The key the application last saw focused.
    fn bound_focus(&mut self) -> Option<FocusKey> {
        self.focus_binding
            .and_then(|binding| binding(&mut self.state).current())
    }

    /// Records the focused key in the application's binding.
    fn bind_focus(&mut self) {
        if let Some(binding) = self.focus_binding {
            binding(&mut self.state).set_current(self.focused_key);
        }
    }

    /// Settles focus, then delivers `event` to the element a handler
    /// redirected it to, if focus is there now. Only one redirect is followed,
    /// so a key cannot loop back to the handler that sent it.
    fn redirect(&mut self, event: Event) -> Option<EventResult> {
        let target = match self.focus_binding {
            Some(binding) => binding(&mut self.state).pending(),
            None => self.focus_request,
        }?;
        self.finalize_view();
        if self.focused_key != Some(target) {
            return None;
        }
        self.redirecting = true;
        let result = self.send(event);
        self.redirecting = false;
        Some(result)
    }

    /// Whether focus has changed or been requested since it last settled.
    fn focus_unsettled(&mut self) -> bool {
        match self.focus_binding {
            Some(binding) => {
                let focus = binding(&mut self.state);
                focus.pending().is_some() || focus.current() != self.focused_key
            }
            None => self.focus_request.is_some(),
        }
    }

    /// Updates the display size.
    pub fn resize(&mut self, new_size: Size) {
        self.size = new_size;
        self.requires_rebuild = true;
    }

    /// Rebuilds the view and render trees if it was requested as a result of event
    /// handling or tracked state changes, and settles focus.
    ///
    /// When it returns, focus is settled: requests have been acted on, the
    /// binding holds the focused key, and views that depend on it have been
    /// rebuilt. If nothing is pending, this method does nothing.
    pub fn finalize_view(&mut self) {
        if self.requires_rebuild {
            self.force_rebuild();
        } else if self.focus_unsettled() {
            self.settle_focus();
        }
    }

    /// Renders the view to the given render target.
    ///
    /// If a rebuild is pending, it will be performed before rendering.
    /// Rebuilds can be eagerly triggered by calling [`rebuild()`](Self::rebuild()).
    pub fn render_animated<T, C>(&mut self, target: &mut T, color: &C)
    where
        V: View<C, S>,
        T: RenderTarget<ColorFormat = C>,
    {
        self.finalize_view();

        let domain = AnimationDomain::top_level(self.elapsed);
        Render::render_animated(
            target,
            self.trees.source(),
            self.trees.target(),
            color,
            &domain,
        );
        self.requires_redraw = false;
    }

    /// Whether the view needs to be redrawn due to changes in state or focus.
    pub fn should_redraw(&mut self) -> bool {
        self.requires_redraw || self.requires_rebuild
    }

    /// Draws the focus as a stroked shape on the given render target.
    pub fn draw_focus_overlay<T, C>(&self, target: &mut T, color: C, line_width: u32)
    where
        T: RenderTarget<ColorFormat = C>,
        C: Copy,
    {
        let stroke = Stroke::new(line_width);
        let brush = SolidBrush::new(color);
        match &self.focus_shape {
            ContentShape::Rectangle(rect) => {
                target.stroke(&stroke, LinearTransform::identity(), &brush, None, rect);
            }
            ContentShape::RoundedRectangle(rrect) => {
                target.stroke(&stroke, LinearTransform::identity(), &brush, None, rrect);
            }
            ContentShape::Circle(circle) => {
                target.stroke(&stroke, LinearTransform::identity(), &brush, None, circle);
            }
            ContentShape::Empty => (),
        }
    }

    /// Records what an event did to focus.
    ///
    /// Focus events say where focus now is, including that there is none
    /// when acquiring it failed or nothing claimed it through a teardown.
    /// Other events only report focus when they
    /// return a focused shape: a key handled without one leaves focus as it
    /// was.
    fn update_focus(&mut self, event: &Event, result: &EventResult) {
        let is_focus_event = matches!(event, Event::Focus { .. });
        match result {
            EventResult::Handled { shape, group, key } if *shape != ContentShape::Empty => {
                self.focus_shape = shape.clone();
                self.focused_key = *key;
                self.focus_group = *group;
            }
            EventResult::Handled { .. } if is_focus_event => {
                self.focus_shape = ContentShape::Empty;
                self.focused_key = None;
            }
            EventResult::Deferred
                if matches!(
                    event,
                    Event::Focus {
                        action: FocusAction::Focus(_) | FocusAction::Teardown,
                        ..
                    }
                ) =>
            {
                self.focus_shape = ContentShape::Empty;
                self.focused_key = None;
            }
            _ => {}
        }
    }
}

impl<V, S, F> Harness for App<V, S, F>
where
    V: ViewLayout<S>,
    V::FocusTree: DefaultFocus,
    V::Renderables: AnimatedJoin,
    F: Fn(&S) -> V,
{
    fn send(&mut self, event: impl Into<Event>) -> EventResult {
        let context = EventContext::new(self.elapsed).with_roles(self.roles);

        let event = event.into();
        let target_tree = self.trees.target_mut();

        let result = self.view.handle_event(
            &event,
            &context,
            target_tree,
            &mut self.state,
            &mut self.view_state,
            &mut self.focus_tree,
        );

        self.update_focus(&event, &result);

        if context.view_rebuild_requested.get() {
            self.requires_rebuild = true;
        }
        if context.redraw_requested.get() {
            self.requires_redraw = true;
        }
        if context.redirect_requested.get()
            && matches!(event, Event::KeyDown(_))
            && !self.redirecting
        {
            return self.redirect(event).unwrap_or(result);
        }
        result
    }

    fn focused_key(&self) -> Option<FocusKey> {
        self.focused_key
    }

    fn send_touch(&mut self, touch: &Touch) {
        // What is left of a cancelled touch goes unanswered.
        if self.cancelled == Some(touch.id) && touch.phase != Phase::Started {
            if matches!(touch.phase, Phase::Ended | Phase::Cancelled) {
                self.cancelled = None;
            }
            return;
        }
        let in_progress = self.touch.as_ref().map(|current| current.id);
        match touch.phase {
            Phase::Started if in_progress.is_none() => {
                self.cancelled = None;
                self.touch = Some(touch.clone());
            }
            Phase::Moved if in_progress == Some(touch.id) => self.touch = Some(touch.clone()),
            Phase::Ended | Phase::Cancelled if in_progress == Some(touch.id) => self.touch = None,
            _ => {}
        }
        let min_touch_target = match touch.tool {
            Tool::Pointer { .. } => Size::zero(),
            Tool::Finger | Tool::Stylus { .. } => self.min_touch_target,
        };
        let context = EventContext::new(self.elapsed)
            .with_roles(self.roles)
            .with_min_touch_target(min_touch_target);

        // A starting touch is first followed to where it lands, so that one
        // landing on no target but near one goes to that target.
        if touch.phase == Phase::Started && context.begin_touch_probe() {
            _ = self.view.handle_touch(
                touch,
                &context,
                self.trees.target_mut(),
                &mut self.state,
                &mut self.view_state,
            );
            context.end_touch_probe();
        }
        let result = self.view.handle_touch(
            touch,
            &context,
            self.trees.target_mut(),
            &mut self.state,
            &mut self.view_state,
        );

        match result {
            TouchResult::Focused(new_focus) => {
                // Tear down the previously focused element.
                _ = self.send(Event::Focus {
                    action: FocusAction::Teardown,
                    group: self.focus_group,
                });
                // Install the new focus tree and reassert focus forward.
                self.focus_tree = new_focus;
                _ = self.send(Event::Focus {
                    action: FocusAction::Focus(FocusDirection::Forward),
                    group: crate::focus::GROUP_0,
                });
            }
            TouchResult::Handled | TouchResult::Deferred => {}
        }

        if context.view_rebuild_requested.get() {
            self.requires_rebuild = true;
        }
        if context.redraw_requested.get() {
            self.requires_redraw = true;
        }
    }
}
