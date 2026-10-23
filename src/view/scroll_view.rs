//! Scroll view and related configuration

use core::time::Duration;

use embedded_touch::{Phase, Touch};

use crate::{
    environment::{Viewport, ViewportEnvironment},
    event::{Event, EventContext, EventResult, TouchResult},
    focus::{
        DefaultFocus, FocusAction, FocusDirection, FocusGroup, FocusKey, Role, Seek, SeekResult,
        SeekTarget,
    },
    layout::{LayoutDirection, ResolvedLayout},
    primitives::{
        Dimensions, Point, ProposedDimension, ProposedDimensions, Size, geometry::Rectangle,
    },
    render::{Capsule, ContentShape, Glide, Offset, ScrollRenderable},
    transition::Opacity,
    view::{ViewLayout, ViewMarker},
};

/// How long the content takes to come to rest after it scrolls, unless
/// [`ScrollView::with_glide_time`] says otherwise.
const GLIDE_TIME: Duration = Duration::from_millis(200);

/// Flung content slows by a factor of e over this time as it coasts.
const FLING_COAST: Duration = Duration::from_millis(325);

/// How a touch lifting from a drag was moving is measured over this much
/// time before it lifted, in milliseconds. A touch held still for longer
/// before lifting does not fling.
const FLING_WINDOW: u32 = 100;

/// A touch lifting slower than this, in pixels per second, does not fling.
const FLING_MIN_SPEED: f32 = 100.0;

/// The fastest a touch flings the content, in pixels per second.
const FLING_MAX_SPEED: f32 = 4000.0;

/// How many of the latest places a dragging touch was are kept.
const TRACK_LENGTH: usize = 4;

/// The axes along which the scroll view can scroll.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScrollDirection {
    /// Constrain scrolling to the vertical axis
    #[default]
    Vertical,
    /// Constrain scrolling to the horizontal axis
    Horizontal,
    /// Allow scrolling in both axes
    Both,
}

/// Configuration for the scroll bars appearance and behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ScrollBarConfig {
    /// When to display scroll bars.
    visibility: ScrollBarVisibility,
    /// Padding applied around all edges of scroll bars.
    padding: u32,
    /// Bar width.
    width: u32,
    /// Whether the scroll bars overlap the content of the scroll view.
    overlaps_content: bool,
    /// The minimum length of the scroll bars.
    minimum_bar_length: u32,
}

impl Default for ScrollBarConfig {
    fn default() -> Self {
        Self {
            visibility: ScrollBarVisibility::default(),
            padding: 5,
            width: 6,
            overlaps_content: false,
            minimum_bar_length: 12,
        }
    }
}

/// When to show the scrollbar
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ScrollBarVisibility {
    /// Always show the scrollbar
    #[default]
    Always,
    /// Never show the scrollbar
    Never,
    // /// Only show the scrollbar when scrolling
    // Auto,
}

/// A scroll view that allows scrolling through its inner content.
///
/// The scroll view can be configured to scroll in a specific direction (vertical, horizontal, or both),
/// and to display a scrollbar with various configurations such as visibility, padding, width, and minimum length.
///
/// # Examples
///
/// A vertically scrollable list of text content:
///
/// ```
/// use buoyant::view::prelude::*;
/// use embedded_graphics::{pixelcolor::Rgb565, mono_font::ascii::FONT_9X15_BOLD};
///
/// fn scrollable_content() -> impl View<Rgb565, ()> {
///     ScrollView::new(
///         VStack::new((
///             Text::new("Line 1", &FONT_9X15_BOLD),
///             Text::new("Line 2", &FONT_9X15_BOLD),
///             Text::new("Line 3", &FONT_9X15_BOLD),
///             Text::new("Line 4", &FONT_9X15_BOLD),
///             Text::new("Line 5", &FONT_9X15_BOLD),
///         ))
///     )
/// }
/// ```
///
/// Customizing the scrollbar appearance:
///
/// ```
/// use buoyant::view::{
///     prelude::*,
///     scroll_view::{ScrollBarVisibility, ScrollDirection}
/// };
/// use embedded_graphics::{pixelcolor::Rgb565, mono_font::ascii::FONT_9X15_BOLD};
///
/// fn custom_scrollbar() -> impl View<Rgb565, ()> {
///     ScrollView::new(
///         VStack::new((
///             Text::new("Content 1", &FONT_9X15_BOLD),
///             Text::new("Content 2", &FONT_9X15_BOLD),
///             Text::new("Content 3", &FONT_9X15_BOLD),
///         ))
///     )
///     .with_direction(ScrollDirection::Both)
///     .with_bar_visibility(ScrollBarVisibility::Never)
///     .with_bar_padding(4)
///     .with_bar_width(8)
///     .with_overlapping_bar(false)
///     .with_minimum_bar_length(30)
/// }
/// ```
#[derive(Debug, Clone)]
pub struct ScrollView<Inner> {
    inner: Inner,
    bar_config: ScrollBarConfig,
    direction: ScrollDirection,
    initial_pin_to_end: bool,
    focus_margin: u32,
    scroll_step: u32,
    self_focus: bool,
    short_content_at_end: bool,
    glide_time: Duration,
    end_generation: Option<u32>,
}

impl<Inner: ViewMarker> ScrollView<Inner> {
    /// Constructs a new [`ScrollView`] with the given inner view.
    #[must_use]
    pub fn new(inner: Inner) -> Self {
        Self {
            inner,
            bar_config: ScrollBarConfig::default(),
            direction: ScrollDirection::default(),
            initial_pin_to_end: false,
            focus_margin: 0,
            scroll_step: 0,
            self_focus: false,
            short_content_at_end: false,
            glide_time: GLIDE_TIME,
            end_generation: None,
        }
    }

    /// How long the content takes to come to rest after it scrolls, other
    /// than by dragging: about this long for a step of a few rows.
    ///
    /// The content moves as a critically damped spring. Scrolled again on
    /// the way, it carries on from where it is and how fast it is moving.
    /// Zero moves it at once.
    #[must_use]
    pub fn with_glide_time(mut self, time: Duration) -> Self {
        self.glide_time = time;
        self
    }

    /// Steps through a focused element longer than the view, this far at a
    /// time, before focus moves on from it.
    ///
    /// Moving focus scrolls by `step` toward the focused element's far edge
    /// while that edge is out of view, so all of it is seen; once it is in
    /// view, focus moves on. Zero, the default, moves focus on at once.
    #[must_use]
    pub fn with_scroll_step(mut self, step: u32) -> Self {
        self.scroll_step = step;
        self
    }

    /// Takes focus itself, as a [`Button`](crate::focus::Role::Button)
    /// does, when nothing in its content can: the content as a whole is
    /// then the focused element, which moving focus steps through as
    /// [`with_scroll_step`](Self::with_scroll_step) sets, before focus moves
    /// on from it. Nothing is drawn to show it.
    ///
    /// For content that is only read, such as a page of text or a list of
    /// readouts, where focusing each part would be a distraction.
    #[must_use]
    pub fn with_self_focus(mut self, self_focus: bool) -> Self {
        self.self_focus = self_focus;
        self
    }

