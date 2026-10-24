use core::{cell::Cell, time::Duration};

use crate::layout::LayoutDirection;
use crate::primitives::{Point, geometry::Rectangle};

pub trait LayoutEnvironment {
    fn layout_direction(&self) -> LayoutDirection;
    /// The duration since the application started.
    /// This is used to drive animations.
    fn app_time(&self) -> Duration;
    /// What the scroll view laying out this content shows of it, in the
    /// coordinates of the origin the content is built at. `None` outside a
    /// scroll view, and wherever the content is drawn transformed.
    fn viewport(&self) -> Option<&Viewport> {
        None
    }
    /// Where content whose rows are placed by their measured heights tells
    /// the scroll view laying it out how far what it shows moved, in the
    /// content's coordinates. `None` outside a scroll view, and wherever
    /// content is laid out but may not be drawn.
    fn drift(&self) -> Option<&Drift> {
        None
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultEnvironment {
    pub app_time: Duration,
}

impl DefaultEnvironment {
    #[must_use]
    pub const fn new(app_time: Duration) -> Self {
        Self { app_time }
    }

    #[must_use]
    pub fn non_animated() -> Self {
        Self {
            app_time: Duration::default(),
        }
    }
}

impl LayoutEnvironment for DefaultEnvironment {
    fn layout_direction(&self) -> LayoutDirection {
        LayoutDirection::default()
    }

    fn app_time(&self) -> Duration {
        self.app_time
    }
}

/// A stretch along one axis, from `start` up to, not including, `end`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: i32,
    pub end: i32,
}

impl Span {
    /// The whole axis: nothing is out of it.
    pub const UNBOUNDED: Self = Self {
        start: i32::MIN,
        end: i32::MAX,
    };

    #[must_use]
    pub const fn new(start: i32, end: i32) -> Self {
        Self { start, end }
    }

    /// What lies in both.
    #[must_use]
    pub fn intersection(self, other: Self) -> Self {
        Self {
            start: self.start.max(other.start),
            end: self.end.min(other.end),
        }
    }

    /// Whether `other` lies wholly within this.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.start <= other.start && other.end <= self.end
    }

    /// Moved by `by`. An end at the edge of the axis stays there.
    #[must_use]
    pub const fn shifted(self, by: i32) -> Self {
        Self {
            start: if self.start == i32::MIN {
                i32::MIN
            } else {
                self.start.saturating_add(by)
            },
            end: if self.end == i32::MAX {
                i32::MAX
            } else {
                self.end.saturating_add(by)
            },
        }
    }
}

/// How far content placed by its rows' measured heights moved since it was
/// last built: rows measured, changed, added, or removed above what it
/// shows move what it shows within the content.
///
/// A scroll view publishes one while it lays out and builds its content.
/// The content reports the move as it is laid out, and the scroll view
/// moves its offset with it, so what is shown stays where it was. The
/// content confirms the move as it is built: content laid out but not built
/// confirms nothing, and the scroll view then lays its content out again
/// without the move.
#[derive(Debug, Default)]
pub struct Drift {
    /// The move reported while laying out, along each axis, x then y.
    reported: Cell<[Option<i32>; 2]>,
    /// The move confirmed while building.
    confirmed: Cell<[Option<i32>; 2]>,
    /// Content placed by measured heights was laid out, moved or not.
    measured: Cell<bool>,
}

impl Drift {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            reported: Cell::new([None; 2]),
            confirmed: Cell::new([None; 2]),
            measured: Cell::new(false),
        }
    }

    /// Reports, while laying out, that what is shown moved `by` along
    /// `axis`. Laid out again, content reports the same move again.
    pub fn report(&self, axis: LayoutDirection, by: i32) {
        Self::set(&self.reported, axis, by);
        self.measured.set(true);
    }

    /// Confirms, while building, the move reported along `axis`.
    pub fn confirm(&self, axis: LayoutDirection, by: i32) {
        Self::set(&self.confirmed, axis, by);
    }

    fn set(cell: &Cell<[Option<i32>; 2]>, axis: LayoutDirection, by: i32) {
        let mut moves = cell.get();
        let i = Viewport::index(axis);
        debug_assert!(
            moves[i].is_none_or(|was| was == by),
            "one content placed by measured heights per scroll view"
        );
        moves[i] = Some(by);
        cell.set(moves);
    }

    /// The move reported while laying out.
    #[must_use]
    pub fn reported(&self) -> Point {
        Self::point(self.reported.get())
    }

    /// The move confirmed while building.
    #[must_use]
    pub fn confirmed(&self) -> Point {
        Self::point(self.confirmed.get())
    }

    /// Whether content placed by measured heights was laid out.
    #[must_use]
    pub fn measured(&self) -> bool {
        self.measured.get()
    }

    fn point(moves: [Option<i32>; 2]) -> Point {
        Point::new(moves[0].unwrap_or(0), moves[1].unwrap_or(0))
    }
}

