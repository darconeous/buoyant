//! `LazyForEach` view. See [`LazyForEach`] for details.

use embedded_touch::Touch;

use crate::{
    environment::{DefaultEnvironment, LayoutEnvironment, Span, Viewport},
    event::{Event, EventContext, EventResult, TouchResult},
    focus::{
        DefaultFocus, FocusAction, FocusDirection, FocusKey, Seek, SeekResult, SeekTarget,
        center_along, nearest_first,
    },
    layout::{HorizontalAlignment, LayoutDirection, ResolvedLayout},
    primitives::{Dimensions, Point, ProposedDimension, ProposedDimensions},
    render::{ContentShape, LazyRows, RowId},
    transition::Opacity,
    view::{ViewLayout, ViewMarker},
};

use super::foreach::{Focus, Keyed, Positional};

/// A vertical list of `count` rows, of which only those near what the
/// enclosing [`ScrollView`](crate::view::ScrollView) shows are built: at
/// most `N` at a time.
///
/// Rows are made from their index on demand, so a long list costs no more
/// than the rows near the view. As the content moves, the list builds the
/// rows coming into view and drops those far from it.
///
/// Rows are placed one of two ways:
///
/// - [`new_vertical`](LazyForEach::new_vertical) takes an [`Extent`], which
///   says how tall every row is, so the list knows where every row is
///   without building it.
/// - [`new_natural`](LazyForEach::new_natural) lets each row take the height
///   it lays itself out to, measured as it is built, and takes the rows not
///   yet built to be about as tall as an estimate. As rows are measured,
///   added, or removed above what the view shows, the scroll view moves with
///   them, so what it shows stays where it was.
///
/// ```
/// use buoyant::view::{LazyForEach, ScrollView, Text};
/// use buoyant::view::lazy_foreach::Extent;
/// use embedded_graphics::mono_font::ascii::FONT_6X10;
///
/// const NAMES: [&str; 3] = ["Alice", "Bob", "Charlie"];
///
/// ScrollView::new(LazyForEach::<8>::new_vertical(
///     NAMES.len(),
///     Extent::Fixed(10),
///     |i| Text::new(NAMES[i], &FONT_6X10),
/// ));
/// ```
///
/// # Focus
///
/// Focus moves through the rows as through a [`ForEach`](crate::view::ForEach),
/// building rows it reaches. The focused row is kept built wherever the
/// content moves, so a control being edited keeps its state. Keyed rows,
/// with [`keyed`](LazyForEachView::keyed), can be focused by key with a
/// focus request even when they are not built: the request builds the row.
///
/// # Capacity
///
/// `N` must hold the rows the view shows, counting those partly shown at
/// either end, and two more, for the focused row and a row being reached
/// while out of view. Rows of natural height are counted at the height of
/// the shortest, which must not be zero. A list given less, as by a view
/// grown taller, builds the focused row and the shown rows nearest the
/// middle of the view, and leaves those at the edges out; debug builds
/// panic instead.
///
/// # Where it works
///
/// Rows are built around what a scroll view shows only where the list sees
/// the scroll view's viewport. Under a scale effect, inside a
/// [`Lazy`](crate::view::Lazy), and outside any scroll view, the list builds
/// its first `N` rows, as a [`ForEach`](crate::view::ForEach) would. Nothing
/// between the scroll view and the list may move it during an animation, as
/// an animated offset or a sibling above it changing size would: the list
/// takes its new place at once rather than sliding, so what it built is
/// where it is drawn.
///
/// Rows of natural height hold what the view shows still only for the one
/// such list a scroll view holds, laid out directly within it; a second, or
/// one inside a row of another list, places its rows where their heights
/// put them.
#[expect(missing_debug_implementations)]
pub struct LazyForEach<const N: usize> {}

/// How tall the rows of a [`LazyForEach`] are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Extent {
    /// Every row is this tall. Rows are offered exactly this height.
    Fixed(u32),
}

/// Rows of a [`LazyForEach`] as tall as they lay themselves out to be.
///
/// A row is measured when it is built; until then it is taken to be the
/// estimate tall. The list remembers the heights of up to `M` rows it no
/// longer has built, so that content moved back and forth is measured
/// again less often. Forgetting a height never moves what is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Natural<const M: usize> {
    estimate: u32,
}

impl<const M: usize> Natural<M> {
    /// Rows taken, until they are built, to be `estimate` tall.
    #[must_use]
    pub const fn new(estimate: u32) -> Self {
        Self { estimate }
    }
}

mod sealed {
    pub trait Sealed {}
}

impl sealed::Sealed for Extent {}
impl<const M: usize> sealed::Sealed for Natural<M> {}

/// How a [`LazyForEach`] knows its rows' heights: an [`Extent`], or
/// [`Natural`].
pub trait RowExtent: sealed::Sealed + Copy + core::fmt::Debug {
    /// What the list keeps of where each built row is.
    type Placed: Copy + Default + core::fmt::Debug;
    /// What the list keeps of its rows from one build to the next.
    type Memory: Clone + Default + core::fmt::Debug;
}

impl RowExtent for Extent {
    type Placed = ();
    type Memory = ();
}

impl<const M: usize> RowExtent for Natural<M> {
    type Placed = Placed;
    type Memory = Memory<M>;
}

/// Where a built row of natural height is, from the list's top, and how
/// tall it measured.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Placed {
    top: i32,
    height: u32,
}

impl Placed {
    fn bottom(self) -> i32 {
        self.top.saturating_add(to_i32(self.height))
    }
}

/// Which edge of a row stays where it was drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edge {
    Top,
    Bottom,
}

/// The row whose edge stays where it was drawn while the rows around it are
/// measured, added, and removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Anchor {
    id: RowId,
    index: usize,
    edge: Edge,
}

/// What a list of rows of natural height keeps from one build to the next.
#[derive(Debug, Clone, Default)]
pub struct Memory<const M: usize> {
    /// The row, and its edge, that stays where it was drawn.
    anchor: Option<Anchor>,
    /// The heights of rows no longer built, in the order of their indices.
    remembered: heapless::Vec<(RowId, usize, u32), M>,
    /// The width the heights were measured at.
    width: Option<u32>,
}

impl<const M: usize> Memory<M> {
    /// Keeps the height of a row no longer built, in place of the height
    /// farthest from `near` when there is no room.
    fn remember(&mut self, id: RowId, index: usize, height: u32, near: usize) {
        if let Some(entry) = self.remembered.iter_mut().find(|(row, ..)| *row == id) {
            *entry = (id, index, height);
            return;
        }
        if self.remembered.is_full() {
            let Some(farthest) = (0..self.remembered.len())
                .max_by_key(|&slot| self.remembered[slot].1.abs_diff(near))
            else {
                return;
            };
            if self.remembered[farthest].1.abs_diff(near) <= index.abs_diff(near) {
                return;
            }
            self.remembered.remove(farthest);
        }
        let slot = self
            .remembered
            .iter()
            .position(|(_, row, _)| *row > index)
            .unwrap_or(self.remembered.len());
        _ = self.remembered.insert(slot, (id, index, height));
    }

    fn forget(&mut self, id: RowId) {
        self.remembered.retain(|(row, ..)| *row != id);
    }
}

/// Prefer using [`LazyForEach::new_vertical`] or [`LazyForEach::new_natural`]
/// to avoid needing to specify type parameters.
#[derive(Debug, Clone)]
pub struct LazyForEachView<const N: usize, V, F, K = Positional, X = Extent>
where
    F: Fn(usize) -> V,
{
    count: usize,
    extent: X,
    build_view: F,
    keys: K,
    spacing: u32,
    alignment: HorizontalAlignment,
}

/// How a [`LazyForEach`] tells its rows apart.
pub trait RowKeys {
    /// The key of the row at `index`, or `None` when rows are told apart by
    /// position.
    fn key(&self, index: usize) -> Option<FocusKey>;
}

impl RowKeys for Positional {
    fn key(&self, _index: usize) -> Option<FocusKey> {
        None
    }
}

impl<K: Fn(usize) -> FocusKey> RowKeys for Keyed<K> {
    fn key(&self, index: usize) -> Option<FocusKey> {
        Some((self.0)(index))
    }
}

impl<const N: usize> LazyForEach<N> {
    /// A list of `count` rows, each as tall as `extent` says, the row at an
    /// index made by `build_view`.
    pub fn new_vertical<V, F>(
        count: usize,
        extent: Extent,
        build_view: F,
    ) -> LazyForEachView<N, V, F, Positional, Extent>
    where
        F: Fn(usize) -> V,
        V: ViewMarker,
    {
        LazyForEachView {
            count,
            extent,
            build_view,
            keys: Positional,
            spacing: 0,
            alignment: HorizontalAlignment::default(),
        }
    }