    /// Keeps this much of the content beyond the focused element in view
    /// when scrolling to it, where there is more content.
    ///
    /// A margin smaller than the next element shows that element cut by the
    /// edge, which says the content goes on without a scroll bar.
    #[must_use]
    pub fn with_focus_margin(mut self, margin: u32) -> Self {
        self.focus_margin = margin;
        self
    }

    /// Places content shorter than the scroll view at its end along the
    /// scroll axis (the bottom for vertical scrolling) rather than its start.
    ///
    /// Useful for a history that grows toward the end, such as a chat.
    #[must_use]
    pub fn with_short_content_at_end(mut self, at_end: bool) -> Self {
        self.short_content_at_end = at_end;
        self
    }

    /// Sets the axes along which the scroll view can scroll.
    #[must_use]
    pub fn with_direction(mut self, direction: ScrollDirection) -> Self {
        self.direction = direction;
        self
    }

    /// Starts the scroll view pinned to the end of its content along the scroll axis.
    ///
    /// When enabled, a freshly built state renders its first frame scrolled to the
    /// trailing edge (bottom for vertical, right for horizontal) and follows content
    /// growth until the user scrolls away from the end.
    #[must_use]
    pub fn with_initial_pin_to_end(mut self, pin: bool) -> Self {
        self.initial_pin_to_end = pin;
        self
    }

    /// Scrolls to the end of the content each time `generation` changes, and
    /// pins it there as [`with_initial_pin_to_end`] does, until the content
    /// is scrolled away from the end again. The first value only records it.
    ///
    /// A count of the things that call for the end, such as messages sent,
    /// serves.
    ///
    /// [`with_initial_pin_to_end`]: Self::with_initial_pin_to_end
    #[must_use]
    pub fn with_end_generation(mut self, generation: u32) -> Self {
        self.end_generation = Some(generation);
        self
    }

    /// Sets whether the scrollbar overlaps the content.
    ///
    /// When `true`, the scrollbar is drawn over the content. When `false`,
    /// space is reserved for the scrollbar, reducing the available content area.
    #[must_use]
    pub fn with_overlapping_bar(mut self, overlaps: bool) -> Self {
        self.bar_config.overlaps_content = overlaps;
        self
    }

    /// Sets the minimum length of the scrollbar.
    #[must_use]
    pub fn with_minimum_bar_length(mut self, length: u32) -> Self {
        self.bar_config.minimum_bar_length = length;
        self
    }

    /// Sets the padding which is applied to all edges of the scrollbar.
    #[must_use]
    pub fn with_bar_padding(mut self, padding: u32) -> Self {
        self.bar_config.padding = padding;
        self
    }

    /// Sets the width (thickness) of the scrollbar.
    ///
    /// This applies to both horizontal and vertical scrollbars.
    #[must_use]
    pub fn with_bar_width(mut self, width: u32) -> Self {
        self.bar_config.width = width;
        self
    }

    /// Configures when the scrollbar should be visible.
    #[must_use]
    pub fn with_bar_visibility(mut self, visibility: ScrollBarVisibility) -> Self {
        self.bar_config.visibility = visibility;
        self
    }
}

/// A state machine to track the interaction state
#[derive(Debug, Clone, Default, PartialEq, Eq)]
enum ScrollInteraction {
    /// No interaction
    #[default]
    Idle,
    /// Tracks interaction from touch down to touch up
    Dragging {
        drag_start: Point,
        last_point: Point,
        target: InteractionTarget,
        touch_id: u8,
        track: Track,
    },
}

/// The latest places a dragging touch was, and when, to tell how fast it was
/// moving as it lifted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Track {
    /// Oldest first.
    points: [Point; TRACK_LENGTH],
    /// In milliseconds of app time, wrapping.
    times: [u32; TRACK_LENGTH],
    len: u8,
}

impl Track {
    fn new(point: Point, now: Duration) -> Self {
        let mut track = Self::default();
        track.push(point, now);
        track
    }

    fn push(&mut self, point: Point, now: Duration) {
        let mut len = usize::from(self.len);
        if len == TRACK_LENGTH {
            self.points.copy_within(1.., 0);
            self.times.copy_within(1.., 0);
            len -= 1;
        }
        self.points[len] = point;
        self.times[len] = Self::millis(now);
        self.len = (len + 1) as u8;
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "times wrap, and only differences between them are taken"
    )]
    const fn millis(now: Duration) -> u32 {
        now.as_millis() as u32
    }

    /// How fast the touch moved over the last [`FLING_WINDOW`], in pixels per
    /// second.
    #[expect(
        clippy::cast_precision_loss,
        reason = "pixel distances and milliseconds stay far below 2^24"
    )]
    fn velocity(&self) -> Option<[f32; 2]> {
        let last = usize::from(self.len).checked_sub(1)?;
        let (to, end) = (self.points[last], self.times[last]);
        let first = (0..last).find(|&i| end.wrapping_sub(self.times[i]) <= FLING_WINDOW)?;
        let elapsed = end.wrapping_sub(self.times[first]);
        if elapsed == 0 {
            return None;
        }
        let moved = to - self.points[first];
        let per_second = 1000.0 / elapsed as f32;
        Some([moved.x as f32 * per_second, moved.y as f32 * per_second])
    }
}

/// Tracks the intended target.
///
/// This allows inner elements like nested scroll views or sliders
/// to capture focus
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
enum InteractionTarget {
    /// The user is interacting with the scroll view itself
    Scroll,
    /// Some element inside the scroll view is being interacted with
    Inner,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContentPinning {
    Floating,
    Pinned(bool, bool),
}

/// Persisted state for the scroll view and its inner view.
#[derive(Debug, Clone)]
pub struct ScrollViewState<InnerState> {
    scroll_offset: Point,
    interaction: ScrollInteraction,
    inner_state: InnerState,
    content_pinning: ContentPinning,
    /// The view keeps the focused element in view. Scrolling by hand
    /// stops this until focus moves again.
    follows_focus: bool,
    /// The key of the element focus was last found on.
    focused_key: Option<FocusKey>,
    /// The shape of the focused element within the content, and its group.
    focused_shape: ContentShape,
    focused_group: FocusGroup,
    /// How the content moves to where it is headed, from its first layout.
    glide: Option<Glide>,
    /// The end generation last acted on.
    end_generation: Option<u32>,
}

/// Whether an offset leaves the content pinned to its end along each axis.
fn pinning_for(offset: Point, scroll_size: Size, inner_size: Size) -> ContentPinning {
    let should_pin_bottom =
        -offset.y >= (inner_size.height.saturating_sub(scroll_size.height)) as i32 && offset.y != 0;
    let should_pin_trailing =
        -offset.x >= (inner_size.width.saturating_sub(scroll_size.width)) as i32 && offset.x != 0;
    match (should_pin_trailing, should_pin_bottom) {
        (false, false) => ContentPinning::Floating,
        (horizontal, vertical) => ContentPinning::Pinned(horizontal, vertical),
    }
}

impl<Inner> ScrollView<Inner> {
    /// Pinned to the end of the content along the axes it scrolls.
    const fn end_pinning(&self) -> ContentPinning {
        let (horizontal, vertical) = match self.direction {
            ScrollDirection::Vertical => (false, true),
            ScrollDirection::Horizontal => (true, false),
            ScrollDirection::Both => (true, true),
        };
        ContentPinning::Pinned(horizontal, vertical)
    }

