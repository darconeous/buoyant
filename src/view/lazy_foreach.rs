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
/// rows coming into view and drops those far from it. The rows are placed
/// as the [`Extent`] says, so the list knows where every row is without
/// building it.
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
/// while out of view. A list given less, as by a view grown taller, builds
/// the focused row and the shown rows nearest the middle of the view, and
/// leaves those at the edges out; debug builds panic instead.
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
#[expect(missing_debug_implementations)]
pub struct LazyForEach<const N: usize> {}

/// How tall the rows of a [`LazyForEach`] are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Extent {
    /// Every row is this tall. Rows are offered exactly this height.
    Fixed(u32),
}

/// Prefer using [`LazyForEach::new_vertical`] to avoid needing to specify
/// type parameters.
#[derive(Debug, Clone)]
pub struct LazyForEachView<const N: usize, V, F, K = Positional>
where
    F: Fn(usize) -> V,
{
    count: usize,
    extent: Extent,
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
    ) -> LazyForEachView<N, V, F, Positional>
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
}

impl<const N: usize, V, F, K> LazyForEachView<N, V, F, K>
where
    F: Fn(usize) -> V,
{
    /// Tells the rows apart by `key`, rather than by their position.
    ///
    /// Focus and each built row's state then follow the row as rows are
    /// added and removed, and the row can be focused by its key even when it
    /// is not built. Keys must be unique among the rows.
    #[must_use]
    pub fn keyed<K2>(self, key: K2) -> LazyForEachView<N, V, F, Keyed<K2>>
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

impl<const N: usize, V, F, K> ViewMarker for LazyForEachView<N, V, F, K>
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
pub struct LazyForEachState<S, const N: usize> {
    /// Each built row's identity, index, and state, in the order of their
    /// indices, as in the render tree.
    rows: heapless::Vec<(RowId, usize, S), N>,
    /// The row focus was last on, with where it last was.
    focused: Option<(RowId, usize)>,
    /// A row to build at the next build, for a focus request to reach.
    realize: Option<(RowId, usize)>,
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
/// the list is shown through.
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

impl<const N: usize, V, F, K> LazyForEachView<N, V, F, K>
where
    F: Fn(usize) -> V,
    K: RowKeys,
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

    const fn extent(&self) -> u32 {
        match self.extent {
            Extent::Fixed(height) => height,
        }
    }

    /// From one row's top to the next's.
    fn pitch(&self) -> i32 {
        i32::try_from(self.extent().saturating_add(self.spacing))
            .unwrap_or(i32::MAX)
            .max(1)
    }

    /// Where row `index` starts, from the first row's top.
    fn offset_of(&self, index: usize) -> i32 {
        i32::try_from(index)
            .unwrap_or(i32::MAX)
            .saturating_mul(self.pitch())
    }

    /// Where row `index` ends.
    fn end_of(&self, index: usize) -> i32 {
        self.offset_of(index)
            .saturating_add(i32::try_from(self.extent()).unwrap_or(i32::MAX))
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
        let extent = i32::try_from(self.extent()).unwrap_or(i32::MAX);
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
        state: &LazyForEachState<S, N>,
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
            .position(|(row_id, row, _)| *row == index && *row_id == id)?;
        render_tree
            .rows
            .get(slot)
            .is_some_and(|(row_id, row, _)| *row == index && *row_id == id)
            .then_some(slot)
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
        let available = i32::try_from(width).unwrap_or(i32::MAX);
        let content = i32::try_from(row_width).unwrap_or(i32::MAX);
        origin
            + Point::new(
                self.alignment.align(available, content),
                self.offset_of(index),
            )
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
            let middle = i32::try_from((last - first) / 2).unwrap_or(i32::MAX);
            let center = |offset: usize| i32::try_from(offset).unwrap_or(i32::MAX);
            for offset in nearest_first(last - first, center, middle) {
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

    /// Builds row `index` while handling an event, between rebuilds, in
    /// place of the built row farthest from the view that is neither shown
    /// nor focused, or failing one, the row farthest from it that is not
    /// focused. Returns where it is, or `None` when there is no room.
    fn build_row<Captures: ?Sized>(
        &self,
        index: usize,
        context: &EventContext,
        render_tree: &mut LazyRows<V::Renderables, N>,
        captures: &mut Captures,
        state: &mut LazyForEachState<V::State, N>,
    ) -> Option<usize>
    where
        V: ViewLayout<Captures>,
    {
        if state.rows.is_full() {
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
                .filter(|(_, (_, row, _))| distance(*row) > 0)
                .max_by_key(|(_, (_, row, _))| distance(*row))
                .or_else(|| candidates().max_by_key(|(_, (_, row, _))| row.abs_diff(index)))
                .map(|(slot, _)| slot)?;
            state.rows.remove(victim);
            render_tree.rows.remove(victim);
        }
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
        let slot = state
            .rows
            .iter()
            .position(|(_, row, _)| *row > index)
            .unwrap_or(state.rows.len());
        let id = self.id_at(index);
        state.rows.insert(slot, (id, index, row_state)).ok()?;
        render_tree.rows.insert(slot, (id, index, tree)).ok()?;
        // What the list vouched for may no longer hold.
        render_tree.stale = true;
        context.request_view_rebuild();
        Some(slot)
    }

    /// Keeps the row at `index` built when it took focus.
    fn keep_if_focused<S>(
        &self,
        state: &mut LazyForEachState<S, N>,
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
}

impl<const N: usize, V, F, K, Captures: ?Sized> ViewLayout<Captures> for LazyForEachView<N, V, F, K>
where
    V: ViewLayout<Captures>,
    V::FocusTree: DefaultFocus,
    F: Fn(usize) -> V,
    K: RowKeys,
{
    type Sublayout = Dimensions;
    type State = LazyForEachState<V::State, N>;
    type FocusTree = Focus<V::FocusTree>;

    fn transition(&self) -> Self::Transition {
        Opacity
    }

    fn build_state(&self, _captures: &mut Captures) -> Self::State {
        LazyForEachState {
            rows: heapless::Vec::new(),
            focused: None,
            realize: None,
        }
    }

    fn layout(
        &self,
        offer: &ProposedDimensions,
        env: &impl LayoutEnvironment,
        captures: &mut Captures,
        _state: &mut Self::State,
    ) -> ResolvedLayout<Self::Sublayout> {
        // Offered no width, the list is as wide as its first row.
        let width = match offer.width {
            ProposedDimension::Exact(width) => width,
            ProposedDimension::Compact | ProposedDimension::Infinite if self.count > 0 => {
                let view = (self.build_view)(0);
                let mut row_state = view.build_state(captures);
                let offer = ProposedDimensions {
                    width: offer.width,
                    height: ProposedDimension::Exact(self.extent()),
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
        };
        let size = Dimensions {
            width: width.into(),
            height: self.total().into(),
        };
        ResolvedLayout {
            sublayouts: size,
            resolved_size: size,
        }
    }

    fn render_tree(
        &self,
        layout: &Self::Sublayout,
        origin: Point,
        env: &impl LayoutEnvironment,
        captures: &mut Captures,
        state: &mut Self::State,
    ) -> Self::Renderables {
        let width: u32 = layout.width.into();
        let viewport = env.viewport();
        let visible = viewport.map(|viewport| {
            viewport
                .visible_along(LayoutDirection::Vertical)
                .shifted(-origin.y)
        });
        // The focused row, and one a focus request is to reach, wherever
        // they are now.
        let found = |kept: Option<(RowId, usize)>| {
            kept.and_then(|(id, last)| self.find(id, last).map(|index| (self.id_at(index), index)))
        };
        state.focused = found(state.focused);
        let realize = found(state.realize.take());
        let chosen = self.choose(
            visible,
            [
                state.focused.map(|(_, index)| index),
                realize.map(|(_, index)| index),
            ],
        );

        // Each row keeps its state while it stays built.
        state.rows.retain_mut(|(id, index, _)| {
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
                _ = state.rows.push((id, index, row_state));
            }
        }
        state.rows.sort_unstable_by_key(|(_, index, _)| *index);

        let env = RowEnvironment { inner: env };
        let mut tree = LazyRows::new(origin, width, visible);
        for (id, index, row_state) in &mut state.rows {
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

    #[expect(clippy::too_many_lines)]
    fn handle_event(
        &self,
        event: &Event,
        context: &EventContext,
        render_tree: &mut Self::Renderables,
        captures: &mut Captures,
        state: &mut Self::State,
        focus: &mut Self::FocusTree,
    ) -> EventResult {
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
                    &mut state.rows[slot].2,
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
                let half = i32::try_from(self.extent() / 2).unwrap_or(i32::MAX);
                let centers: heapless::Vec<i32, N> = state
                    .rows
                    .iter()
                    .map(|(_, index, _)| render_tree.origin.y + self.offset_of(*index) + half)
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
                        &mut state.rows[slot].2,
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
                // A row not built is built now, as moving through the list
                // reaches it.
                let slot = match self.slot(state, render_tree, current) {
                    Some(slot) => slot,
                    None => match self.build_row(current, context, render_tree, captures, state) {
                        Some(slot) => slot,
                        None => return EventResult::Deferred,
                    },
                };
                let result = (self.build_view)(current).handle_event(
                    &focus_event_for(current_event),
                    context,
                    &mut render_tree.rows[slot].2,
                    captures,
                    &mut state.rows[slot].2,
                    &mut focus.tree,
                );
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
                &mut state.rows[slot].2,
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
                &mut state.rows[slot].2,
                &mut focus.tree,
            );
            if result.is_handled() {
                return result;
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
        let on_path = focus.and_then(|focus| Some((self.focused_index(focus)?, &focus.tree)));
        let mut result = SeekResult::Absent;
        for (slot, (_, index, row_state)) in state.rows.iter().enumerate() {
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

    fn handle_touch(
        &self,
        touch: &Touch,
        context: &EventContext,
        render_tree: &mut Self::Renderables,
        captures: &mut Captures,
        state: &mut Self::State,
    ) -> TouchResult<Self::FocusTree> {
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
                &mut state.rows[slot].2,
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