    /// A list of `count` rows, each as tall as it lays itself out to be, the
    /// row at an index made by `build_view`. Rows not yet built are taken to
    /// be as tall as `natural` estimates.
    pub fn new_natural<const M: usize, V, F>(
        count: usize,
        natural: Natural<M>,
        build_view: F,
    ) -> LazyForEachView<N, V, F, Positional, Natural<M>>
    where
        F: Fn(usize) -> V,
        V: ViewMarker,
    {
        LazyForEachView {
            count,
            extent: natural,
            build_view,
            keys: Positional,
            spacing: 0,
            alignment: HorizontalAlignment::default(),
        }
    }
}

impl<const N: usize, V, F, K, X> LazyForEachView<N, V, F, K, X>
where
    F: Fn(usize) -> V,
{
    /// Tells the rows apart by `key`, rather than by their position.
    ///
    /// Focus and each built row's state then follow the row as rows are
    /// added and removed, and the row can be focused by its key even when it
    /// is not built. Keys must be unique among the rows.
    #[must_use]
    pub fn keyed<K2>(self, key: K2) -> LazyForEachView<N, V, F, Keyed<K2>, X>
    where
        K2: Fn(usize) -> FocusKey,
    {
        LazyForEachView {
            count: self.count,
            extent: self.extent,
            build_view: self.build_view,
            keys: Keyed(key),
            spacing: self.spacing,
            alignment: self.alignment,
        }
    }

    /// Inserts spacing between rows.
    #[must_use]
    pub const fn with_spacing(mut self, spacing: u32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Places rows narrower than the list.
    #[must_use]
    pub const fn with_alignment(mut self, alignment: HorizontalAlignment) -> Self {
        self.alignment = alignment;
        self
    }
}

impl<const N: usize, V, F, K, X> ViewMarker for LazyForEachView<N, V, F, K, X>
where
    F: Fn(usize) -> V,
    V: ViewMarker,
{
    type Renderables = LazyRows<V::Renderables, N>;
    type Transition = Opacity;
}

/// The built rows of a [`LazyForEach`], with their states, and the rows it
/// keeps built wherever the content moves.
#[derive(Debug, Clone)]
pub struct LazyForEachState<S, const N: usize, X: RowExtent = Extent> {
    /// Each built row's identity, index, place, and state, in the order of
    /// their indices, as in the render tree.
    rows: heapless::Vec<(RowId, usize, X::Placed, S), N>,
    /// The row focus was last on, with where it last was.
    focused: Option<(RowId, usize)>,
    /// A row to build at the next build, for a focus request to reach.
    realize: Option<(RowId, usize)>,
    /// What the rows' heights keep from one build to the next.
    memory: X::Memory,
}

/// The rows chosen to build.
struct Chosen<const N: usize> {
    /// Their indices, in order.
    rows: heapless::Vec<usize, N>,
    /// The rows from `first` up to `last` are shown, where the list sees its
    /// viewport.
    shown: Option<(usize, usize)>,
    /// Not all shown rows could be built.
    short: bool,
}

/// A row's environment: laid out in a vertical list, shown through what
/// the list is shown through. A row reports no move to the scroll view: the
/// list holds what it shows still.
struct RowEnvironment<'a, E> {
    inner: &'a E,
}

impl<E: LayoutEnvironment> LayoutEnvironment for RowEnvironment<'_, E> {
    fn layout_direction(&self) -> LayoutDirection {
        LayoutDirection::Vertical
    }

    fn app_time(&self) -> core::time::Duration {
        self.inner.app_time()
    }

    fn viewport(&self) -> Option<&Viewport> {
        self.inner.viewport()
    }
}

/// A pixel quantity as an `i32`, saturating.
fn to_i32<T: TryInto<i32>>(value: T) -> i32 {
    value.try_into().unwrap_or(i32::MAX)
}

/// A row built while the list handles an event, between rebuilds: it joins
/// the built rows only if it answers the event.
struct Loose<S, R, P> {
    index: usize,
    state: S,
    tree: R,
    placed: P,
    /// The built row it takes the place of, the list being full.
    victim: Option<usize>,
}

/// How a list handling an event builds the row at an index on the side,
/// `None` with no room for it; adds it to its built rows; and finds a built
/// row's center.
type BuildLoose<Me, Captures, T, S, L> =
    fn(&Me, usize, &EventContext, &T, &mut Captures, &S) -> Option<L>;
type Keep<Me, T, S, L> = fn(&Me, L, &mut T, &mut S) -> bool;
type Center<Me, S, T> = fn(&Me, &S, &T, usize) -> i32;