    /// The whole content, as a focused element's shape is kept: in the
    /// content's coordinates.
    fn whole_content<R>(render_tree: &ScrollRenderable<R>) -> ContentShape {
        ContentShape::Rectangle(Rectangle::new(Point::zero(), render_tree.inner_size))
    }

    /// Where the content's origin is placed.
    fn content_origin<R>(render_tree: &ScrollRenderable<R>) -> Point {
        render_tree.offset() + render_tree.inner.offset
    }

    /// How far the content can scroll along each axis.
    fn permitted(scroll_size: Size, inner_size: Size) -> Point {
        Point::new(
            inner_size.width.saturating_sub(scroll_size.width) as i32,
            inner_size.height.saturating_sub(scroll_size.height) as i32,
        )
    }

    /// Where content dragged to `offset` is: movement beyond the bounds is
    /// reduced by half.
    fn dragged(offset: Point, scroll_size: Size, inner_size: Size) -> Point {
        let permitted = Self::permitted(scroll_size, inner_size);
        let axis = |offset: i32, permitted: i32| {
            if offset > 0 {
                offset / 2
            } else if -offset > permitted {
                offset.midpoint(permitted) - permitted
            } else {
                offset
            }
        };
        Point::new(axis(offset.x, permitted.x), axis(offset.y, permitted.y))
    }

    /// The offset a drag leaves content at, where it is at `at`: the inverse
    /// of [`Self::dragged`].
    fn undragged(at: Point, scroll_size: Size, inner_size: Size) -> Point {
        let permitted = Self::permitted(scroll_size, inner_size);
        let axis = |at: i32, permitted: i32| {
            if at > 0 {
                at * 2
            } else if -at > permitted {
                at * 2 + permitted
            } else {
                at
            }
        };
        Point::new(axis(at.x, permitted.x), axis(at.y, permitted.y))
    }

    /// Lets the content go at `velocity`, from where the drag left it: it
    /// coasts on to rest, or to the bound in its way.
    fn fling<R, S>(
        &self,
        velocity: [f32; 2],
        now: Duration,
        render_tree: &mut ScrollRenderable<R>,
        state: &mut ScrollViewState<S>,
    ) {
        let (scroll_size, inner_size) = (render_tree.scroll_size, render_tree.inner_size);
        let shift = self.content_shift(scroll_size, inner_size);
        let permitted = Self::permitted(scroll_size, inner_size);
        let from = Self::dragged(state.scroll_offset, scroll_size, inner_size);
        let stop = Glide::coast_stop(from, velocity, FLING_COAST);
        let rest = Point::new(stop.x.clamp(-permitted.x, 0), stop.y.clamp(-permitted.y, 0));
        state.scroll_offset = rest;
        state.content_pinning = pinning_for(rest, scroll_size, inner_size);
        state.follows_focus = false;
        let glide = Glide::fling(
            from + shift,
            velocity,
            rest + shift,
            now,
            FLING_COAST,
            self.glide_time,
        );
        render_tree.glide = *state.glide.insert(glide);
    }

    /// How fast a touch lifting from a drag of the content flings it, along
    /// the axes it scrolls.
    fn fling_velocity(&self, track: &Track) -> Option<[f32; 2]> {
        let [x, y] = track.velocity()?;
        let [x, y] = match self.direction {
            ScrollDirection::Vertical => [0.0, y],
            ScrollDirection::Horizontal => [x, 0.0],
            ScrollDirection::Both => [x, y],
        };
        let speed_squared = x * x + y * y;
        if self.glide_time.is_zero() || speed_squared < FLING_MIN_SPEED * FLING_MIN_SPEED {
            return None;
        }
        let max = FLING_MAX_SPEED;
        Some([x.clamp(-max, max), y.clamp(-max, max)])
    }

    /// `touch`, where it is on the content, which lays itself out from zero.
    fn content_touch<R>(touch: &Touch, render_tree: &ScrollRenderable<R>) -> Touch {
        let mut touch = touch.clone();
        touch.location -= Self::content_origin(render_tree).into();
        touch
    }

    /// Where `frame`, a shape within the content, is on screen, with the
    /// focus margin around it.
    fn focus_frame<R>(&self, frame: &Rectangle, render_tree: &ScrollRenderable<R>) -> Rectangle {
        let margin = self.focus_margin as i32;
        Rectangle::new(
            frame.origin + render_tree.offset() + render_tree.inner.offset
                - Point::new(margin, margin),
            Size::new(
                frame.size.width + 2 * self.focus_margin,
                frame.size.height + 2 * self.focus_margin,
            ),
        )
    }

    /// Scrolls to `offset`, kept within the content. Returns whether the
    /// view moved.
    fn scroll_to<R, S>(
        &self,
        mut offset: Point,
        context: &EventContext,
        render_tree: &mut ScrollRenderable<R>,
        state: &mut ScrollViewState<S>,
    ) -> bool {
        // The margin never scrolls past the content's edges.
        let (scroll_size, inner_size) = (render_tree.scroll_size, render_tree.inner_size);
        let permitted = Point::new(
            inner_size.width.saturating_sub(scroll_size.width) as i32,
            inner_size.height.saturating_sub(scroll_size.height) as i32,
        );
        offset.x = offset.x.clamp(-permitted.x, 0);
        offset.y = offset.y.clamp(-permitted.y, 0);
        if offset == state.scroll_offset {
            return false;
        }
        context.request_redraw();
        state.scroll_offset = offset;
        state.content_pinning = pinning_for(offset, scroll_size, inner_size);
        let destination = offset + self.content_shift(scroll_size, inner_size);
        *render_tree.offset_mut() = destination;
        let glide = state.glide.get_or_insert(render_tree.glide);
        glide.head_for(destination, context.app_time, self.glide_time);
        render_tree.glide = *glide;
        let (horizontal_bar, vertical_bar) =
            self.scroll_bars(Point::zero(), scroll_size, inner_size, offset);
        render_tree.set_bars(horizontal_bar, vertical_bar);
        true
    }

    /// Scrolls a step toward the focused element's far edge in the
    /// direction of `action`, while that edge is out of view, in place of
    /// moving focus on. Returns `None` once the edge is in view.
    ///
    /// The step is taken before the focused element sees the event, so an
    /// element that keeps Next and Previous for itself while captive gets
    /// them only once its edge is in view.
    fn step_through<R, S>(
        &self,
        action: FocusAction,
        context: &EventContext,
        render_tree: &mut ScrollRenderable<R>,
        state: &mut ScrollViewState<S>,
    ) -> Option<EventResult> {
        let step = self.scroll_step as i32;
        if step == 0
            || state.interaction != ScrollInteraction::Idle
            || self.direction == ScrollDirection::Horizontal
        {
            return None;
        }
        let frame = self.focus_frame(&state.focused_shape.bounding_box()?, render_tree);
        let view = render_tree.bounds();
        let mut offset = state.scroll_offset;
        match action {
            FocusAction::Next => {
                let hidden = frame.y_end() - view.y_end();
                if hidden <= 0 {
                    return None;
                }
                offset.y -= hidden.min(step);
            }
            FocusAction::Previous => {
                let hidden = view.origin.y - frame.origin.y;
                if hidden <= 0 {
                    return None;
                }
                offset.y += hidden.min(step);
            }
            _ => return None,
        }
        self.scroll_to(offset, context, render_tree, state)
            .then(|| EventResult::Handled {
                shape: state
                    .focused_shape
                    .clone()
                    .with_offset(render_tree.offset() + render_tree.inner.offset),
                group: state.focused_group,
                key: state.focused_key,
            })
    }

