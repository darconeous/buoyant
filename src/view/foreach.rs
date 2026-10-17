//! `ForEach` view. See [`ForEach`] for details.

use core::cmp::max;

use embedded_touch::Touch;

use crate::{
    environment::LayoutEnvironment,
    event::{EventContext, EventResult, TouchResult},
    focus::{DefaultFocus, FocusKey, Seek, SeekResult},
    layout::{HorizontalAlignment, LayoutDirection, ResolvedLayout, VerticalAlignment},
    primitives::{Dimension, Dimensions, Point, ProposedDimension, ProposedDimensions},
    transition::Opacity,
    view::Event,
    view::{ViewLayout, ViewMarker},
};

/// A homogeneous collection of views, arranged vertically. Up to N views
/// will be rendered.
///
/// Alignment and spacing can be configured, and have the same behavior
/// as with `VStack`.
///
/// Example:
///
/// ```
/// use buoyant::view::{ForEach, Text};
/// use buoyant::layout::HorizontalAlignment;
/// use embedded_graphics::mono_font::ascii::FONT_6X13;
///
/// let mut names = heapless::Vec::<String, 10>::new();
/// names.push("Alice".to_string()).unwrap();
/// names.push("Bob".to_string()).unwrap();
/// names.push("Charlie".to_string()).unwrap();
///
/// ForEach::<10>::new(&names, |name| {
///     Text::new(name, &FONT_6X13)
/// })
///     .with_spacing(12)
///     .with_alignment(HorizontalAlignment::Leading);
/// ```
#[expect(missing_debug_implementations)]
pub struct ForEach<const N: usize> {}

/// Prefer using `ForEach::new` to avoid needing to specify
/// type parameters.
#[derive(Debug, Clone)]
pub struct ForEachView<'a, const N: usize, I, V, F, D = Vertical, K = Positional>
where
    F: Fn(&'a I) -> V,
{
    items: &'a [I],
    build_view: F,
    direction_with_alignment: D,
    spacing: u32,
    keys: K,
}

/// How a [`ForEach`] tells its items apart.
pub trait ItemKeys<I> {
    /// The item's key, or `None` when items are told apart by position.
    fn key(&self, item: &I) -> Option<FocusKey>;
}

/// Items are told apart by their position: focus and each item's state stay
/// at an index when the items change.
#[derive(Debug, Clone, Copy, Default)]
pub struct Positional;

impl<I> ItemKeys<I> for Positional {
    fn key(&self, _item: &I) -> Option<FocusKey> {
        None
    }
}

/// Items are told apart by a key. See [`ForEachView::keyed`].
#[derive(Debug, Clone, Copy)]
pub struct Keyed<K>(K);

impl<I, K: Fn(&I) -> FocusKey> ItemKeys<I> for Keyed<K> {
    fn key(&self, item: &I) -> Option<FocusKey> {
        Some((self.0)(item))
    }
}

/// Specifies a vertical direction for `ForEach`. Used in [`ForEachView::with_direction`].
#[derive(Debug, Clone, Copy, Default)]
pub struct Vertical(pub HorizontalAlignment);
/// Specifies a horizontal direction for `ForEach`. Used in [`ForEachView::with_direction`].
#[derive(Debug, Clone, Copy, Default)]
pub struct Horizontal(pub VerticalAlignment);

pub trait ForEachDirection: Copy + Default {
    fn layout_dir() -> LayoutDirection;
    fn align(&self, accumulated: i32, layout_size: Dimensions, item_size: Dimensions) -> Point;
    fn size_of(&self, item_size: Dimensions) -> i32;
}

impl ForEachDirection for Vertical {
    fn layout_dir() -> LayoutDirection {
        LayoutDirection::Vertical
    }

    fn align(&self, accumulated: i32, layout_size: Dimensions, item_size: Dimensions) -> Point {
        let alignment = &self.0;
        let width = alignment.align(layout_size.width.into(), item_size.width.into());
        Point::new(width, accumulated)
    }

    fn size_of(&self, item_size: Dimensions) -> i32 {
        item_size.height.into()
    }
}

impl ForEachDirection for Horizontal {
    fn layout_dir() -> LayoutDirection {
        LayoutDirection::Horizontal
    }

    fn align(&self, accumulated: i32, layout_size: Dimensions, item_size: Dimensions) -> Point {
        let alignment = &self.0;
        let height = alignment.align(layout_size.height.into(), item_size.height.into());
        Point::new(accumulated, height)
    }

    fn size_of(&self, item_size: Dimensions) -> i32 {
        item_size.width.into()
    }
}

#[derive(Debug, Clone)]
struct ForEachEnvironment<'a, T, D> {
    inner_environment: &'a T,
    _direction: D,
}