impl<const N: usize, V, F, K, X> LazyForEachView<N, V, F, K, X>
where
    F: Fn(usize) -> V,
    K: RowKeys,
    X: RowExtent,
{
    fn key_at(&self, index: usize) -> Option<FocusKey> {
        if index < self.count {
            self.keys.key(index)
        } else {
            None
        }
    }

    fn id_at(&self, index: usize) -> RowId {
        self.keys.key(index).map_or(RowId::Index(index), RowId::Key)
    }

    /// Where row `id`, last at `last`, is now.
    fn find(&self, id: RowId, last: usize) -> Option<usize> {
        match id {
            RowId::Index(index) => (index < self.count).then_some(index),
            RowId::Key(key) if self.key_at(last) == Some(key) => Some(last),
            RowId::Key(key) => (0..self.count).find(|&index| self.key_at(index) == Some(key)),
        }
    }

    /// The index of the row focus is on, if it is still here.
    fn focused_index<T: DefaultFocus>(&self, focus: &Focus<T>) -> Option<usize> {
        match focus.key {
            Some(key) => self.find(RowId::Key(key), focus.index),
            None if focus.index == usize::MAX => self.count.checked_sub(1),
            None => Some(focus.index),
        }
    }

    /// Finds the focused row again after the rows changed.
    ///
    /// A keyed row is found by its key. When it is gone, focus stays at its
    /// index, clamped to the rows that remain, on a fresh focus tree.
    fn resolve_focus<T: DefaultFocus>(&self, focus: &mut Focus<T>) {
        let Some(key) = focus.key else {
            return;
        };
        if let Some(index) = self.find(RowId::Key(key), focus.index) {
            focus.index = index;
        } else {
            focus.index = focus.index.min(self.count.saturating_sub(1));
            focus.key = None;
            focus.tree = T::default_first();
        }
    }

    /// Where the built row at `index` is in the state and the render tree.
    fn slot<S, R>(
        &self,
        state: &LazyForEachState<S, N, X>,
        render_tree: &LazyRows<R, N>,
        index: usize,
    ) -> Option<usize> {
        if index >= self.count {
            return None;
        }
        let id = self.id_at(index);
        let slot = state
            .rows
            .iter()
            .position(|(row_id, row, ..)| *row == index && *row_id == id)?;
        render_tree
            .rows
            .get(slot)
            .is_some_and(|(row_id, row, _)| *row == index && *row_id == id)
            .then_some(slot)
    }

    /// How far across a list `width` wide a row `row_width` wide is placed.
    fn row_x(&self, width: u32, row_width: u32) -> i32 {
        self.alignment.align(to_i32(width), to_i32(row_width))
    }

    /// The focused row, and one a focus request is to reach, wherever they
    /// are now. Takes the request.
    fn kept<S>(&self, state: &mut LazyForEachState<S, N, X>) -> [Option<usize>; 2] {
        let found = |kept: Option<(RowId, usize)>| {
            kept.and_then(|(id, last)| self.find(id, last).map(|index| (self.id_at(index), index)))
        };
        state.focused = found(state.focused);
        let realize = found(state.realize.take());
        [
            state.focused.map(|(_, index)| index),
            realize.map(|(_, index)| index),
        ]
    }

    /// Keeps the row at `index` built when it took focus.
    fn keep_if_focused<S>(
        &self,
        state: &mut LazyForEachState<S, N, X>,
        index: usize,
        result: &EventResult,
    ) {
        if let EventResult::Handled { shape, .. } = result
            && *shape != ContentShape::Empty
        {
            state.focused = Some((self.id_at(index), index));
        }
    }

    fn named(&self, index: usize, result: EventResult) -> EventResult {
        match self.key_at(index) {
            Some(key) => result.with_key(key),
            None => result,
        }
    }

    fn empty_state<S>() -> LazyForEachState<S, N, X> {
        LazyForEachState {
            rows: heapless::Vec::new(),
            focused: None,
            realize: None,
            memory: X::Memory::default(),
        }
    }

    /// How wide the list is when offered `offer`: offered no width, as wide
    /// as its first row laid out at `height`.
    fn width<Captures: ?Sized>(
        &self,
        offer: &ProposedDimensions,
        height: ProposedDimension,
        env: &impl LayoutEnvironment,
        captures: &mut Captures,
    ) -> u32
    where
        V: ViewLayout<Captures>,
    {
        match offer.width {
            ProposedDimension::Exact(width) => width,
            ProposedDimension::Compact | ProposedDimension::Infinite if self.count > 0 => {
                let view = (self.build_view)(0);
                let mut row_state = view.build_state(captures);
                let offer = ProposedDimensions {
                    width: offer.width,
                    height,
                };
                view.layout(
                    &offer,
                    &RowEnvironment { inner: env },
                    captures,
                    &mut row_state,
                )
                .resolved_size
                .width
                .into()
            }
            ProposedDimension::Compact | ProposedDimension::Infinite => 0,
        }
    }

    #[expect(
        clippy::too_many_lines,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    fn handle<Captures: ?Sized>(
        &self,
        event: &Event,
        context: &EventContext,
        render_tree: &mut LazyRows<V::Renderables, N>,
        captures: &mut Captures,
        state: &mut LazyForEachState<V::State, N, X>,
        focus: &mut Focus<V::FocusTree>,
        loose: BuildLoose<
            Self,
            Captures,
            LazyRows<V::Renderables, N>,
            LazyForEachState<V::State, N, X>,
            Loose<V::State, V::Renderables, X::Placed>,
        >,
        keep: Keep<
            Self,
            LazyRows<V::Renderables, N>,
            LazyForEachState<V::State, N, X>,
            Loose<V::State, V::Renderables, X::Placed>,
        >,
        center: Center<Self, LazyForEachState<V::State, N, X>, LazyRows<V::Renderables, N>>,
    ) -> EventResult
    where
        V: ViewLayout<Captures>,
        V::FocusTree: DefaultFocus,
    {
        self.resolve_focus(focus);

        if let Event::Focus {
            action: focus_event,
            group,
        } = event
        {
            let focus_event_for = |action: FocusAction| Event::Focus {
                action,
                group: *group,
            };

            // Torn down, focus leaves the list, whatever the row says: a
            // control can answer as if it kept focus while it lets go.
            if *focus_event == FocusAction::Teardown {
                state.focused = None;
                let index = self.focused_index(focus).unwrap_or(focus.index);
                let Some(slot) = self.slot(state, render_tree, index) else {
                    return EventResult::Deferred;
                };
                focus.index = index;
                let result = (self.build_view)(index).handle_event(
                    event,
                    context,
                    &mut render_tree.rows[slot].2,
                    captures,
                    &mut state.rows[slot].3,
                    &mut focus.tree,
                );
                return self.named(index, result);
            }

            // Entered across its axis, the list starts at the built row
            // nearest where focus comes from.
            if let FocusAction::Enter { dir, from } = focus_event
                && dir.axis() != LayoutDirection::Vertical
            {
                let target = center_along(from, LayoutDirection::Vertical);
                let centers: heapless::Vec<i32, N> = (0..state.rows.len())
                    .map(|slot| center(self, state, render_tree, slot))
                    .collect();
                let order: heapless::Vec<usize, N> =
                    nearest_first(centers.len(), |slot| centers[slot], target).collect();
                for slot in order {
                    let index = state.rows[slot].1;
                    if self.slot(state, render_tree, index) != Some(slot) {
                        continue;
                    }
                    focus.index = index;
                    focus.key = self.key_at(index);
                    focus.tree = match dir.focus_direction() {
                        FocusDirection::Forward => DefaultFocus::default_first(),
                        FocusDirection::Backward => DefaultFocus::default_last(),
                    };
                    let result = (self.build_view)(index).handle_event(
                        event,
                        context,
                        &mut render_tree.rows[slot].2,
                        captures,
                        &mut state.rows[slot].3,
                        &mut focus.tree,
                    );
                    if result.is_handled() {
                        self.keep_if_focused(state, index, &result);
                        return self.named(index, result);
                    }
                }
                return EventResult::Deferred;
            }

            let Some(mut current) = self.focused_index(focus) else {
                return EventResult::Deferred;
            };
            let mut current_event = *focus_event;
            loop {
                if current >= self.count {
                    return EventResult::Deferred;
                }
                focus.index = current;
                focus.key = self.key_at(current);
                let view = (self.build_view)(current);
                let current_focus_event = focus_event_for(current_event);
                let result = if let Some(slot) = self.slot(state, render_tree, current) {
                    view.handle_event(
                        &current_focus_event,
                        context,
                        &mut render_tree.rows[slot].2,
                        captures,
                        &mut state.rows[slot].3,
                        &mut focus.tree,
                    )
                } else {
                    // A row not built is built as moving through the list
                    // reaches it, and kept only if it answers: a search
                    // that finds nothing leaves the list as it was.
                    let Some(mut row) = loose(self, current, context, render_tree, captures, state)
                    else {
                        return EventResult::Deferred;
                    };
                    let result = view.handle_event(
                        &current_focus_event,
                        context,
                        &mut row.tree,
                        captures,
                        &mut row.state,
                        &mut focus.tree,
                    );
                    if !matches!(result, EventResult::Deferred) {
                        if !keep(self, row, render_tree, state) {
                            return EventResult::Deferred;
                        }
                        // What the list vouched for may no longer hold.
                        render_tree.stale = true;
                        context.request_view_rebuild();
                    }
                    result
                };
                if !matches!(result, EventResult::Deferred) {
                    self.keep_if_focused(state, current, &result);
                    return self.named(current, result);
                }

                // The row is done. A move across the axis is the row's alone.
                match focus_event.stack_step(Some(LayoutDirection::Vertical)) {
                    None => return EventResult::Deferred,
                    Some(FocusDirection::Forward) => {
                        current += 1;
                        if current >= self.count {
                            return EventResult::Deferred;
                        }
                        focus.tree = DefaultFocus::default_first();
                        current_event = focus_event.entering(FocusDirection::Forward);
                    }
                    Some(FocusDirection::Backward) => {
                        let Some(previous) = current.checked_sub(1) else {
                            return EventResult::Deferred;
                        };
                        current = previous;
                        focus.tree = DefaultFocus::default_last();
                        current_event = focus_event.entering(FocusDirection::Backward);
                    }
                }
            }
        }

        // Key events go only to the focused row.
        if matches!(event, Event::KeyDown(_) | Event::KeyUp(_)) {
            let Some(slot) = self.slot(state, render_tree, focus.index) else {
                return EventResult::Deferred;
            };
            return (self.build_view)(focus.index).handle_event(
                event,
                context,
                &mut render_tree.rows[slot].2,
                captures,
                &mut state.rows[slot].3,
                &mut focus.tree,
            );
        }

        // Asked for a row it holds but has not built, the list builds it
        // next time.
        if let Event::Realize(key) = event
            && let Some(index) = (0..self.count).find(|&index| self.key_at(index) == Some(*key))
            && self.slot(state, render_tree, index).is_none()
        {
            state.realize = Some((RowId::Key(*key), index));
            return EventResult::handled_unfocused();
        }

        // Anything else goes to each built row in turn.
        for slot in 0..state.rows.len() {
            let index = state.rows[slot].1;
            if self.slot(state, render_tree, index) != Some(slot) {
                continue;
            }
            focus.tree = DefaultFocus::default_first();
            focus.index = index;
            focus.key = self.key_at(index);
            let result = (self.build_view)(index).handle_event(
                event,
                context,
                &mut render_tree.rows[slot].2,
                captures,
                &mut state.rows[slot].3,
                &mut focus.tree,
            );
            if result.is_handled() {
                return result;
            }
        }
        EventResult::Deferred
    }

    fn seek_rows<Captures: ?Sized>(
        &self,
        seek: &Seek,
        render_tree: &LazyRows<V::Renderables, N>,
        state: &LazyForEachState<V::State, N, X>,
        focus: Option<&Focus<V::FocusTree>>,
    ) -> SeekResult<Focus<V::FocusTree>>
    where
        V: ViewLayout<Captures>,
        V::FocusTree: DefaultFocus,
    {
        let on_path = focus.and_then(|focus| Some((self.focused_index(focus)?, &focus.tree)));
        let mut result = SeekResult::Absent;
        for (slot, (_, index, _, row_state)) in state.rows.iter().enumerate() {
            let index = *index;
            if self.slot(state, render_tree, index) != Some(slot) {
                continue;
            }
            let row_focus = on_path.and_then(|(i, tree)| (i == index).then_some(tree));
            let key = self.key_at(index);
            // Asking for a row's key asks for the row.
            let row_seek = match seek.target {
                SeekTarget::Key(target) if key == Some(target) => seek.with_target(SeekTarget::Any),
                _ => *seek,
            };
            let found = (self.build_view)(index)
                .seek(&row_seek, &render_tree.rows[slot].2, row_state, row_focus)
                .map(|tree| Focus { index, key, tree });
            result = result.or(found);
        }
        result
    }

    fn touch_rows<Captures: ?Sized>(
        &self,
        touch: &Touch,
        context: &EventContext,
        render_tree: &mut LazyRows<V::Renderables, N>,
        captures: &mut Captures,
        state: &mut LazyForEachState<V::State, N, X>,
    ) -> TouchResult<Focus<V::FocusTree>>
    where
        V: ViewLayout<Captures>,
    {
        for slot in 0..state.rows.len() {
            let index = state.rows[slot].1;
            if self.slot(state, render_tree, index) != Some(slot) {
                continue;
            }
            let view = (self.build_view)(index);
            match view.handle_touch(
                touch,
                context,
                &mut render_tree.rows[slot].2,
                captures,
                &mut state.rows[slot].3,
            ) {
                TouchResult::Focused(tree) => {
                    return TouchResult::Focused(Focus {
                        index,
                        key: self.key_at(index),
                        tree,
                    });
                }
                TouchResult::Handled => return TouchResult::Handled,
                TouchResult::Deferred => {}
            }
        }
        TouchResult::Deferred
    }
}