    /// Where content shorter than the view is moved to, within the view.
    fn content_shift(&self, scroll_size: Size, inner_size: Size) -> Point {
        if !self.short_content_at_end {
            return Point::zero();
        }
        let (horizontal, vertical) = match self.direction {
            ScrollDirection::Vertical => (false, true),
            ScrollDirection::Horizontal => (true, false),
            ScrollDirection::Both => (true, true),
        };
        Point::new(
            if horizontal {
                scroll_size.width.saturating_sub(inner_size.width) as i32
            } else {
                0
            },
            if vertical {
                scroll_size.height.saturating_sub(inner_size.height) as i32
            } else {
                0
            },
        )
    }
}

impl<Inner> ScrollView<Inner> {
    /// Focus turned past the first or last element that takes it: the
    /// content beyond, which takes none, scrolls into view a step at a time
    /// (all at once without a step), with focus kept where it was. Returns
    /// `None` once there is no more of it, so the turn goes on to whatever
    /// bounds the focus.
    #[expect(clippy::too_many_arguments)]
    fn scroll_past_boundary<Captures>(
        &self,
        action: FocusAction,
        group: FocusGroup,
        context: &EventContext,
        render_tree: &mut ScrollRenderable<Inner::Renderables>,
        captures: &mut Captures,
        state: &mut ScrollViewState<Inner::State>,
        focus: &mut Inner::FocusTree,
    ) -> Option<EventResult>
    where
        Inner: ViewLayout<Captures>,
    {
        let forward = match action {
            FocusAction::Next => true,
            FocusAction::Previous => false,
            _ => return None,
        };
        if state.interaction != ScrollInteraction::Idle
            || self.direction == ScrollDirection::Horizontal
        {
            return None;
        }
        let permitted = render_tree
            .inner_size
            .height
            .saturating_sub(render_tree.scroll_size.height) as i32;
        let left = if forward {
            permitted + state.scroll_offset.y
        } else {
            -state.scroll_offset.y
        };
        if left <= 0 {
            return None;
        }
        // Back onto the element the turn went past, as bounded focus would.
        let EventResult::Handled { shape, key, group } = self.inner.handle_event(
            &Event::Focus {
                action: FocusAction::Focus(if forward {
                    FocusDirection::Backward
                } else {
                    FocusDirection::Forward
                }),
                group,
            },
            context,
            render_tree.inner_mut(),
            captures,
            &mut state.inner_state,
            focus,
        ) else {
            return None;
        };
        let step = match self.scroll_step as i32 {
            0 => left,
            step => step.min(left),
        };
        let mut offset = state.scroll_offset;
        offset.y += if forward { -step } else { step };
        self.scroll_to(offset, context, render_tree, state);
        // As if scrolled by hand: a rebuild must not scroll back to focus.
        state.follows_focus = false;
        state.focused_key = key;
        state.focused_shape = shape.clone();
        state.focused_group = group;
        Some(EventResult::Handled {
            shape: shape.with_offset(render_tree.offset() + render_tree.inner.offset),
            group,
            key,
        })
    }
}

impl<Inner: ViewMarker> ViewMarker for ScrollView<Inner> {
    type Renderables = ScrollRenderable<Inner::Renderables>;
    type Transition = Opacity;
}

impl<Inner: ViewLayout<Captures>, Captures> ViewLayout<Captures> for ScrollView<Inner> {
    type State = ScrollViewState<Inner::State>;
    type Sublayout = Dimensions;
    type FocusTree = Inner::FocusTree;

    fn transition(&self) -> Self::Transition {
        Opacity
    }

    fn build_state(&self, captures: &mut Captures) -> Self::State {
        let content_pinning = if self.initial_pin_to_end {
            self.end_pinning()
        } else {
            ContentPinning::Floating
        };
        Self::State {
            scroll_offset: Point::zero(),
            interaction: ScrollInteraction::Idle,
            inner_state: self.inner.build_state(captures),
            content_pinning,
            follows_focus: true,
            focused_key: None,
            focused_shape: ContentShape::Empty,
            focused_group: crate::focus::GROUP_0,
            glide: None,
            end_generation: self.end_generation,
        }
    }

    fn layout(
        &self,
        offer: &crate::primitives::ProposedDimensions,
        _env: &impl crate::environment::LayoutEnvironment,
        _captures: &mut Captures,
        _state: &mut Self::State,
    ) -> ResolvedLayout<Self::Sublayout> {
        let dimensions = offer.resolve_most_flexible(Size::zero(), Size::new(1, 1));
        ResolvedLayout {
            sublayouts: dimensions,
            resolved_size: dimensions,
        }
    }