impl<T: LayoutEnvironment, D: ForEachDirection> LayoutEnvironment for ForEachEnvironment<'_, T, D> {
    fn layout_direction(&self) -> LayoutDirection {
        D::layout_dir()
    }

    fn app_time(&self) -> core::time::Duration {
        self.inner_environment.app_time()
    }
}

impl<'a, T: LayoutEnvironment, D> ForEachEnvironment<'a, T, D> {
    fn new(environment: &'a T, direction: D) -> Self {
        Self {
            inner_environment: environment,
            _direction: direction,
        }
    }
}

impl<const N: usize> ForEach<N> {
    #[allow(missing_docs)]
    #[expect(clippy::new_ret_no_self)]
    #[deprecated(since = "0.6.0", note = "Use `ForEach::<N>::new_vertical`")]
    pub fn new<'a, I, V, F>(items: &'a [I], build_view: F) -> ForEachView<'a, N, I, V, F, Vertical>
    where
        F: Fn(&'a I) -> V,
        V: ViewMarker,
    {
        Self::new_vertical(items, build_view)
    }

    #[allow(missing_docs)]
    pub fn new_vertical<'a, I, V, F>(
        items: &'a [I],
        build_view: F,
    ) -> ForEachView<'a, N, I, V, F, Vertical>
    where
        F: Fn(&'a I) -> V,
        V: ViewMarker,
    {
        ForEachView {
            items,
            build_view,
            direction_with_alignment: Vertical::default(),
            spacing: 0,
            keys: Positional,
        }
    }

    #[allow(missing_docs)]
    pub fn new_horizontal<'a, I, V, F>(
        items: &'a [I],
        build_view: F,
    ) -> ForEachView<'a, N, I, V, F, Horizontal>
    where
        F: Fn(&'a I) -> V,
    {
        ForEachView {
            items,
            build_view,
            direction_with_alignment: Horizontal::default(),
            spacing: 0,
            keys: Positional,
        }
    }
}