// ─── Rows of one height ──────────────────────────────────────────────────

impl<const N: usize, V, F, K> LazyForEachView<N, V, F, K, Extent>
where
    F: Fn(usize) -> V,
    K: RowKeys,
{
    const fn extent(&self) -> u32 {
        match self.extent {
            Extent::Fixed(height) => height,
        }
    }

    /// From one row's top to the next's.
    fn pitch(&self) -> i32 {
        to_i32(self.extent().saturating_add(self.spacing)).max(1)
    }

    /// Where row `index` starts, from the first row's top.
    fn offset_of(&self, index: usize) -> i32 {
        to_i32(index).saturating_mul(self.pitch())
    }

    /// Where row `index` ends.
    fn end_of(&self, index: usize) -> i32 {
        self.offset_of(index).saturating_add(to_i32(self.extent()))
    }

    /// The height of every row and the spacing between them.
    fn total(&self) -> u32 {
        let count = u32::try_from(self.count).unwrap_or(u32::MAX);
        count.checked_sub(1).map_or(0, |gaps| {
            count
                .saturating_mul(self.extent())
                .saturating_add(gaps.saturating_mul(self.spacing))
        })
    }

    /// The rows `span`, from the first row's top, shows: from the first up
    /// to the last.
    fn rows_in(&self, span: Span) -> (usize, usize) {
        let extent = to_i32(self.extent());
        let pitch = self.pitch();
        let index = |at: i32| usize::try_from(at).unwrap_or(usize::MAX);
        // The first whose end is past the start, and the first that starts
        // at or past the end.
        let first = if span.start < extent {
            0
        } else {
            index((span.start - extent) / pitch).saturating_add(1)
        };
        let last = if span.end <= 0 {
            0
        } else {
            index(span.end.saturating_add(pitch - 1) / pitch)
        };
        let first = first.min(self.count);
        (first, last.min(self.count).max(first))
    }

    fn row_offer(&self, width: u32) -> ProposedDimensions {
        ProposedDimensions {
            width: ProposedDimension::Exact(width),
            height: ProposedDimension::Exact(self.extent()),
        }
    }

    /// Where row `index`, `row_width` wide, is placed in a list at `origin`,
    /// `width` wide.
    fn row_origin(&self, origin: Point, width: u32, index: usize, row_width: u32) -> Point {
        origin + Point::new(self.row_x(width, row_width), self.offset_of(index))
    }

    /// The rows to build: those kept built wherever the content moves, then
    /// those shown, then those beyond, nearest first, alternately below and
    /// above.
    fn choose(&self, visible: Option<Span>, kept: [Option<usize>; 2]) -> Chosen<N> {
        let mut rows = heapless::Vec::<usize, N>::new();
        let add = |rows: &mut heapless::Vec<usize, N>, index: usize| {
            if !rows.contains(&index) {
                _ = rows.push(index);
            }
        };
        for index in kept.into_iter().flatten() {
            add(&mut rows, index);
        }
        let Some(visible) = visible else {
            for index in 0..self.count {
                if rows.is_full() {
                    break;
                }
                add(&mut rows, index);
            }
            rows.sort_unstable();
            return Chosen {
                rows,
                shown: None,
                short: false,
            };
        };
        let (first, last) = self.rows_in(visible);
        let unbuilt = (first..last).filter(|index| !rows.contains(index)).count();
        let short = unbuilt > N - rows.len();
        if short {
            debug_assert!(
                false,
                "a LazyForEach of {N} rows cannot build the {} rows it shows",
                last - first
            );
            // The rows nearest the middle of the view.
            let middle = to_i32((last - first) / 2);
            for offset in nearest_first(last - first, to_i32, middle) {
                if rows.is_full() {
                    break;
                }
                add(&mut rows, first + offset);
            }
        } else {
            for index in first..last {
                add(&mut rows, index);
            }
            let (mut below, mut above) = (last, first);
            while !rows.is_full() && (below < self.count || above > 0) {
                if below < self.count {
                    add(&mut rows, below);
                    below += 1;
                }
                if !rows.is_full() && above > 0 {
                    above -= 1;
                    add(&mut rows, above);
                }
            }
        }
        rows.sort_unstable();
        Chosen {
            rows,
            shown: Some((first, last)),
            short,
        }
    }

    /// What the built `rows`, in order, vouch for around the rows `first` up
    /// to `last`, which are shown: where the view can be with everything it
    /// shows built, and where it can be with no need to build more.
    fn coverage(&self, rows: &[usize], first: usize, last: usize) -> (Span, Span) {
        let built = |index: usize| rows.binary_search(&index).is_ok();
        let mut start = first;
        while start > 0 && built(start - 1) {
            start -= 1;
        }
        let mut end = last;
        while end < self.count && built(end) {
            end += 1;
        }
        // Half the rows built beyond the view on each side are its margin.
        let above = (first - start).div_ceil(2);
        let below = (end - last).div_ceil(2);
        let unbounded_above = start == 0;
        let unbounded_below = end >= self.count;
        let realized = Span::new(
            if unbounded_above {
                i32::MIN
            } else {
                self.end_of(start - 1)
            },
            if unbounded_below {
                i32::MAX
            } else {
                self.offset_of(end)
            },
        );
        let safe = Span::new(
            if unbounded_above {
                i32::MIN
            } else {
                self.end_of(start - 1 + above)
            },
            if unbounded_below {
                i32::MAX
            } else {
                self.offset_of(end - below)
            },
        );
        (realized, safe)
    }

    fn layout_rows<Captures: ?Sized>(
        &self,
        offer: &ProposedDimensions,
        env: &impl LayoutEnvironment,
        captures: &mut Captures,
    ) -> ResolvedLayout<Dimensions>
    where
        V: ViewLayout<Captures>,
    {
        let height = ProposedDimension::Exact(self.extent());
        let size = Dimensions {
            width: self.width(offer, height, env, captures).into(),
            height: self.total().into(),
        };
        ResolvedLayout {
            sublayouts: size,
            resolved_size: size,
        }
    }

    fn build_rows<Captures: ?Sized>(
        &self,
        layout: Dimensions,
        origin: Point,
        env: &impl LayoutEnvironment,
        captures: &mut Captures,
        state: &mut LazyForEachState<V::State, N, Extent>,
    ) -> LazyRows<V::Renderables, N>
    where
        V: ViewLayout<Captures>,
    {
        let width: u32 = layout.width.into();
        let viewport = env.viewport();
        let visible = viewport.map(|viewport| {
            viewport
                .visible_along(LayoutDirection::Vertical)
                .shifted(-origin.y)
        });
        let chosen = self.choose(visible, self.kept(state));

        // Each row keeps its state while it stays built.
        state.rows.retain_mut(|(id, index, (), _)| {
            match chosen.rows.iter().find(|&&row| self.id_at(row) == *id) {
                Some(&row) => {
                    *index = row;
                    true
                }
                None => false,
            }
        });
        for &index in &chosen.rows {
            let id = self.id_at(index);
            if !state.rows.iter().any(|(row_id, ..)| *row_id == id) {
                let row_state = (self.build_view)(index).build_state(captures);
                _ = state.rows.push((id, index, (), row_state));
            }
        }
        state.rows.sort_unstable_by_key(|(_, index, ..)| *index);

        let env = RowEnvironment { inner: env };
        let mut tree = LazyRows::new(origin, width, visible);
        for (id, index, (), row_state) in &mut state.rows {
            let view = (self.build_view)(*index);
            let layout = view.layout(&self.row_offer(width), &env, captures, row_state);
            let at = self.row_origin(origin, width, *index, layout.resolved_size.width.into());
            let row = view.render_tree(&layout.sublayouts, at, &env, captures, row_state);
            _ = tree.rows.push((*id, *index, row));
        }

        if let (Some(viewport), Some((first, last))) = (viewport, chosen.shown) {
            // Short of rows, the list asks for no more: it would get none.
            let (realized, safe) = if chosen.short {
                (Span::UNBOUNDED, Span::UNBOUNDED)
            } else {
                self.coverage(&chosen.rows, first, last)
            };
            viewport.vouch(
                LayoutDirection::Vertical,
                realized.shifted(origin.y),
                safe.shifted(origin.y),
            );
        }
        tree
    }

    /// Builds row `index` on the side while handling an event, between
    /// rebuilds, to take the place of the built row farthest from the view
    /// that is neither shown nor focused, or failing one, the row farthest
    /// from it that is not focused. `None` when there is no room.
    fn loose<Captures: ?Sized>(
        &self,
        index: usize,
        context: &EventContext,
        render_tree: &LazyRows<V::Renderables, N>,
        captures: &mut Captures,
        state: &LazyForEachState<V::State, N, Extent>,
    ) -> Option<Loose<V::State, V::Renderables, ()>>
    where
        V: ViewLayout<Captures>,
    {
        let victim = if state.rows.is_full() {
            let focused = state.focused.map(|(id, _)| id);
            let shown = render_tree.visible.map(|visible| self.rows_in(visible));
            let distance = |row: usize| match shown {
                Some((first, _)) if row < first => first - row,
                Some((_, last)) if row >= last => row + 1 - last,
                Some(_) => 0,
                None => row.abs_diff(index),
            };
            let candidates = || {
                state
                    .rows
                    .iter()
                    .enumerate()
                    .filter(|(_, (id, ..))| Some(*id) != focused)
            };
            let victim = candidates()
                .filter(|(_, (_, row, ..))| distance(*row) > 0)
                .max_by_key(|(_, (_, row, ..))| distance(*row))
                .or_else(|| candidates().max_by_key(|(_, (_, row, ..))| row.abs_diff(index)))
                .map(|(slot, _)| slot)?;
            Some(victim)
        } else {
            None
        };
        let view = (self.build_view)(index);
        let mut row_state = view.build_state(captures);
        let environment = DefaultEnvironment::new(context.app_time);
        let env = RowEnvironment {
            inner: &environment,
        };
        let width = render_tree.width;
        let layout = view.layout(&self.row_offer(width), &env, captures, &mut row_state);
        let at = self.row_origin(
            render_tree.origin,
            width,
            index,
            layout.resolved_size.width.into(),
        );
        let tree = view.render_tree(&layout.sublayouts, at, &env, captures, &mut row_state);
        Some(Loose {
            index,
            state: row_state,
            tree,
            placed: (),
            victim,
        })
    }

    /// Adds a row built on the side to the built rows. Returns whether there
    /// was room.
    fn keep<S, R>(
        &self,
        row: Loose<S, R, ()>,
        render_tree: &mut LazyRows<R, N>,
        state: &mut LazyForEachState<S, N, Extent>,
    ) -> bool {
        if let Some(victim) = row.victim {
            state.rows.remove(victim);
            render_tree.rows.remove(victim);
        }
        let slot = state
            .rows
            .iter()
            .position(|(_, index, ..)| *index > row.index)
            .unwrap_or(state.rows.len());
        let id = self.id_at(row.index);
        state
            .rows
            .insert(slot, (id, row.index, (), row.state))
            .is_ok()
            && render_tree
                .rows
                .insert(slot, (id, row.index, row.tree))
                .is_ok()
    }

    /// Where the built row in `slot` is centered along the list.
    fn center<S, R>(
        &self,
        state: &LazyForEachState<S, N, Extent>,
        render_tree: &LazyRows<R, N>,
        slot: usize,
    ) -> i32 {
        render_tree.origin.y + self.offset_of(state.rows[slot].1) + to_i32(self.extent() / 2)
    }
}