    fn render_tree(
        &self,
        layout: &Self::Sublayout,
        origin: crate::primitives::Point,
        env: &impl crate::environment::LayoutEnvironment,
        captures: &mut Captures,
        state: &mut Self::State,
    ) -> Self::Renderables {
        let (horizontal_padding, vertical_padding) = if !self.bar_config.overlaps_content
            && self.bar_config.visibility == ScrollBarVisibility::Always
        {
            let bar_space = self.bar_config.padding * 2 + self.bar_config.width;
            match self.direction {
                ScrollDirection::Vertical => (bar_space, 0),
                ScrollDirection::Horizontal => (0, bar_space),
                ScrollDirection::Both => (bar_space, bar_space),
            }
        } else {
            (0, 0)
        };

        let (inner_width, inner_height) = match self.direction {
            ScrollDirection::Vertical => (
                ProposedDimension::Exact(Into::<u32>::into(layout.width) - horizontal_padding),
                ProposedDimension::Compact,
            ),
            ScrollDirection::Horizontal => (
                ProposedDimension::Compact,
                ProposedDimension::Exact(Into::<u32>::into(layout.height) - vertical_padding),
            ),
            ScrollDirection::Both => (ProposedDimension::Compact, ProposedDimension::Compact),
        };

        let inner_offer = ProposedDimensions::new(inner_width, inner_height);
        // The content sees no viewport of an enclosing scroll view: its own
        // is published while it is built, below.
        let inner_layout = self.inner.layout(
            &inner_offer,
            &ViewportEnvironment::new(env, None),
            captures,
            &mut state.inner_state,
        );

        let scroll_view_width: u32 = layout.width.into();
        let scroll_view_height: u32 = layout.height.into();
        let inner_view_width: u32 = inner_layout.resolved_size.width.0;
        let inner_view_height: u32 = inner_layout.resolved_size.height.0;
        let scroll_size = Size::new(scroll_view_width, scroll_view_height);
        let inner_size = Size::new(inner_view_width, inner_view_height);

        let permitted_offset_x = inner_view_width.saturating_sub(scroll_view_width) as i32;
        let permitted_offset_y = inner_view_height.saturating_sub(scroll_view_height) as i32;

        // Asked for the end since the last layout: pinned there, as if the
        // content had been scrolled there by hand.
        if state.interaction == ScrollInteraction::Idle
            && self.end_generation != state.end_generation
        {
            state.end_generation = self.end_generation;
            state.content_pinning = self.end_pinning();
            state.follows_focus = false;
        }

        // Adjust scroll offset if pinning is enabled
        if state.interaction == ScrollInteraction::Idle
            && let ContentPinning::Pinned(horizontal, vertical) = state.content_pinning
        {
            if horizontal {
                state.scroll_offset.x = -permitted_offset_x;
            }
            if vertical {
                state.scroll_offset.y = -permitted_offset_y;
            }
        }

        // Make sure the inner view is within the scroll view's bounds, mutate scroll
        // offset to correct only if not dragging
        let is_dragging;
        let subview_offset = match state.interaction {
            ScrollInteraction::Dragging { .. } => {
                // The offset should be set correctly based on the scroll direction
                // in the event handler, don't bother checking it here
                is_dragging = true;
                Self::dragged(state.scroll_offset, scroll_size, inner_size)
            }
            ScrollInteraction::Idle => {
                is_dragging = false;

                state.scroll_offset.x = state.scroll_offset.x.clamp(-permitted_offset_x, 0);
                state.scroll_offset.y = state.scroll_offset.y.clamp(-permitted_offset_y, 0);

                state.scroll_offset
            }
        };

        let (horizontal_bar, vertical_bar) =
            self.scroll_bars(Point::zero(), scroll_size, inner_size, subview_offset);

        let inner_origin = subview_offset + self.content_shift(scroll_size, inner_size);
        // Dragged, the content follows the touch; otherwise it glides to
        // where it is headed, from wherever it was on its way somewhere else.
        // Settled before the content is built, so that the content, which can
        // be large, is not held on the stack across the call.
        let glide = match &mut state.glide {
            Some(glide) if !is_dragging => {
                glide.head_for(inner_origin, env.app_time(), self.glide_time);
                *glide
            }
            glide => *glide.insert(Glide::at_rest(inner_origin)),
        };
        // What is shown of the content, where it is drawn now, which lazy
        // content builds around.
        let drawn = glide.at(env.app_time()).0;
        let viewport = Viewport::new(Rectangle::new(-drawn, scroll_size));
        let inner_render_tree = self.inner.render_tree(
            &inner_layout.sublayouts,
            Point::zero(),
            &ViewportEnvironment::new(env, Some(&viewport)),
            captures,
            &mut state.inner_state,
        );
        let offset = Offset::new(inner_origin, inner_render_tree);

        ScrollRenderable::new(
            scroll_size,
            inner_size,
            Offset::new(origin, (offset, horizontal_bar, vertical_bar)),
            glide,
        )
        .with_coverage(viewport.realized(), viewport.safe())
    }

    #[expect(clippy::too_many_lines)]
    fn handle_event(
        &self,
        event: &crate::event::Event,
        context: &EventContext,
        render_tree: &mut Self::Renderables,
        captures: &mut Captures,
        state: &mut Self::State,
        focus: &mut Self::FocusTree,
    ) -> EventResult {
        // Handle focus events separately
        if let Event::Focus {
            action: focus_event,
            group,
        } = event
        {
            // A move up or down takes the path the wheel does through the
            // content, stepping through and past it.
            let sequence = match focus_event {
                FocusAction::Toward { dir, .. } if dir.axis() == LayoutDirection::Vertical => {
                    match dir.focus_direction() {
                        FocusDirection::Forward => FocusAction::Next,
                        FocusDirection::Backward => FocusAction::Previous,
                    }
                }
                other => *other,
            };
            if let Some(result) = self.step_through(sequence, context, render_tree, state) {
                return result;
            }

            // Route focus event to inner view, where the content lays itself
            // out.
            let mut result = self.inner.handle_event(
                &Event::Focus {
                    action: focus_event.offset(-Self::content_origin(render_tree)),
                    group: *group,
                },
                context,
                render_tree.inner_mut(),
                captures,
                &mut state.inner_state,
                focus,
            );

            if matches!(result, EventResult::Deferred)
                && let Some(past) = self.scroll_past_boundary(
                    sequence,
                    *group,
                    context,
                    render_tree,
                    captures,
                    state,
                    focus,
                )
            {
                return past;
            }

            // Nothing in the content took focus: the content as a whole
            // does, where the scroll view takes focus itself.
            if matches!(result, EventResult::Deferred)
                && focus_event.is_acquire()
                && self.self_focus
                && context.roles.contains(Role::Button)
            {
                result = EventResult::Handled {
                    shape: Self::whole_content(render_tree),
                    group: *group,
                    key: None,
                };
            }

            if let EventResult::Handled {
                shape,
                key,
                group: found_group,
            } = &mut result
                && let Some(frame) = shape.bounding_box()
            {
                // Moving focus keeps the focused element in view, whether by
                // navigating or by focus landing on a different element.
                // Acquiring focus in place, as a rebuild does, follows it too,
                // unless the content was scrolled away from it by hand since.
                let navigating = focus_event.is_move() || *key != state.focused_key;
                state.focused_key = *key;
                state.focused_shape = shape.clone();
                state.focused_group = *found_group;
                if navigating {
                    state.follows_focus = true;
                }
                let should_auto_scroll = state.interaction == ScrollInteraction::Idle
                    && (navigating || (focus_event.is_acquire() && state.follows_focus));

                if should_auto_scroll {
                    let scroll_view_frame = render_tree.bounds();
                    let target_frame = self.focus_frame(&frame, render_tree);

                    let mut new_offset = state.scroll_offset;

                    // Adjust vertical offset. An element taller than the view
                    // shows the edge nearest where it was out of view.
                    let above = target_frame.origin.y < scroll_view_frame.origin.y;
                    let below = target_frame.y_end() > scroll_view_frame.y_end();
                    if target_frame.size.height > scroll_view_frame.size.height {
                        if above && !below {
                            new_offset.y += scroll_view_frame.y_end() - target_frame.y_end();
                        } else if below && !above {
                            new_offset.y -= target_frame.origin.y - scroll_view_frame.origin.y;
                        }
                    } else if above {
                        new_offset.y += scroll_view_frame.origin.y - target_frame.origin.y;
                    } else if below {
                        new_offset.y -= target_frame.y_end() - scroll_view_frame.y_end();
                    }

                    // Adjust horizontal offset
                    if target_frame.origin.x < scroll_view_frame.origin.x {
                        // Target is left of visible area
                        new_offset.x += scroll_view_frame.origin.x - target_frame.origin.x;
                    } else if target_frame.x_end() > scroll_view_frame.x_end() {
                        // Target is right of visible area
                        new_offset.x -= target_frame.x_end() - scroll_view_frame.x_end();
                    }

                    self.scroll_to(new_offset, context, render_tree, state);
                }

                // Always adjust shape with offset so parent views see correct position
                *shape = shape.clone().with_offset(Self::content_origin(render_tree));
            }

            return result;
        }

        let (result, delta) = match event {
            Event::Scroll(delta) => {
                context.request_view_rebuild();
                (EventResult::Deferred, *delta)
            }
            _ => (
                self.inner.handle_event(
                    event,
                    context,
                    render_tree.inner_mut(),
                    captures,
                    &mut state.inner_state,
                    focus,
                ),
                Point::zero(),
            ),
        };

        // Constrain delta to configured axis
        let delta = match self.direction {
            ScrollDirection::Vertical => Point::new(0, delta.y),
            ScrollDirection::Horizontal => Point::new(delta.x, 0),
            ScrollDirection::Both => delta,
        };
        state.scroll_offset.x = state.scroll_offset.x.saturating_add(delta.x);
        state.scroll_offset.y = state.scroll_offset.y.saturating_add(delta.y);

        if delta != Point::zero() {
            // Scrolled by hand: stop following focus until it moves.
            state.follows_focus = false;
        }
        state.content_pinning = pinning_for(
            state.scroll_offset,
            render_tree.scroll_size,
            render_tree.inner_size,
        );

        // Recompute scroll bars and manually update the target tree.
        // This is used to avoid recomputing the entire view tree every time the
        // scroll position changes. If there's scroll jank it's probably related
        // to this optimization.
        if delta != Point::zero() && !context.view_rebuild_requested.get() {
            let subview_offset = Self::dragged(
                state.scroll_offset,
                render_tree.scroll_size,
                render_tree.inner_size,
            );
            let destination = subview_offset
                + self.content_shift(render_tree.scroll_size, render_tree.inner_size);
            *render_tree.offset_mut() = destination;
            // Dragged, the content follows the touch.
            render_tree.glide = *state.glide.insert(Glide::at_rest(destination));
            let (horizontal_bar, vertical_bar) = self.scroll_bars(
                Point::zero(),
                render_tree.scroll_size,
                render_tree.inner_size,
                subview_offset,
            );
            render_tree.set_bars(horizontal_bar, vertical_bar);
        }

        result
    }

