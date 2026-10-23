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
}

impl Viewport {
    /// A viewport showing `visible`, with nothing yet vouched for.
    #[must_use]
    pub const fn new(visible: Rectangle) -> Self {
        Self {
            visible,
            realized: Cell::new([Span::UNBOUNDED; 2]),
            safe: Cell::new([Span::UNBOUNDED; 2]),
        }
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

/// Another environment with `viewport` in place of its own.
pub(crate) struct ViewportEnvironment<'a, E> {
    inner: &'a E,
    viewport: Option<&'a Viewport>,
}

impl<'a, E> ViewportEnvironment<'a, E> {
    pub(crate) const fn new(inner: &'a E, viewport: Option<&'a Viewport>) -> Self {
        Self { inner, viewport }
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
}