impl<const N: usize, V, F, K, Captures: ?Sized> ViewLayout<Captures>
    for LazyForEachView<N, V, F, K, Extent>
where
    V: ViewLayout<Captures>,
    V::FocusTree: DefaultFocus,
    F: Fn(usize) -> V,
    K: RowKeys,
{
    type Sublayout = Dimensions;
    type State = LazyForEachState<V::State, N, Extent>;
    type FocusTree = Focus<V::FocusTree>;

    fn transition(&self) -> Self::Transition {
        Opacity
    }

    fn build_state(&self, _captures: &mut Captures) -> Self::State {
        Self::empty_state()
    }

    fn layout(
        &self,
        offer: &ProposedDimensions,
        env: &impl LayoutEnvironment,
        captures: &mut Captures,
        _state: &mut Self::State,
    ) -> ResolvedLayout<Self::Sublayout> {
        self.layout_rows(offer, env, captures)
    }

    fn render_tree(
        &self,
        layout: &Self::Sublayout,
        origin: Point,
        env: &impl LayoutEnvironment,
        captures: &mut Captures,
        state: &mut Self::State,
    ) -> Self::Renderables {
        self.build_rows(*layout, origin, env, captures, state)
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
        self.handle(
            event,
            context,
            render_tree,
            captures,
            state,
            focus,
            Self::loose,
            Self::keep,
            Self::center,
        )
    }

    fn seek(
        &self,
        seek: &Seek,
        render_tree: &Self::Renderables,
        state: &Self::State,
        focus: Option<&Self::FocusTree>,
    ) -> SeekResult<Self::FocusTree> {
        self.seek_rows::<Captures>(seek, render_tree, state, focus)
    }

    fn handle_touch(
        &self,
        touch: &Touch,
        context: &EventContext,
        render_tree: &mut Self::Renderables,
        captures: &mut Captures,
        state: &mut Self::State,
    ) -> TouchResult<Self::FocusTree> {
        self.touch_rows(touch, context, render_tree, captures, state)
    }
}

// ─── Rows of natural height ──────────────────────────────────────────────

/// The heights a list of natural rows knows, by index, and the estimate for
/// the rest: where its rows are, as far as it knows.
struct Model<const N: usize, const M: usize> {
    count: usize,
    estimate: i32,
    spacing: i32,
    /// Built rows' and remembered heights, each in the order of indices.
    built: heapless::Vec<(usize, u32), N>,
    remembered: heapless::Vec<(usize, u32), M>,
}

impl<const N: usize, const M: usize> Model<N, M> {
    fn height(&self, index: usize) -> i32 {
        let known = |rows: &[(usize, u32)]| {
            rows.binary_search_by_key(&index, |&(row, _)| row)
                .ok()
                .map(|slot| rows[slot].1)
        };
        known(&self.built)
            .or_else(|| known(&self.remembered))
            .map_or(self.estimate, to_i32)
    }

    /// Where row `index` starts, from the first row's top.
    fn offset_of(&self, index: usize) -> i32 {
        let pitch = self.estimate.saturating_add(self.spacing);
        let measured = self
            .built
            .iter()
            .chain(&self.remembered)
            .filter(|&&(row, _)| row < index)
            .fold(0_i32, |sum, &(_, height)| {
                sum.saturating_add(to_i32(height) - self.estimate)
            });
        to_i32(index).saturating_mul(pitch).saturating_add(measured)
    }

    /// From the top of row `from` to the top of row `to`, below it.
    fn between(&self, from: usize, to: usize) -> i32 {
        self.offset_of(to) - self.offset_of(from)
    }

    fn edge(&self, index: usize, edge: Edge) -> i32 {
        let top = self.offset_of(index);
        match edge {
            Edge::Top => top,
            Edge::Bottom => top.saturating_add(self.height(index)),
        }
    }