    fn seek(
        &self,
        seek: &Seek,
        render_tree: &Self::Renderables,
        state: &Self::State,
        focus: Option<&Self::FocusTree>,
    ) -> SeekResult<Self::FocusTree> {
        let seek = match seek.target {
            // Content scrolled out of view is not under the point.
            SeekTarget::At(point) if !render_tree.bounds().contains(&point) => {
                return SeekResult::Absent;
            }
            SeekTarget::At(point) => {
                seek.with_target(SeekTarget::At(point - Self::content_origin(render_tree)))
            }
            _ => *seek,
        };
        match self
            .inner
            .seek(&seek, render_tree.inner(), &state.inner_state, focus)
        {
            SeekResult::Absent
                if self.self_focus
                    && seek.roles.contains(Role::Button)
                    && matches!(seek.target, SeekTarget::Any | SeekTarget::At(_)) =>
            {
                SeekResult::found(DefaultFocus::default_first())
            }
            result => result,
        }
    }

    #[expect(clippy::too_many_lines)]
    fn handle_touch(
        &self,
        touch: &Touch,
        context: &EventContext,
        render_tree: &mut Self::Renderables,
        captures: &mut Captures,
        state: &mut Self::State,
    ) -> TouchResult<Self::FocusTree> {
        // Only track the first touch. This could cause problems if
        // the touch is "lost" without an ended or cancelled event.
        if let ScrollInteraction::Dragging { touch_id, .. } = state.interaction
            && touch.id != touch_id
        {
            return self.inner.handle_touch(
                &Self::content_touch(touch, render_tree),
                context,
                render_tree.inner_mut(),
                captures,
                &mut state.inner_state,
            );
        }
        let point = touch.location.into();
        let mut fling = None;
        let (result, delta) = match touch.phase {
            Phase::Started => {
                let bounds = render_tree.bounds();
                if bounds.contains(&point) {
                    // Touched anywhere in it, the scroll view outranks small
                    // targets outside that the touch is only near.
                    context.touch_starts_in();
                    if context.is_touch_probe() {
                        // Flung content takes the touch for the scroll view.
                        if !render_tree.glide.is_coasting(context.app_time) {
                            _ = self.inner.handle_touch(
                                &Self::content_touch(touch, render_tree),
                                context,
                                render_tree.inner_mut(),
                                captures,
                                &mut state.inner_state,
                            );
                        }
                        return TouchResult::Handled;
                    }
                }
                if bounds.contains(&point) && render_tree.glide.is_coasting(context.app_time) {
                    // A touch on flung content stops it where it is, and
                    // drags it from there: it is for the scroll view alone.
                    let (scroll_size, inner_size) =
                        (render_tree.scroll_size, render_tree.inner_size);
                    let at = render_tree.glide.at(context.app_time).0;
                    state.scroll_offset = Self::undragged(
                        at - self.content_shift(scroll_size, inner_size),
                        scroll_size,
                        inner_size,
                    );
                    *render_tree.offset_mut() = at;
                    render_tree.glide = *state.glide.insert(Glide::at_rest(at));
                    state.interaction = ScrollInteraction::Dragging {
                        drag_start: point,
                        last_point: point,
                        target: InteractionTarget::Scroll,
                        touch_id: touch.id,
                        track: Track::new(point, context.app_time),
                    };
                    context.request_redraw();
                    return TouchResult::Handled;
                }
                if bounds.contains(&point) {
                    state.interaction = ScrollInteraction::Dragging {
                        drag_start: point,
                        last_point: point,
                        target: InteractionTarget::Unknown,
                        touch_id: touch.id,
                        track: Track::new(point, context.app_time),
                    };

                    context.request_redraw();
                    // returning the inner result here would move focus before we're committed to scrolling
                    {
                        let _inner_result = self.inner.handle_touch(
                            &Self::content_touch(touch, render_tree),
                            context,
                            render_tree.inner_mut(),
                            captures,
                            &mut state.inner_state,
                        );
                    }
                    (TouchResult::Handled, Point::zero())
                } else {
                    // Touches cannot start outside the bounds, return early
                    return TouchResult::Deferred;
                }
            }
            Phase::Moved => match &mut state.interaction {
                ScrollInteraction::Dragging {
                    drag_start,
                    last_point,
                    target,
                    track,
                    ..
                } => {
                    let delta = point - *last_point;

                    *last_point = point;
                    track.push(point, context.app_time);
                    let total_scroll = point - *drag_start;

                    context.request_redraw();
                    // 4 pixels of wiggle without cancelling inner
                    match target {
                        InteractionTarget::Scroll => (TouchResult::Handled, delta),
                        InteractionTarget::Inner => {
                            let inner_result = self.inner.handle_touch(
                                &Self::content_touch(touch, render_tree),
                                context,
                                render_tree.inner_mut(),
                                captures,
                                &mut state.inner_state,
                            );
                            (inner_result, Point::zero())
                        }
                        InteractionTarget::Unknown => {
                            let horizontal_intent = total_scroll.x.abs() >= 4
                                && self.direction != ScrollDirection::Vertical;
                            let vertical_intent = total_scroll.y.abs() >= 4
                                && self.direction != ScrollDirection::Horizontal;

                            if horizontal_intent || vertical_intent {
                                // cancel inner interaction once we're sure the user intended to scroll
                                *target = InteractionTarget::Scroll;
                                let mut cancel_touch = Self::content_touch(touch, render_tree);
                                cancel_touch.phase = Phase::Cancelled;
                                // returning the inner result here would move focus before we're committed to scrolling
                                {
                                    let _inner_result = self.inner.handle_touch(
                                        &cancel_touch,
                                        context,
                                        render_tree.inner_mut(),
                                        captures,
                                        &mut state.inner_state,
                                    );
                                }
                                (TouchResult::Handled, delta)
                            } else {
                                let horizontal_intent = total_scroll.x.abs() >= 8
                                    && self.direction == ScrollDirection::Vertical;
                                let vertical_intent = total_scroll.y.abs() >= 8
                                    && self.direction == ScrollDirection::Horizontal;

                                // notify inner of result, don't commit
                                let inner_result = self.inner.handle_touch(
                                    &Self::content_touch(touch, render_tree),
                                    context,
                                    render_tree.inner_mut(),
                                    captures,
                                    &mut state.inner_state,
                                );
                                let result = if (horizontal_intent || vertical_intent)
                                    && matches!(
                                        &inner_result,
                                        TouchResult::Focused(_) | TouchResult::Handled
                                    ) {
                                    *target = InteractionTarget::Inner;
                                    inner_result
                                } else {
                                    TouchResult::Handled
                                };
                                (result, delta)
                            }
                        }
                    }
                }
                ScrollInteraction::Idle => (TouchResult::Handled, Point::zero()),
            },
            Phase::Ended => match state.interaction {
                ScrollInteraction::Dragging {
                    drag_start: _,
                    last_point,
                    target,
                    mut track,
                    ..
                } => {
                    state.interaction = ScrollInteraction::Idle;

                    let delta = point - last_point;

                    context.request_view_rebuild();
                    if target == InteractionTarget::Scroll {
                        // Lifted moving, the content coasts on. Otherwise the
                        // rebuild glides it back within its bounds from where
                        // the touch left it.
                        track.push(point, context.app_time);
                        fling = self.fling_velocity(&track);
                        (TouchResult::Handled, delta)
                    } else {
                        let inner_result = self.inner.handle_touch(
                            &Self::content_touch(touch, render_tree),
                            context,
                            render_tree.inner_mut(),
                            captures,
                            &mut state.inner_state,
                        );
                        (inner_result, delta)
                    }
                }
                ScrollInteraction::Idle => (
                    self.inner.handle_touch(
                        &Self::content_touch(touch, render_tree),
                        context,
                        render_tree.inner_mut(),
                        captures,
                        &mut state.inner_state,
                    ),
                    Point::zero(),
                ),
            },
            Phase::Cancelled => {
                state.interaction = ScrollInteraction::Idle;
                context.request_view_rebuild();
                let inner_result = self.inner.handle_touch(
                    &Self::content_touch(touch, render_tree),
                    context,
                    render_tree.inner_mut(),
                    captures,
                    &mut state.inner_state,
                );
                (inner_result, Point::zero())
            }
            Phase::Hovering(_) => (
                self.inner.handle_touch(
                    &Self::content_touch(touch, render_tree),
                    context,
                    render_tree.inner_mut(),
                    captures,
                    &mut state.inner_state,
                ),
                Point::zero(),
            ),
        };

        // Constrain delta to configured axis
        let delta = match self.direction {
            ScrollDirection::Vertical => Point::new(0, delta.y),
            ScrollDirection::Horizontal => Point::new(delta.x, 0),
            ScrollDirection::Both => delta,
        };
        state.scroll_offset.x = state.scroll_offset.x.saturating_add(delta.x);
        state.scroll_offset.y = state.scroll_offset.y.saturating_add(delta.y);

        if delta != Point::zero() {
            // Scrolled by hand: stop following focus until it moves.
            state.follows_focus = false;
        }
        state.content_pinning = pinning_for(
            state.scroll_offset,
            render_tree.scroll_size,
            render_tree.inner_size,
        );

        if let Some(velocity) = fling {
            self.fling(velocity, context.app_time, render_tree, state);
        }

        if delta != Point::zero() && !context.view_rebuild_requested.get() {
            let subview_offset = Self::dragged(
                state.scroll_offset,
                render_tree.scroll_size,
                render_tree.inner_size,
            );
            let destination = subview_offset
                + self.content_shift(render_tree.scroll_size, render_tree.inner_size);
            *render_tree.offset_mut() = destination;
            // Dragged, the content follows the touch.
            render_tree.glide = *state.glide.insert(Glide::at_rest(destination));
            let (horizontal_bar, vertical_bar) = self.scroll_bars(
                Point::zero(),
                render_tree.scroll_size,
                render_tree.inner_size,
                subview_offset,
            );
            render_tree.set_bars(horizontal_bar, vertical_bar);
        }

        result
    }
}

// TODO: remove generics to prevent excessive monomorphization
impl<V> ScrollView<V> {
    #[must_use]
    fn scroll_bars(
        &self,
        origin: Point,
        scroll_size: Size,
        inner_size: Size,
        subview_offset: Point,
    ) -> (Option<Capsule>, Option<Capsule>) {
        let overlap_padding = match self.direction {
            ScrollDirection::Vertical | ScrollDirection::Horizontal => 0,
            ScrollDirection::Both => self.bar_config.width + self.bar_config.padding,
        };

        let should_show_scrollbar = match self.bar_config.visibility {
            ScrollBarVisibility::Always => true,
            ScrollBarVisibility::Never => false,
        };

        // Create scrollbars based on direction
        let vertical_bar = if should_show_scrollbar
            && matches!(
                self.direction,
                ScrollDirection::Vertical | ScrollDirection::Both
            ) {
            let (bar_y, bar_height) = bar_size(
                scroll_size.height,
                inner_size.height,
                subview_offset.y,
                self.bar_config.minimum_bar_length,
                self.bar_config.padding,
                self.bar_config.padding + overlap_padding,
            );
            let bar_x = scroll_size
                .width
                .saturating_sub(self.bar_config.padding)
                .saturating_sub(self.bar_config.width);

            Some(Capsule::new(
                origin + Point::new(bar_x as i32, bar_y),
                Size::new(self.bar_config.width, bar_height),
            ))
        } else {
            None
        };

        let horizontal_bar = if should_show_scrollbar
            && matches!(
                self.direction,
                ScrollDirection::Horizontal | ScrollDirection::Both
            ) {
            let (bar_x, bar_width) = bar_size(
                scroll_size.width,
                inner_size.width,
                subview_offset.x,
                self.bar_config.minimum_bar_length,
                self.bar_config.padding,
                self.bar_config.padding + overlap_padding,
            );
            let bar_y = scroll_size
                .height
                .saturating_sub(self.bar_config.padding)
                .saturating_sub(self.bar_config.width);

            Some(Capsule::new(
                origin + Point::new(bar_x, bar_y as i32),
                Size::new(bar_width, self.bar_config.width),
            ))
        } else {
            None
        };
        (vertical_bar, horizontal_bar)
    }
}

/// Calculates the position and size of the scrollbar based on the scroll view's size and
/// attributes.
///
/// Returns a tuple containing the position of the scrollbar and its length.
fn bar_size(
    scroll_view_length: u32,
    inner_view_length: u32,
    scroll_offset: i32,
    min_length: u32,
    leading_padding: u32,
    trailing_padding: u32,
) -> (i32, u32) {
    let overscroll_amount = if scroll_offset > 0 {
        scroll_offset as u32
    } else {
        let max_offset = inner_view_length.saturating_sub(scroll_view_length);
        ((-scroll_offset) as u32).saturating_sub(max_offset)
    };

    let available_space = scroll_view_length.saturating_sub(leading_padding + trailing_padding);

    let perceived_scroll_length = scroll_view_length.saturating_sub(overscroll_amount);
    let bar_length = ((available_space * perceived_scroll_length)
        / inner_view_length.max(scroll_view_length))
    .max(min_length);

    let bar_position = if inner_view_length <= scroll_view_length {
        // Inner view is smaller, bar always touches the top or bottom
        if scroll_offset < 0 {
            // Bottom
            (leading_padding + available_space.saturating_sub(bar_length)) as i32
        } else {
            // Top
            leading_padding as i32
        }
    } else {
        // Actual scrollable content - position based on scroll progress
        let max_travel = available_space.saturating_sub(bar_length) as i32;
        let permitted_offset = (inner_view_length - scroll_view_length) as i32;

        let scroll_progress = (-scroll_offset).max(0).min(permitted_offset);
        leading_padding as i32 + (scroll_progress * max_travel) / permitted_offset
    };

    (bar_position, bar_length)
}

#[cfg(test)]
mod tests {
    use super::bar_size;