impl<'a, const N: usize, I, V, F, K> ForEachView<'a, N, I, V, F, Vertical, K>
where
    F: Fn(&'a I) -> V,
{
    /// Sets an alignment strategy for when child views vary in size
    #[must_use]
    pub fn with_alignment(self, alignment: HorizontalAlignment) -> Self {
        ForEachView {
            direction_with_alignment: Vertical(alignment),
            ..self
        }
    }
}

impl<'a, const N: usize, I, V, F, K> ForEachView<'a, N, I, V, F, Horizontal, K>
where
    F: Fn(&'a I) -> V,
{
    /// Sets an alignment strategy for when child views vary in size
    #[must_use]
    pub fn with_alignment(self, alignment: VerticalAlignment) -> Self {
        ForEachView {
            direction_with_alignment: Horizontal(alignment),
            ..self
        }
    }
}

impl<'a, const N: usize, I, V, F, D, K> ForEachView<'a, N, I, V, F, D, K>
where
    D: ForEachDirection,
    F: Fn(&'a I) -> V,
{
    /// Sets an direction and alignment strategy for when child views vary in size
    #[must_use]
    pub fn with_direction<D2>(self, direction: D2) -> ForEachView<'a, N, I, V, F, D2, K>
    where
        D2: ForEachDirection, // const shinenigans
    {
        ForEachView {
            items: self.items,
            build_view: self.build_view,
            direction_with_alignment: direction,
            spacing: self.spacing,
            keys: self.keys,
        }
    }

    /// Tells the items apart by `key`, rather than by their position.
    ///
    /// Focus and each item's state then follow the item: a focused item
    /// that moves stays focused, and its view keeps its state, such as a
    /// control being edited. When the focused item is removed, focus moves
    /// to the item now nearest its place. Results that focus an item carry
    /// its key, unless an element within the item names itself.
    ///
    /// Keys must be unique among the items.
    #[must_use]
    pub fn keyed<K2>(self, key: K2) -> ForEachView<'a, N, I, V, F, D, Keyed<K2>>
    where
        K2: Fn(&I) -> FocusKey,
    {
        ForEachView {
            items: self.items,
            build_view: self.build_view,
            direction_with_alignment: self.direction_with_alignment,
            spacing: self.spacing,
            keys: Keyed(key),
        }
    }

    /// Inserts spacing between child views
    #[must_use]
    pub const fn with_spacing(mut self, spacing: u32) -> Self {
        self.spacing = spacing;
        self
    }
}

impl<'a, const N: usize, I, V, F, D, K> ViewMarker for ForEachView<'a, N, I, V, F, D, K>
where
    F: Fn(&'a I) -> V,
    V: ViewMarker,
{
    type Renderables = heapless::Vec<V::Renderables, N>;
    type Transition = Opacity;
}

/// Which item of a [`ForEach`] holds focus, and the focus within it.
#[derive(Debug, Clone)]
pub struct Focus<T: DefaultFocus> {
    index: usize,
    /// The focused item's key, which finds it again when the items change.
    key: Option<FocusKey>,
    tree: T,
}

impl<T: DefaultFocus> DefaultFocus for Focus<T> {
    fn default_first() -> Self {
        Self {
            index: 0,
            key: None,
            tree: T::default_first(),
        }
    }

    fn default_last() -> Self {
        // For ForEach, we don't know the count at compile time,
        // so we use usize::MAX as a sentinel to indicate "last"
        Self {
            index: usize::MAX,
            key: None,
            tree: T::default_last(),
        }
    }
}

impl<T: DefaultFocus> Default for Focus<T> {
    fn default() -> Self {
        Self::default_first()
    }
}

/// The state of each item's view, with the key it belongs to.
#[derive(Debug, Clone)]
pub struct ForEachState<S, const N: usize> {
    keys: heapless::Vec<Option<FocusKey>, N>,
    items: heapless::Vec<S, N>,
}

impl<'a, const N: usize, I, V, F, D, K> ForEachView<'a, N, I, V, F, D, K>
where
    F: Fn(&'a I) -> V,
    K: ItemKeys<I>,
{
    fn key_at(&self, index: usize) -> Option<FocusKey> {
        self.items.get(index).and_then(|item| self.keys.key(item))
    }

    /// The index of the item focus is on, if it is still here.
    fn focused_index<T: DefaultFocus>(&self, focus: &Focus<T>) -> Option<usize> {
        match focus.key {
            Some(key) => self
                .items
                .iter()
                .position(|item| self.keys.key(item) == Some(key)),
            None if focus.index == usize::MAX => self.items.len().checked_sub(1),
            None => Some(focus.index),
        }
    }

    /// Finds the focused item again after the items changed.
    ///
    /// A keyed item is found by its key. When it is gone, focus stays at its
    /// index, clamped to the items that remain, on a fresh focus tree.
    fn resolve_focus<T: DefaultFocus>(&self, focus: &mut Focus<T>) {
        let Some(key) = focus.key else {
            return;
        };
        if self.key_at(focus.index) == Some(key) {
            return;
        }
        if let Some(index) = self
            .items
            .iter()
            .position(|item| self.keys.key(item) == Some(key))
        {
            focus.index = index;
        } else {
            focus.index = focus.index.min(self.items.len().saturating_sub(1));
            focus.key = None;
            focus.tree = T::default_first();
        }
    }

    /// Matches each item's view state to its item, by key or by position.
    fn reconcile<Captures: ?Sized>(
        &self,
        captures: &mut Captures,
        state: &mut ForEachState<V::State, N>,
    ) where
        V: ViewLayout<Captures>,
    {
        let unchanged = state.keys.len() == self.items.len().min(N)
            && self
                .items
                .iter()
                .zip(&state.keys)
                .all(|(item, key)| self.keys.key(item) == *key);
        if unchanged {
            return;
        }
        let count = self.items.len().min(N);
        let positional = self
            .items
            .first()
            .is_none_or(|item| self.keys.key(item).is_none());
        if positional {
            // Each index keeps its state; only the end of the list changes.
            state.keys.truncate(count);
            state.items.truncate(count);
            for item in &self.items[state.items.len()..count] {
                _ = state.keys.push(None);
                _ = state
                    .items
                    .push((self.build_view)(item).build_state(captures));
            }
            return;
        }
        let mut keys = core::mem::take(&mut state.keys);
        let mut items = core::mem::take(&mut state.items);
        for item in &self.items[..count] {
            let key = self.keys.key(item);
            let item_state = keys.iter().position(|k| *k == key).map_or_else(
                || (self.build_view)(item).build_state(captures),
                |previous| {
                    keys.swap_remove(previous);
                    items.swap_remove(previous)
                },
            );
            _ = state.keys.push(key);
            _ = state.items.push(item_state);
        }
    }
}

impl<'a, const N: usize, I, V, F, Captures: ?Sized, D: ForEachDirection, K: ItemKeys<I>>
    ViewLayout<Captures> for ForEachView<'a, N, I, V, F, D, K>
where
    V: ViewLayout<Captures>,
    V::FocusTree: DefaultFocus,
    F: Fn(&'a I) -> V,
{
    type Sublayout = ResolvedLayout<heapless::Vec<ResolvedLayout<V::Sublayout>, N>>;
    type State = ForEachState<V::State, N>;
    type FocusTree = Focus<V::FocusTree>;

    fn transition(&self) -> Self::Transition {
        Opacity
    }

    fn build_state(&self, captures: &mut Captures) -> Self::State {
        let mut state = ForEachState {
            keys: heapless::Vec::new(),
            items: heapless::Vec::new(),
        };
        for item in self.items {
            let view = (self.build_view)(item);
            _ = state.keys.push(self.keys.key(item));
            _ = state.items.push(view.build_state(captures));
        }
        state
    }
    // This layout implementation trades extra work for lower memory usage as embedded is the
    // primary target environment. Views are repeatedly created for every layout call, but it
    // should be assumed that this is cheap
    fn layout(
        &self,
        offer: &ProposedDimensions,
        env: &impl LayoutEnvironment,
        captures: &mut Captures,
        state: &mut Self::State,
    ) -> ResolvedLayout<Self::Sublayout> {
        self.reconcile(captures, state);
        let state = &mut state.items;
        let env = &ForEachEnvironment::new(env, self.direction_with_alignment);
        let mut sublayouts: heapless::Vec<ResolvedLayout<V::Sublayout>, N> = heapless::Vec::new();
        let mut subview_stages: heapless::Vec<(i8, bool), N> = heapless::Vec::new();

        // fill sublayouts with an initial garbage layout
        // TODO: guess there are no empty views, often no extra work needed?
        for (i, item) in self.items.iter().enumerate() {
            let view = (self.build_view)(item);
            let Some(item_state) = state.get_mut(i) else {
                break;
            };
            _ = sublayouts.push(view.layout(offer, env, captures, item_state));
            _ = subview_stages.push((view.priority(), view.is_empty()));
        }

        let mut layout_fn = |index: usize, offer: ProposedDimensions| {
            let layout = (self.build_view)(&self.items[index]).layout(
                &offer,
                env,
                captures,
                &mut state[index],
            );
            let size = layout.resolved_size;
            sublayouts[index] = layout;
            size
        };

        let direction = D::layout_dir();

        // collect the unsized subviews with the max layout priority into a group
        let mut subviews_indices: [usize; N] = [0; N];
        let mut flexibilities: [Dimension; N] = [0u32.into(); N];
        let size = layout_n(
            &subview_stages,
            &mut subviews_indices,
            &mut flexibilities,
            direction,
            *offer,
            self.spacing,
            &mut layout_fn,
        );
        ResolvedLayout {
            sublayouts,
            resolved_size: size,
        }
        .nested()
    }

    fn render_tree(
        &self,
        layout: &Self::Sublayout,
        origin: Point,
        env: &impl LayoutEnvironment,
        captures: &mut Captures,
        state: &mut Self::State,
    ) -> Self::Renderables {
        let env = &ForEachEnvironment::new(env, self.direction_with_alignment);

        let mut accumulated_size = 0;
        let mut renderables = heapless::Vec::new();

        for ((item_layout, item), item_state) in layout
            .sublayouts
            .iter()
            .zip(self.items)
            .zip(&mut state.items)
        {
            let aligned_origin = self.direction_with_alignment.align(
                accumulated_size,
                layout.resolved_size,
                item_layout.resolved_size,
            ) + origin;

            let view = (self.build_view)(item);

            // TODO: If we include an ID here, rows can be animated and transitioned
            let item = renderables.push(view.render_tree(
                &item_layout.sublayouts,
                aligned_origin,
                env,
                captures,
                item_state,
            ));
            assert!(item.is_ok());

            if !view.is_empty() {
                accumulated_size += self
                    .direction_with_alignment
                    .size_of(item_layout.resolved_size)
                    + self.spacing as i32;
            }
        }

        renderables
    }

    #[allow(clippy::too_many_lines)]
    fn handle_event(
        &self,
        event: &Event,
        context: &EventContext,
        render_tree: &mut Self::Renderables,
        captures: &mut Captures,
        state: &mut Self::State,
        focus: &mut Self::FocusTree,
    ) -> crate::event::EventResult {
        use crate::event::Event;
        use crate::focus::{FocusAction, FocusDirection};

        let state = &mut state.items;
        // Only as many items as were laid out can take events.
        let len = self.items.len().min(state.len()).min(render_tree.len());
        self.resolve_focus(focus);

        // Handle focus events specially - they need to route through the focus tree
        if let Event::Focus {
            action: focus_event,
            group,
        } = event
        {
            // Get the current focused index, resolving the usize::MAX sentinel to the actual last index
            let mut current = if focus.index == usize::MAX {
                // Sentinel for "last" - resolve to actual last index
                if len == 0 {
                    return EventResult::Deferred;
                }
                let last_index = len - 1;
                focus.index = last_index;
                last_index
            } else {
                focus.index
            };

            // The event to use - initially the original event, but when entering
            // a new child during navigation we switch to a Focus event
            let mut current_event = *focus_event;

            loop {
                // Ensure current index is valid
                if current >= len {
                    return EventResult::Deferred;
                }
                focus.key = self.key_at(current);

                // Try focus on the current child
                let item = &self.items[current];
                let view = (self.build_view)(item);
                let item_state = &mut state[current];
                let item_render_tree = &mut render_tree[current];

                let child_result = view.handle_event(
                    &Event::Focus {
                        action: current_event,
                        group: *group,
                    },
                    context,
                    item_render_tree,
                    captures,
                    item_state,
                    &mut focus.tree,
                );

                // If the child handled it (not deferred), return the result
                if !matches!(child_result, EventResult::Deferred)
                    || *focus_event == FocusAction::Teardown
                {
                    return match focus.key {
                        Some(key) => child_result.with_key(key),
                        None => child_result,
                    };
                }

                // Child is exhausted, try to move based on action
                match focus_event {
                    FocusAction::Blur | FocusAction::Teardown => {
                        return EventResult::Deferred;
                    }
                    FocusAction::Focus(FocusDirection::Forward)
                    | FocusAction::Select
                    | FocusAction::Next => {
                        // Advance to next child
                        current += 1;
                        if current >= len {
                            return EventResult::Deferred;
                        }
                        focus.index = current;
                        focus.tree = <V::FocusTree as DefaultFocus>::default_first();
                        // When entering a new child, use Focus action (forward)
                        current_event = FocusAction::Focus(FocusDirection::Forward);
                    }
                    FocusAction::Focus(FocusDirection::Backward) | FocusAction::Previous => {
                        // Go to previous child
                        if current == 0 {
                            return EventResult::Deferred;
                        }
                        current -= 1;
                        focus.index = current;
                        focus.tree = <V::FocusTree as DefaultFocus>::default_last();
                        // When entering a new child, use Focus action (backward)
                        current_event = FocusAction::Focus(FocusDirection::Backward);
                    }
                }
            }
        }

        // Key events route only to the currently focused child, not via DFS
        if matches!(event, Event::KeyDown(_) | Event::KeyUp(_)) {
            // Unresolved "last" sentinel - no focused child yet
            if focus.index >= len {
                return EventResult::Deferred;
            }
            let item = &self.items[focus.index];
            let view = (self.build_view)(item);
            return view.handle_event(
                event,
                context,
                &mut render_tree[focus.index],
                captures,
                &mut state[focus.index],
                &mut focus.tree,
            );
        }

        // For non-focus events (touch, scroll, etc.), use DFS approach
        for (i, item) in self.items.iter().take(len).enumerate() {
            let view = (self.build_view)(item);
            let item_state = &mut state[i];
            let item_render_tree = &mut render_tree[i];
            focus.tree = DefaultFocus::default_first();
            focus.index = i;
            focus.key = self.keys.key(item);
            let child_result = view.handle_event(
                event,
                context,
                item_render_tree,
                captures,
                item_state,
                &mut focus.tree,
            );
            if child_result.is_handled() {
                return child_result;
            }
        }
        EventResult::Deferred
    }

    fn seek(
        &self,
        seek: &Seek,
        render_tree: &Self::Renderables,
        state: &Self::State,
        focus: Option<&Self::FocusTree>,
    ) -> SeekResult<Self::FocusTree> {
        let len = self
            .items
            .len()
            .min(state.items.len())
            .min(render_tree.len());
        let on_path = focus.and_then(|focus| Some((self.focused_index(focus)?, &focus.tree)));
        let mut result = SeekResult::Absent;
        for (index, item) in self.items[..len].iter().enumerate() {
            let item_focus = on_path.and_then(|(i, tree)| (i == index).then_some(tree));
            let found = (self.build_view)(item)
                .seek(seek, &render_tree[index], &state.items[index], item_focus)
                .map(|tree| Focus {
                    index,
                    key: self.keys.key(item),
                    tree,
                });
            result = result.or(found);
        }
        result
    }

    fn handle_touch(
        &self,
        touch: &Touch,
        context: &EventContext,
        render_tree: &mut Self::Renderables,
        captures: &mut Captures,
        state: &mut Self::State,
    ) -> TouchResult<Self::FocusTree> {
        let state = &mut state.items;
        for ((i, item), (item_state, item_render_tree)) in self
            .items
            .iter()
            .enumerate()
            .zip(state.iter_mut().zip(render_tree.iter_mut()))
        {
            let view = (self.build_view)(item);
            match view.handle_touch(touch, context, item_render_tree, captures, item_state) {
                TouchResult::Focused(f) => {
                    return TouchResult::Focused(Focus {
                        index: i,
                        key: self.keys.key(item),
                        tree: f,
                    });
                }
                TouchResult::Handled => return TouchResult::Handled,
                TouchResult::Deferred => {}
            }
        }
        TouchResult::Deferred
    }
}

#[allow(clippy::too_many_lines)]
fn layout_n(
    subviews: &[(i8, bool)],
    subviews_indices: &mut [usize],
    flexibilities: &mut [Dimension],
    direction: LayoutDirection,
    offer: ProposedDimensions,
    spacing: u32,
    layout_fn: &mut dyn FnMut(usize, ProposedDimensions) -> Dimensions,
) -> Dimensions {
    let proposed_dimension = match direction {
        LayoutDirection::Horizontal => offer.width,
        LayoutDirection::Vertical => offer.height,
    };
    let ProposedDimension::Exact(size) = proposed_dimension else {
        // Compact or infinite offer
        let mut total_size: Dimension = 0u32.into();
        let mut max_cross_size: Dimension = 0u32.into();
        let mut non_empty_views: u32 = 0;
        for (i, (_, is_empty)) in subviews.iter().enumerate() {
            // layout must be called at least once on every view to avoid panic unwrapping the
            // resolved layout.
            let dimensions = layout_fn(i, offer);
            if *is_empty {
                continue;
            }

            let (size, cross_size) = match direction {
                LayoutDirection::Vertical => (dimensions.height, dimensions.width),
                LayoutDirection::Horizontal => (dimensions.width, dimensions.height),
            };
            total_size += size;
            max_cross_size = max(max_cross_size, cross_size);
            non_empty_views += 1;
        }
        return match direction {
            LayoutDirection::Horizontal => Dimensions {
                width: total_size + spacing * (non_empty_views.saturating_sub(1)),
                height: max_cross_size,
            },
            LayoutDirection::Vertical => Dimensions {
                width: max_cross_size,
                height: total_size + spacing * (non_empty_views.saturating_sub(1)),
            },
        };
    };

    // compute the "flexibility" of each view on the vertical axis and sort by decreasing
    // flexibility
    // Flexibility is defined as the difference between the responses to 0 and infinite height offers
    flexibilities.fill(Dimension::from(0u32));
    let mut num_empty_views = 0;
    let (min_proposal, max_proposal) = match direction {
        LayoutDirection::Horizontal => (
            ProposedDimensions {
                width: ProposedDimension::Exact(0),
                height: offer.height,
            },
            ProposedDimensions {
                width: ProposedDimension::Infinite,
                height: offer.height,
            },
        ),
        LayoutDirection::Vertical => (
            ProposedDimensions {
                width: offer.width,
                height: ProposedDimension::Exact(0),
            },
            ProposedDimensions {
                width: offer.width,
                height: ProposedDimension::Infinite,
            },
        ),
    };

    for index in 0..subviews.len() {
        let minimum_dimension = layout_fn(index, min_proposal);
        // skip any further work for empty views
        if subviews[index].1 {
            num_empty_views += 1;
            continue;
        }
        let maximum_dimension = layout_fn(index, max_proposal);
        flexibilities[index] = match direction {
            LayoutDirection::Horizontal => maximum_dimension.width - minimum_dimension.width,
            LayoutDirection::Vertical => maximum_dimension.height - minimum_dimension.height,
        };
    }

    let len = subviews.len() as u32;
    let mut remaining_size = size.saturating_sub(spacing * len.saturating_sub(num_empty_views + 1));
    let mut last_priority_group: Option<i8> = None;
    let mut max_cross_size: Dimension = 0u32.into();
    loop {
        subviews_indices.fill(0);
        let mut max = i8::MIN;
        let mut slice_start: usize = 0;
        let mut slice_len: usize = 0;
        for (i, (priority, is_empty)) in subviews.iter().enumerate() {
            if last_priority_group.is_some_and(|p| p <= *priority) || *is_empty {
                continue;
            }
            match max.cmp(priority) {
                core::cmp::Ordering::Less => {
                    max = *priority;
                    slice_start = i;
                    slice_len = 1;
                    subviews_indices[slice_start] = i;
                }
                core::cmp::Ordering::Equal => {
                    if slice_len == 0 {
                        slice_start = i;
                    }

                    subviews_indices[slice_start + slice_len] = i;
                    slice_len += 1;
                }
                core::cmp::Ordering::Greater => {}
            }
        }
        last_priority_group = Some(max);

        if slice_len == 0 {
            break;
        }

        let group_indices = &mut subviews_indices[slice_start..slice_start + slice_len];
        group_indices.sort_unstable_by_key(|&i| flexibilities[i]);

        let mut remaining_group_size = group_indices.len() as u32;

        match direction {
            LayoutDirection::Horizontal => {
                for index in group_indices {
                    let width_fraction = remaining_size / remaining_group_size
                        + remaining_size % remaining_group_size;
                    let size = layout_fn(
                        *index,
                        ProposedDimensions {
                            width: ProposedDimension::Exact(width_fraction),
                            height: offer.height,
                        },
                    );
                    remaining_size = remaining_size.saturating_sub(size.width.into());
                    remaining_group_size -= 1;
                    max_cross_size = max_cross_size.max(size.height);
                }
            }
            LayoutDirection::Vertical => {
                for index in group_indices {
                    let height_fraction = remaining_size / remaining_group_size
                        + remaining_size % remaining_group_size;
                    let size = layout_fn(
                        *index,
                        ProposedDimensions {
                            width: offer.width,
                            height: ProposedDimension::Exact(height_fraction),
                        },
                    );
                    remaining_size = remaining_size.saturating_sub(size.height.into());
                    remaining_group_size -= 1;
                    max_cross_size = max_cross_size.max(size.width);
                }
            }
        }
    }

    match direction {
        LayoutDirection::Horizontal => Dimensions {
            width: (size.saturating_sub(remaining_size)).into(),
            height: max_cross_size,
        },
        LayoutDirection::Vertical => Dimensions {
            width: max_cross_size,
            height: (size.saturating_sub(remaining_size)).into(),
        },
    }
}