/// What a scroll view shows of its content, and what of the content's lazily
/// built parts is there to show.
///
/// A scroll view publishes one through the [`LayoutEnvironment`] while it
/// builds its content. Lazy content, such as a
/// [`LazyForEach`](crate::view::LazyForEach), builds what is visible and some
/// way beyond, and narrows the spans it vouches for. The scroll view checks
/// them against where its content is drawn before each frame is painted.
#[derive(Debug)]
pub struct Viewport {
    visible: Rectangle,
    /// Along each axis, x then y: wherever the visible part lies within
    /// these, everything it shows is built.
    realized: Cell<[Span; 2]>,
    /// Within `realized`: while the visible part stays within these, nothing
    /// needs to be built.
    safe: Cell<[Span; 2]>,
    /// Along each axis, whether the content holds to its end: pinned there,
    /// or too short to fill the view and placed at its end.
    end_gravity: [bool; 2],
}

impl Viewport {
    /// A viewport showing `visible`, with nothing yet vouched for.
    #[must_use]
    pub const fn new(visible: Rectangle) -> Self {
        Self {
            visible,
            realized: Cell::new([Span::UNBOUNDED; 2]),
            safe: Cell::new([Span::UNBOUNDED; 2]),
            end_gravity: [false; 2],
        }
    }

    /// The same, holding to its end along the axes `gravity` says, x then y.
    #[must_use]
    pub const fn with_end_gravity(mut self, gravity: [bool; 2]) -> Self {
        self.end_gravity = gravity;
        self
    }

    /// Whether the content holds to its end along `axis`.
    #[must_use]
    pub const fn end_gravity(&self, axis: LayoutDirection) -> bool {
        self.end_gravity[Self::index(axis)]
    }

    /// The part shown.
    #[must_use]
    pub const fn visible(&self) -> Rectangle {
        self.visible
    }

    /// The part shown, along `axis`.
    #[must_use]
    pub const fn visible_along(&self, axis: LayoutDirection) -> Span {
        let Rectangle { origin, size } = self.visible;
        match axis {
            LayoutDirection::Horizontal => {
                Span::new(origin.x, origin.x.saturating_add(size.width as i32))
            }
            LayoutDirection::Vertical => {
                Span::new(origin.y, origin.y.saturating_add(size.height as i32))
            }
        }
    }

    /// Narrows what is vouched for along `axis` to `realized` and `safe`.
    pub fn vouch(&self, axis: LayoutDirection, realized: Span, safe: Span) {
        let i = Self::index(axis);
        let mut spans = self.realized.get();
        spans[i] = spans[i].intersection(realized);
        self.realized.set(spans);
        let mut spans = self.safe.get();
        spans[i] = spans[i].intersection(safe.intersection(realized));
        self.safe.set(spans);
    }

    /// Wherever the visible part lies within these, along each axis, all it
    /// shows is built.
    #[must_use]
    pub fn realized(&self) -> [Span; 2] {
        self.realized.get()
    }

    /// While the visible part stays within these, nothing needs building.
    #[must_use]
    pub fn safe(&self) -> [Span; 2] {
        self.safe.get()
    }

    /// The same view, for content built from `origin` rather than from zero.
    #[must_use]
    pub fn rebased(&self, origin: Point) -> Self {
        Self::new(Rectangle::new(
            self.visible.origin - origin,
            self.visible.size,
        ))
        .with_end_gravity(self.end_gravity)
    }

    /// Takes on what was vouched for in `rebased`, a viewport made by
    /// [`rebased`](Self::rebased) for content built from `origin`.
    pub fn absorb(&self, rebased: &Self, origin: Point) {
        let (realized, safe) = (rebased.realized(), rebased.safe());
        for (axis, by) in [
            (LayoutDirection::Horizontal, origin.x),
            (LayoutDirection::Vertical, origin.y),
        ] {
            let i = Self::index(axis);
            self.vouch(axis, realized[i].shifted(by), safe[i].shifted(by));
        }
    }

    const fn index(axis: LayoutDirection) -> usize {
        match axis {
            LayoutDirection::Horizontal => 0,
            LayoutDirection::Vertical => 1,
        }
    }
}

/// Another environment with `viewport` and `drift` in place of its own.
pub(crate) struct ViewportEnvironment<'a, E> {
    inner: &'a E,
    viewport: Option<&'a Viewport>,
    drift: Option<&'a Drift>,
}

impl<'a, E> ViewportEnvironment<'a, E> {
    pub(crate) const fn new(
        inner: &'a E,
        viewport: Option<&'a Viewport>,
        drift: Option<&'a Drift>,
    ) -> Self {
        Self {
            inner,
            viewport,
            drift,
        }
    }
}

impl<'a, E: LayoutEnvironment> ViewportEnvironment<'a, E> {
    /// `inner` as it is, except that content laid out in it reports no
    /// move: it may not be built.
    pub(crate) fn without_drift(inner: &'a E) -> Self {
        Self::new(inner, inner.viewport(), None)
    }
}

impl<E: LayoutEnvironment> LayoutEnvironment for ViewportEnvironment<'_, E> {
    fn layout_direction(&self) -> LayoutDirection {
        self.inner.layout_direction()
    }

    fn app_time(&self) -> Duration {
        self.inner.app_time()
    }

    fn viewport(&self) -> Option<&Viewport> {
        self.viewport
    }

    fn drift(&self) -> Option<&Drift> {
        self.drift
    }
}