    #[test]
    fn smaller_inner() {
        // inner view is smaller, at rest
        let scroll_length = 100;
        let inner_length = 50;
        let scroll_offset = 0;
        let min_length = 10;
        let leading_padding = 5;
        let trailing_padding = 5;

        let (bar_y, bar_length) = bar_size(
            scroll_length,
            inner_length,
            scroll_offset,
            min_length,
            leading_padding,
            trailing_padding,
        );

        assert_eq!(bar_y, 5);
        assert_eq!(bar_length, 90);
    }

    #[test]
    fn equal_inner() {
        // inner view is same size, at rest
        let scroll_length = 100;
        let inner_length = 100;
        let scroll_offset = 0;
        let min_length = 10;
        let leading_padding = 5;
        let trailing_padding = 5;

        let (bar_y, bar_length) = bar_size(
            scroll_length,
            inner_length,
            scroll_offset,
            min_length,
            leading_padding,
            trailing_padding,
        );

        assert_eq!(bar_y, 5);
        assert_eq!(bar_length, 90);
    }

    #[test]
    fn double_inner_top() {
        // inner view is 2x scroll view, resting at top
        let scroll_length = 100;
        let inner_length = 200;
        let scroll_offset = 0;
        let min_length = 10;
        let leading_padding = 5;
        let trailing_padding = 5;

        let (bar_y, bar_length) = bar_size(
            scroll_length,
            inner_length,
            scroll_offset,
            min_length,
            leading_padding,
            trailing_padding,
        );

        assert_eq!(bar_y, 5);
        assert_eq!(bar_length, 45); // (100 - 2 * 5) * 100 / 200
    }

