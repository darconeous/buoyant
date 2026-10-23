use core::time::Duration;

use crate::{
    environment::Span,
    focus::FocusKey,
    primitives::Point,
    render::{AnimatedJoin, AnimationDomain, ContentShape, IntrinsicShape, Realization, Render},
    render_target::RenderTarget,
};

/// Which row of a [`LazyForEach`](crate::view::LazyForEach) a built row is,
/// across rebuilds: its key, or its index where rows have no keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowId {
    /// Rows told apart by position.
    Index(usize),
    /// Rows told apart by key.
    Key(FocusKey),
}

/// The rows a [`LazyForEach`](crate::view::LazyForEach) built, in order.
///
/// Rows are paired with the rows of another tree by their identity, not
/// their place, so a row animates from where it was however the rows built
/// around it changed.
#[derive(Debug, Clone)]
pub struct LazyRows<T, const N: usize> {
    /// Each row's identity, index, and tree.
    pub(crate) rows: heapless::Vec<(RowId, usize, T), N>,
    /// Where the first row is placed, the width rows are offered, and the
    /// span along the list's axis that rows were chosen to show, from its
    /// first row's edge: what a row built later, between rebuilds, needs.
    pub(crate) origin: Point,
    pub(crate) width: u32,
    pub(crate) visible: Option<Span>,
    /// A row was built or dropped since the rows were chosen, so what the
    /// list vouched for may no longer hold: the tree must be rebuilt before
    /// it is drawn again.
    pub(crate) stale: bool,
}

impl<T, const N: usize> LazyRows<T, N> {
    pub(crate) const fn new(origin: Point, width: u32, visible: Option<Span>) -> Self {
        Self {
            rows: heapless::Vec::new(),
            origin,
            width,
            visible,
            stale: false,
        }
    }

    /// The indices of the rows built, in order.
    pub fn built(&self) -> impl Iterator<Item = usize> + '_ {
        self.rows.iter().map(|(_, index, _)| *index)
    }

    /// The row in `source` that `id` was.
    fn paired(source: &Self, id: RowId) -> Option<&T> {
        source
            .rows
            .iter()
            .find(|(source_id, ..)| *source_id == id)
            .map(|(.., tree)| tree)
    }
}

/// A list that moved takes its place at once, its rows with it, so that the
/// rows it built are where it is drawn. Rows animate between their places
/// in a list that stays put.
impl<T: AnimatedJoin, const N: usize> AnimatedJoin for LazyRows<T, N> {
    fn join_from(&mut self, source: &Self, domain: &AnimationDomain) {
        if source.origin != self.origin {
            return;
        }
        for (id, _, tree) in &mut self.rows {
            if let Some(from) = Self::paired(source, *id) {
                tree.join_from(from, domain);
            }
        }
    }
}

impl<C, T: Render<C>, const N: usize> Render<C> for LazyRows<T, N> {
    fn render(&self, render_target: &mut impl RenderTarget<ColorFormat = C>, style: &C) {
        for (.., tree) in &self.rows {
            tree.render(render_target, style);
        }
    }

    fn render_animated(
        render_target: &mut impl RenderTarget<ColorFormat = C>,
        source: &Self,
        target: &Self,
        style: &C,
        domain: &AnimationDomain,
    ) {
        // A row built since the source, or in a list that moved, is drawn as
        // an animated one is, at the domain's time, from itself. Rows since
        // dropped are gone.
        let moved = source.origin != target.origin;
        for (id, _, tree) in &target.rows {
            let from = Self::paired(source, *id).filter(|_| !moved).unwrap_or(tree);
            T::render_animated(render_target, from, tree, style, domain);
        }
    }

    fn realization(&self, app_time: Duration) -> Realization {
        if self.stale {
            return Realization::Miss;
        }
        self.rows
            .iter()
            .map(|(.., tree)| tree.realization(app_time))
            .max()
            .unwrap_or_default()
    }
}

impl<T: IntrinsicShape, const N: usize> IntrinsicShape for LazyRows<T, N> {
    fn content_shape(&self) -> ContentShape {
        self.rows
            .iter()
            .filter_map(|(.., tree)| tree.content_shape().bounding_box())
            .reduce(|a, b| a.union(&b))
            .map_or(ContentShape::Empty, ContentShape::Rectangle)
    }
}