    fn total(&self) -> i32 {
        if self.count == 0 {
            0
        } else {
            self.offset_of(self.count) - self.spacing
        }
    }

    /// The row at `y`: the last that starts at or above it.
    fn index_at(&self, y: i32) -> usize {
        let (mut low, mut high) = (0, self.count);
        while high - low > 1 {
            let middle = low + (high - low) / 2;
            if self.offset_of(middle) <= y {
                low = middle;
            } else {
                high = middle;
            }
        }
        low
    }
}

/// Where a row is placed: its top, or its bottom.
#[derive(Clone, Copy)]
enum At {
    Top(i32),
    Bottom(i32),
}

impl<const N: usize, const M: usize, V, F, K> LazyForEachView<N, V, F, K, Natural<M>>
where
    F: Fn(usize) -> V,
    K: RowKeys,
{
    const fn estimate(&self) -> u32 {
        self.extent.estimate
    }

    fn natural_offer(width: u32) -> ProposedDimensions {
        ProposedDimensions {
            width: ProposedDimension::Exact(width),
            height: ProposedDimension::Compact,
        }
    }

    /// Where row `id`, last at `last`, is now. Rows added or removed above
    /// move every row after them alike, so a key not where it was is first
    /// looked for as far from it as the last row looked for had moved.
    fn found_again(&self, id: RowId, last: usize, moved: &mut isize) -> Option<usize> {
        let RowId::Key(key) = id else {
            return self.find(id, last);
        };
        if self.key_at(last) == Some(key) {
            return Some(last);
        }
        if let Some(guess) = last.checked_add_signed(*moved)
            && self.key_at(guess) == Some(key)
        {
            return Some(guess);
        }
        let index = (0..self.count).find(|&index| self.key_at(index) == Some(key))?;
        *moved = isize::try_from(index).unwrap_or(isize::MAX)
            - isize::try_from(last).unwrap_or(isize::MAX);
        Some(index)
    }

    /// Where each built row and each remembered height is now: `None` for
    /// rows gone.
    fn where_now<S>(
        &self,
        state: &LazyForEachState<S, N, Natural<M>>,
    ) -> (
        heapless::Vec<Option<usize>, N>,
        heapless::Vec<Option<usize>, M>,
    ) {
        let mut moved = 0;
        let built = state
            .rows
            .iter()
            .map(|(id, last, ..)| self.found_again(*id, *last, &mut moved))
            .collect();
        let remembered = state
            .memory
            .remembered
            .iter()
            .map(|(id, last, _)| self.found_again(*id, *last, &mut moved))
            .collect();
        (built, remembered)
    }

    /// What the list knows of its rows' heights, with its rows where `now`
    /// says they are.
    fn model<S>(
        &self,
        state: &LazyForEachState<S, N, Natural<M>>,
        now: &(
            heapless::Vec<Option<usize>, N>,
            heapless::Vec<Option<usize>, M>,
        ),
    ) -> Model<N, M> {
        let mut built: heapless::Vec<(usize, u32), N> = state
            .rows
            .iter()
            .zip(&now.0)
            .filter_map(|((.., placed, _), index)| Some(((*index)?, placed.height)))
            .collect();
        built.sort_unstable_by_key(|&(index, _)| index);
        let mut remembered: heapless::Vec<(usize, u32), M> = state
            .memory
            .remembered
            .iter()
            .zip(&now.1)
            .filter_map(|((.., height), index)| Some(((*index)?, *height)))
            .collect();
        remembered.sort_unstable_by_key(|&(index, _)| index);
        Model {
            count: self.count,
            estimate: to_i32(self.estimate()),
            spacing: to_i32(self.spacing),
            built,
            remembered,
        }
    }

    /// How far what the list shows moved since it was built: where its
    /// anchor row's edge is now, against where it was drawn.
    fn moved<S>(
        state: &LazyForEachState<S, N, Natural<M>>,
        model: &Model<N, M>,
        now: &[Option<usize>],
    ) -> i32 {
        let Some(anchor) = state.memory.anchor else {
            return 0;
        };
        let alive = || {
            state
                .rows
                .iter()
                .zip(now)
                .enumerate()
                .filter_map(|(slot, (row, index))| Some((slot, row, (*index)?)))
        };
        // The anchor row, or failing it, the row built nearest where it was,
        // below it before above.
        let found = alive()
            .find(|(_, (id, ..), _)| *id == anchor.id)
            .map(|(_, (.., placed, _), index)| (*placed, index, anchor.edge))
            .or_else(|| {
                alive()
                    .min_by_key(|(_, (_, last, ..), _)| {
                        (last.abs_diff(anchor.index), *last < anchor.index)
                    })
                    .map(|(_, (.., placed, _), index)| (*placed, index, Edge::Top))
            });
        let Some((placed, index, edge)) = found else {
            return 0;
        };
        let drawn = match edge {
            Edge::Top => placed.top,
            Edge::Bottom => placed.bottom(),
        };
        model.edge(index, edge) - drawn
    }

    fn layout_rows<Captures: ?Sized>(
        &self,
        offer: &ProposedDimensions,
        env: &impl LayoutEnvironment,
        captures: &mut Captures,
        state: &LazyForEachState<V::State, N, Natural<M>>,
    ) -> ResolvedLayout<Dimensions>
    where
        V: ViewLayout<Captures>,
    {
        let now = self.where_now(state);
        let model = self.model(state, &now);
        // Laid out again, the list reports the same move: it moves nothing
        // until it is built.
        if let Some(drift) = env.drift() {
            drift.report(
                LayoutDirection::Vertical,
                Self::moved(state, &model, &now.0),
            );
        }
        let size = Dimensions {
            width: self
                .width(offer, ProposedDimension::Compact, env, captures)
                .into(),
            height: u32::try_from(model.total()).unwrap_or(0).into(),
        };
        ResolvedLayout {
            sublayouts: size,
            resolved_size: size,
        }
    }

    /// Lays out and builds row `index`, placed `at`, into a slot of `state`:
    /// its own when it is built, else a free one, else that of the row not
    /// yet placed this build, nor kept, farthest from `near`, whose height
    /// is remembered. Returns where it was placed, or `None` with no slot.
    #[expect(clippy::too_many_arguments)]
    fn place<Captures: ?Sized>(
        &self,
        index: usize,
        at: At,
        width: u32,
        origin: Point,
        env: &impl LayoutEnvironment,
        captures: &mut Captures,
        state: &mut LazyForEachState<V::State, N, Natural<M>>,
        tree: &mut LazyRows<V::Renderables, N>,
        placed: &mut [bool; N],
        kept: [Option<usize>; 2],
        near: usize,
    ) -> Option<Placed>
    where
        V: ViewLayout<Captures>,
    {
        let id = self.id_at(index);
        let view = (self.build_view)(index);
        let slot = if let Some(slot) = state.rows.iter().position(|(row, ..)| *row == id) {
            if placed[slot] {
                return Some(state.rows[slot].2);
            }
            slot
        } else if state.rows.is_full() {
            let victim = (0..state.rows.len())
                .filter(|&slot| !placed[slot] && !kept.contains(&Some(state.rows[slot].1)))
                .max_by_key(|&slot| state.rows[slot].1.abs_diff(near))?;
            let (gone, was, height) = (
                state.rows[victim].0,
                state.rows[victim].1,
                state.rows[victim].2.height,
            );
            state.memory.remember(gone, was, height, near);
            state.rows[victim] = (id, index, Placed::default(), view.build_state(captures));
            victim
        } else {
            let row_state = view.build_state(captures);
            state
                .rows
                .push((id, index, Placed::default(), row_state))
                .ok()?;
            state.rows.len() - 1
        };
        let row_state = &mut state.rows[slot].3;
        let layout = view.layout(&Self::natural_offer(width), env, captures, row_state);
        let height: u32 = layout.resolved_size.height.into();
        let top = match at {
            At::Top(top) => top,
            At::Bottom(bottom) => bottom.saturating_sub(to_i32(height)),
        };
        let x = self.row_x(width, layout.resolved_size.width.into());
        let row = view.render_tree(
            &layout.sublayouts,
            origin + Point::new(x, top),
            env,
            captures,
            row_state,
        );
        tree.rows.push((id, index, row)).ok()?;
        let where_placed = Placed { top, height };
        state.rows[slot].1 = index;
        state.rows[slot].2 = where_placed;
        placed[slot] = true;
        Some(where_placed)
    }

    #[expect(clippy::too_many_lines)]
    fn build_rows<Captures: ?Sized>(
        &self,
        layout: Dimensions,
        origin: Point,
        env: &impl LayoutEnvironment,
        captures: &mut Captures,
        state: &mut LazyForEachState<V::State, N, Natural<M>>,
    ) -> LazyRows<V::Renderables, N>
    where
        V: ViewLayout<Captures>,
    {
        let width: u32 = layout.width.into();
        let height = to_i32(u32::from(layout.height));
        let spacing = to_i32(self.spacing);
        let viewport = env.viewport();
        let visible = viewport.map(|viewport| {
            viewport
                .visible_along(LayoutDirection::Vertical)
                .shifted(-origin.y)
        });
        let mut tree = LazyRows::new(origin, width, visible);

        // The rows found again where they are now, and moved, with what they
        // show, by what the list reported as it was laid out.
        let now = self.where_now(state);
        let model = self.model(state, &now);
        let moved = Self::moved(state, &model, &now.0);
        if let Some(drift) = env.drift() {
            drift.confirm(LayoutDirection::Vertical, moved);
        }
        let mut found = now.0.iter();
        state.rows.retain_mut(
            |(_, index, placed, _)| match found.next().copied().flatten() {
                Some(row) => {
                    *index = row;
                    placed.top = placed.top.saturating_add(moved);
                    true
                }
                None => false,
            },
        );
        let mut found = now.1.iter();
        state.memory.remembered.retain_mut(|(_, index, _)| {
            found.next().copied().flatten().is_some_and(|row| {
                *index = row;
                true
            })
        });
        state.rows.sort_unstable_by_key(|(_, index, ..)| *index);
        state
            .memory
            .remembered
            .sort_unstable_by_key(|&(_, index, _)| index);
        tree.rebased = moved != 0;
        // Heights hold at the width they were measured at.
        if state.memory.width != Some(width) {
            state.memory.remembered.clear();
            state.memory.width = Some(width);
        }

        let kept = self.kept(state);
        if self.count == 0 {
            state.rows.clear();
            state.memory.anchor = None;
            if let Some(viewport) = viewport {
                viewport.vouch(LayoutDirection::Vertical, Span::UNBOUNDED, Span::UNBOUNDED);
            }
            return tree;
        }
        let last_row = self.count - 1;
        let total = model.total();
        let slot_of = |state: &LazyForEachState<V::State, N, Natural<M>>, index: usize| {
            state.rows.iter().position(|(_, row, ..)| *row == index)
        };

        // The row that stays where it was drawn: at the end, the last row's
        // bottom; else the topmost row shown that was drawn; else, among
        // rows never drawn, the row at the top of what is shown.
        let shown = visible.unwrap_or(Span::new(0, 0));
        let at_end = shown.end >= total
            && (viewport.is_some_and(|viewport| viewport.end_gravity(LayoutDirection::Vertical))
                || shown.start > 0);
        let drawn_last = slot_of(state, last_row).map(|slot| state.rows[slot].2);
        let drawn_shown = state
            .rows
            .iter()
            .filter(|(.., placed, _)| placed.top < shown.end && placed.bottom() > shown.start)
            .min_by_key(|(_, index, ..)| *index)
            .map(|(_, index, placed, _)| (*index, *placed));
        let (anchor, edge, at) = match (at_end, drawn_last, drawn_shown) {
            (true, Some(placed), _) => (last_row, Edge::Bottom, At::Bottom(placed.bottom())),
            (_, _, Some((index, placed))) if visible.is_some() => {
                (index, Edge::Top, At::Top(placed.top))
            }
            (true, None, _) => (last_row, Edge::Bottom, At::Bottom(total)),
            _ if visible.is_none() => (0, Edge::Top, At::Top(0)),
            _ => {
                let index = model.index_at(shown.start.max(0));
                (index, Edge::Top, At::Top(model.offset_of(index)))
            }
        };

        // The rows shown, from the anchor down and then up; then those beyond,
        // alternately below and above; and the kept rows where they are.
        let reserved = kept
            .iter()
            .flatten()
            .filter(|&&index| index != anchor)
            .count()
            .min(1 + usize::from(kept[0] != kept[1]));
        let limit = N.saturating_sub(reserved).max(1);
        let mut placed = [false; N];
        let count_placed = |placed: &[bool; N]| placed.iter().filter(|&&p| p).count();
        let env_rows = RowEnvironment { inner: env };
        let place = |index: usize,
                     at: At,
                     state: &mut LazyForEachState<V::State, N, Natural<M>>,
                     tree: &mut LazyRows<V::Renderables, N>,
                     placed: &mut [bool; N],
                     captures: &mut Captures| {
            self.place(
                index, at, width, origin, &env_rows, captures, state, tree, placed, kept, anchor,
            )
        };
        let Some(first_placed) = place(anchor, at, state, &mut tree, &mut placed, captures) else {
            return tree;
        };
        let (mut first, mut last) = (anchor, anchor);
        let (mut top, mut bottom) = (first_placed.top, first_placed.bottom());
        let mut short = false;
        if visible.is_some() {
            while last < last_row && bottom.saturating_add(spacing) < shown.end {
                let row = if count_placed(&placed) < limit {
                    place(
                        last + 1,
                        At::Top(bottom.saturating_add(spacing)),
                        state,
                        &mut tree,
                        &mut placed,
                        captures,
                    )
                } else {
                    None
                };
                let Some(row) = row else {
                    short = true;
                    break;
                };
                last += 1;
                bottom = row.bottom();
            }
            while first > 0 && top.saturating_sub(spacing) > shown.start {
                let row = if count_placed(&placed) < limit {
                    place(
                        first - 1,
                        At::Bottom(top.saturating_sub(spacing)),
                        state,
                        &mut tree,
                        &mut placed,
                        captures,
                    )
                } else {
                    None
                };
                let Some(row) = row else {
                    short = true;
                    break;
                };
                first -= 1;
                top = row.top;
            }
        }
        debug_assert!(
            !short,
            "a LazyForEach of {N} rows cannot build the rows it shows"
        );
        // The kept rows, where the rows between would put them.
        for index in kept.into_iter().flatten() {
            if (first..=last).contains(&index) {
                continue;
            }
            let at = if index > last {
                At::Top(
                    bottom
                        .saturating_add(spacing)
                        .saturating_add(model.between(last + 1, index)),
                )
            } else {
                At::Bottom(
                    top.saturating_sub(spacing)
                        .saturating_sub(model.between(index + 1, first)),
                )
            };
            _ = place(index, at, state, &mut tree, &mut placed, captures);
        }
        // Beyond the rows shown, nearest first.
        while count_placed(&placed) < N && (last < last_row || first > 0) {
            let below = if last < last_row {
                place(
                    last + 1,
                    At::Top(bottom.saturating_add(spacing)),
                    state,
                    &mut tree,
                    &mut placed,
                    captures,
                )
            } else {
                None
            };
            if let Some(row) = below {
                last += 1;
                bottom = row.bottom();
            }
            let above = if count_placed(&placed) < N && first > 0 {
                place(
                    first - 1,
                    At::Bottom(top.saturating_sub(spacing)),
                    state,
                    &mut tree,
                    &mut placed,
                    captures,
                )
            } else {
                None
            };
            if let Some(row) = above {
                first -= 1;
                top = row.top;
            }
            if below.is_none() && above.is_none() {
                break;
            }
        }

        // Rows not placed are dropped, their heights remembered; rows placed
        // are not remembered.
        for slot in (0..state.rows.len()).rev() {
            let (id, index, Placed { height, .. }, _) = state.rows[slot];
            if placed[slot] {
                state.memory.forget(id);
            } else {
                state.memory.remember(id, index, height, anchor);
                state.rows.remove(slot);
            }
        }
        state.rows.sort_unstable_by_key(|(_, index, ..)| *index);
        tree.rows.sort_unstable_by_key(|(_, index, _)| *index);
        state.memory.anchor = Some(Anchor {
            id: self.id_at(anchor),
            index: anchor,
            edge,
        });
        // Rows that measured other than the list was laid out for move it:
        // the next frame lays it out again.
        tree.drifted = state
            .rows
            .iter()
            .any(|(_, index, placed, _)| to_i32(placed.height) != model.height(*index));

        for (_, index, placed, _) in &state.rows {
            if to_i32(placed.height) != model.height(*index) {}
        }
        if let Some(viewport) = viewport {
            let (realized, safe) = if short {
                (Span::UNBOUNDED, Span::UNBOUNDED)
            } else {
                Self::coverage(state, shown, first, last, last_row, height, spacing)
            };
            viewport.vouch(
                LayoutDirection::Vertical,
                realized.shifted(origin.y),
                safe.shifted(origin.y),
            );
        }
        tree
    }

    /// What the rows built from `first` to `last`, a run, vouch for around
    /// `shown`: unbounded at an end of the list only where its row meets the
    /// content's edge, so that a frame showing a gap there is rebuilt.
    fn coverage<S>(
        state: &LazyForEachState<S, N, Natural<M>>,
        shown: Span,
        first: usize,
        last: usize,
        last_row: usize,
        height: i32,
        spacing: i32,
    ) -> (Span, Span) {
        let placed = |index: usize| {
            state
                .rows
                .iter()
                .find(|(_, row, ..)| *row == index)
                .map(|(.., placed, _)| *placed)
                .unwrap_or_default()
        };
        let first_shown = (first..=last)
            .find(|&index| placed(index).bottom() > shown.start)
            .unwrap_or(first);
        let last_shown = (first..=last)
            .rev()
            .find(|&index| placed(index).top < shown.end)
            .unwrap_or(last);
        let above = (first_shown - first).div_ceil(2);
        let below = (last - last_shown).div_ceil(2);
        let top = placed(first).top;
        let bottom = placed(last).bottom();
        let unbounded_above = first == 0 && top == 0;
        let unbounded_below = last == last_row && bottom == height;
        let start = |index: usize| {
            if index == 0 {
                placed(0).top
            } else {
                placed(index).top.saturating_sub(spacing)
            }
        };
        let end = |index: usize| {
            if index == last_row {
                placed(index).bottom()
            } else {
                placed(index).bottom().saturating_add(spacing)
            }
        };
        let realized = Span::new(
            if unbounded_above {
                i32::MIN
            } else {
                start(first)
            },
            if unbounded_below { i32::MAX } else { end(last) },
        );
        let safe = Span::new(
            if unbounded_above {
                i32::MIN
            } else {
                start(first + above)
            },
            if unbounded_below {
                i32::MAX
            } else {
                end(last - below)
            },
        );
        (realized, safe)
    }

    /// Builds row `index` on the side while handling an event, between
    /// rebuilds, next to the built row nearest it, to take the place of the
    /// built row farthest from the view that is not focused. `None` when
    /// there is no room.
    fn loose<Captures: ?Sized>(
        &self,
        index: usize,
        context: &EventContext,
        render_tree: &LazyRows<V::Renderables, N>,
        captures: &mut Captures,
        state: &LazyForEachState<V::State, N, Natural<M>>,
    ) -> Option<Loose<V::State, V::Renderables, Placed>>
    where
        V: ViewLayout<Captures>,
    {
        let victim = if state.rows.is_full() {
            let focused = state.focused.map(|(id, _)| id);
            let shown = render_tree.visible;
            let distance = |placed: Placed| match shown {
                Some(shown) if placed.bottom() <= shown.start => {
                    i64::from(shown.start) - i64::from(placed.bottom())
                }
                Some(shown) if placed.top >= shown.end => {
                    i64::from(placed.top) - i64::from(shown.end)
                }
                _ => 0,
            };
            let victim = (0..state.rows.len())
                .filter(|&slot| Some(state.rows[slot].0) != focused)
                .max_by_key(|&slot| {
                    (
                        distance(state.rows[slot].2),
                        state.rows[slot].1.abs_diff(index),
                    )
                })?;
            Some(victim)
        } else {
            None
        };
        // Placed against the built row nearest it that stays, with the rows
        // between where the list takes them to be.
        let identity = (
            state.rows.iter().map(|(_, row, ..)| Some(*row)).collect(),
            state
                .memory
                .remembered
                .iter()
                .map(|(_, row, _)| Some(*row))
                .collect(),
        );
        let model = self.model(state, &identity);
        let nearest = state
            .rows
            .iter()
            .enumerate()
            .filter(|&(slot, _)| Some(slot) != victim)
            .min_by_key(|(_, (_, row, ..))| row.abs_diff(index))
            .map(|(_, (_, row, placed, _))| (*row, *placed));
        let view = (self.build_view)(index);
        let mut row_state = view.build_state(captures);
        let environment = DefaultEnvironment::new(context.app_time);
        let env = RowEnvironment {
            inner: &environment,
        };
        let width = render_tree.width;
        let layout = view.layout(&Self::natural_offer(width), &env, captures, &mut row_state);
        let height: u32 = layout.resolved_size.height.into();
        let spacing = to_i32(self.spacing);
        let top = match nearest {
            Some((row, placed)) if row < index => placed
                .bottom()
                .saturating_add(spacing)
                .saturating_add(model.between(row + 1, index)),
            Some((row, placed)) => placed
                .top
                .saturating_sub(spacing)
                .saturating_sub(model.between(index + 1, row))
                .saturating_sub(to_i32(height)),
            None => model.offset_of(index),
        };
        let x = self.row_x(width, layout.resolved_size.width.into());
        let tree = view.render_tree(
            &layout.sublayouts,
            render_tree.origin + Point::new(x, top),
            &env,
            captures,
            &mut row_state,
        );
        Some(Loose {
            index,
            state: row_state,
            tree,
            placed: Placed { top, height },
            victim,
        })
    }

    /// Adds a row built on the side to the built rows, remembering the
    /// height of the row it takes the place of. Returns whether there was
    /// room.
    fn keep<S, R>(
        &self,
        row: Loose<S, R, Placed>,
        render_tree: &mut LazyRows<R, N>,
        state: &mut LazyForEachState<S, N, Natural<M>>,
    ) -> bool {
        if let Some(victim) = row.victim {
            let (id, was, Placed { height, .. }, _) = state.rows[victim];
            state.memory.remember(id, was, height, row.index);
            state.rows.remove(victim);
            render_tree.rows.remove(victim);
        }
        let slot = state
            .rows
            .iter()
            .position(|(_, index, ..)| *index > row.index)
            .unwrap_or(state.rows.len());
        let id = self.id_at(row.index);
        state.memory.forget(id);
        state
            .rows
            .insert(slot, (id, row.index, row.placed, row.state))
            .is_ok()
            && render_tree
                .rows
                .insert(slot, (id, row.index, row.tree))
                .is_ok()
    }

    /// Where the built row in `slot` is centered along the list.
    #[expect(clippy::unused_self, reason = "the signature `handle` takes")]
    fn center<S, R>(
        &self,
        state: &LazyForEachState<S, N, Natural<M>>,
        render_tree: &LazyRows<R, N>,
        slot: usize,
    ) -> i32 {
        let placed = state.rows[slot].2;
        render_tree.origin.y + placed.top + to_i32(placed.height / 2)
    }
}