    #[test]
    fn double_inner_bottom() {
        // inner view is 2x scroll view, resting at bottom (-y scrolls down)
        let scroll_length = 100;
        let inner_length = 200;
        let scroll_offset = -100;
        let min_length = 10;
        let leading_padding = 5;
        let trailing_padding = 5;

        let (bar_y, bar_length) = bar_size(
            scroll_length,
            inner_length,
            scroll_offset,
            min_length,
            leading_padding,
            trailing_padding,
        );

        assert_eq!(bar_y, 50);
        assert_eq!(bar_length, 45); // (100 - 2 * 5) * 100 / 200
    }

    #[test]
    fn double_inner_slight_overscroll_top() {
        // inner view is 2x scroll view, overscrolled at top
        let scroll_length = 100;
        let inner_length = 200;
        let scroll_offset = 20; // slight overscroll up
        let min_length = 10;
        let leading_padding = 5;
        let trailing_padding = 5;

        let (bar_y, bar_length) = bar_size(
            scroll_length,
            inner_length,
            scroll_offset,
            min_length,
            leading_padding,
            trailing_padding,
        );

        assert_eq!(bar_y, 5);
        assert_eq!(bar_length, 36); // (100 - 2 * 5) * (100 - 20) / 200
    }

    #[test]
    fn double_inner_slight_overscroll_bottom() {
        // inner view is 2x scroll view, overscrolled at bottom
        let scroll_length = 100;
        let inner_length = 200;
        let scroll_offset = -120; // slight overscroll down
        let min_length = 10;
        let leading_padding = 5;
        let trailing_padding = 5;

        let (bar_y, bar_length) = bar_size(
            scroll_length,
            inner_length,
            scroll_offset,
            min_length,
            leading_padding,
            trailing_padding,
        );

        assert_eq!(bar_y, 59);
        assert_eq!(bar_length, 36); // (100 - 2 * 5) * (100 - 20) / 200
    }

    #[test]
    fn min_length_bar_top_rest() {
        // inner view is 1000x scroll view, resting at top
        let scroll_length = 100;
        let inner_length = 100_000;
        let scroll_offset = 0;
        let min_length = 7;
        let leading_padding = 10;
        let trailing_padding = 10;

        let (bar_y, bar_length) = bar_size(
            scroll_length,
            inner_length,
            scroll_offset,
            min_length,
            leading_padding,
            trailing_padding,
        );

        assert_eq!(bar_y, 10);
        assert_eq!(bar_length, 7);
    }

    #[test]
    fn min_length_bar_bottom_rest() {
        // inner view is 1000x scroll view, resting at bottom
        let scroll_length = 100;
        let inner_length = 100_000;
        let scroll_offset = -99900;
        let min_length = 9;
        let leading_padding = 2;
        let trailing_padding = 5;

        let (bar_y, bar_length) = bar_size(
            scroll_length,
            inner_length,
            scroll_offset,
            min_length,
            leading_padding,
            trailing_padding,
        );

        assert_eq!(bar_y, 86); // 100 - 5 - 9
        assert_eq!(bar_length, 9);
    }

    #[test]
    fn half_length_bar_top_overscrolled() {
        // inner view overscrolled up to cause half bar length
        let scroll_length = 100;
        let inner_length = 10;
        let scroll_offset = 50; // overscrolled up halfway
        let min_length = 7;
        let leading_padding = 10;
        let trailing_padding = 10;

        let (bar_y, bar_length) = bar_size(
            scroll_length,
            inner_length,
            scroll_offset,
            min_length,
            leading_padding,
            trailing_padding,
        );

        assert_eq!(bar_y, 10);
        assert_eq!(bar_length, 40); // (100 - (10 + 10)) * 50 / 100
    }

    #[test]
    fn half_length_bar_bottom_overscrolled() {
        // inner view overscrolled down to cause half bar length
        let scroll_length = 100;
        let inner_length = 10;
        let scroll_offset = -50; // overscrolled down halfway
        let min_length = 9;
        let leading_padding = 2;
        let trailing_padding = 2;

        let (bar_y, bar_length) = bar_size(
            scroll_length,
            inner_length,
            scroll_offset,
            min_length,
            leading_padding,
            trailing_padding,
        );

        assert_eq!(bar_y, 50);
        assert_eq!(bar_length, 48); // (100 - (2 + 2)) * 50 / 100
    }
}