impl<const N: usize, const M: usize, V, F, K, Captures: ?Sized> ViewLayout<Captures>
    for LazyForEachView<N, V, F, K, Natural<M>>
where
    V: ViewLayout<Captures>,
    V::FocusTree: DefaultFocus,
    F: Fn(usize) -> V,
    K: RowKeys,
{
    type Sublayout = Dimensions;
    type State = LazyForEachState<V::State, N, Natural<M>>;
    type FocusTree = Focus<V::FocusTree>;

    fn transition(&self) -> Self::Transition {
        Opacity
    }

    fn build_state(&self, _captures: &mut Captures) -> Self::State {
        Self::empty_state()
    }

    fn layout(
        &self,
        offer: &ProposedDimensions,
        env: &impl LayoutEnvironment,
        captures: &mut Captures,
        state: &mut Self::State,
    ) -> ResolvedLayout<Self::Sublayout> {
        self.layout_rows(offer, env, captures, state)
    }

    fn render_tree(
        &self,
        layout: &Self::Sublayout,
        origin: Point,
        env: &impl LayoutEnvironment,
        captures: &mut Captures,
        state: &mut Self::State,
    ) -> Self::Renderables {
        self.build_rows(*layout, origin, env, captures, state)
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
        self.handle(
            event,
            context,
            render_tree,
            captures,
            state,
            focus,
            Self::loose,
            Self::keep,
            Self::center,
        )
    }

    fn seek(
        &self,
        seek: &Seek,
        render_tree: &Self::Renderables,
        state: &Self::State,
        focus: Option<&Self::FocusTree>,
    ) -> SeekResult<Self::FocusTree> {
        self.seek_rows::<Captures>(seek, render_tree, state, focus)
    }

    fn handle_touch(
        &self,
        touch: &Touch,
        context: &EventContext,
        render_tree: &mut Self::Renderables,
        captures: &mut Captures,
        state: &mut Self::State,
    ) -> TouchResult<Self::FocusTree> {
        self.touch_rows(touch, context, render_tree, captures, state)
    }
}
