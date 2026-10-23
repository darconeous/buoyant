use crate::{
    environment::{LayoutEnvironment, ViewportEnvironment},
    event::{Event, EventResult, TouchResult},
    focus::{Seek, SeekResult, SeekTarget},
    layout::ResolvedLayout,
    primitives::{Point, ProposedDimensions},
    render::Offset,
    view::{ViewLayout, ViewMarker},
};

use embedded_touch::Touch;

#[derive(Debug, Clone)]
pub struct GeometryGroup<InnerView> {
    inner: InnerView,
}

impl<InnerView: ViewMarker> GeometryGroup<InnerView> {
    pub const fn new(view: InnerView) -> Self {
        Self { inner: view }
    }
}

impl<InnerView> ViewMarker for GeometryGroup<InnerView>
where
    InnerView: ViewMarker,
{
    type Renderables = Offset<InnerView::Renderables>;
    type Transition = InnerView::Transition;
}

impl<Captures: ?Sized, InnerView> ViewLayout<Captures> for GeometryGroup<InnerView>
where
    InnerView: ViewLayout<Captures>,
{
    type Sublayout = InnerView::Sublayout;
    type State = InnerView::State;
    type FocusTree = InnerView::FocusTree;

    fn priority(&self) -> i8 {
        self.inner.priority()
    }

    fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    fn transition(&self) -> Self::Transition {
        self.inner.transition()
    }

    fn build_state(&self, captures: &mut Captures) -> Self::State {
        self.inner.build_state(captures)
    }

    fn layout(
        &self,
        offer: &ProposedDimensions,
        env: &impl LayoutEnvironment,
        captures: &mut Captures,
        state: &mut Self::State,
    ) -> ResolvedLayout<Self::Sublayout> {
        self.inner.layout(offer, env, captures, state)
    }

    fn render_tree(
        &self,
        layout: &Self::Sublayout,
        origin: Point,
        env: &impl LayoutEnvironment,
        captures: &mut Captures,
        state: &mut Self::State,
    ) -> Self::Renderables {
        // Store the offset, and render subtrees from zero, where a viewport
        // the subtree is shown through is brought into its coordinates.
        let Some(viewport) = env.viewport() else {
            return Offset::new(
                origin,
                self.inner
                    .render_tree(layout, Point::zero(), env, captures, state),
            );
        };
        let rebased = viewport.rebased(origin);
        let subtree = self.inner.render_tree(
            layout,
            Point::zero(),
            &ViewportEnvironment::new(env, Some(&rebased)),
            captures,
            state,
        );
        viewport.absorb(&rebased, origin);
        Offset::new(origin, subtree)
    }

    fn handle_event(
        &self,
        event: &crate::view::Event,
        context: &crate::event::EventContext,
        render_tree: &mut Self::Renderables,
        captures: &mut Captures,
        state: &mut Self::State,
        focus: &mut Self::FocusTree,
    ) -> EventResult {
        // The subtree is laid out from zero: a frame focus moves from is
        // brought into its coordinates.
        let event = match event {
            Event::Focus { action, group } => &Event::Focus {
                action: action.offset(-render_tree.offset),
                group: *group,
            },
            other => other,
        };
        self.inner
            .handle_event(
                event,
                context,
                &mut render_tree.subtree,
                captures,
                state,
                focus,
            )
            .with_offset(render_tree.offset)
    }

    fn seek(
        &self,
        seek: &Seek,
        render_tree: &Self::Renderables,
        state: &Self::State,
        focus: Option<&Self::FocusTree>,
    ) -> SeekResult<Self::FocusTree> {
        let seek = match seek.target {
            SeekTarget::At(point) => seek.with_target(SeekTarget::At(point - render_tree.offset)),
            _ => *seek,
        };
        self.inner.seek(&seek, &render_tree.subtree, state, focus)
    }

    fn handle_touch(
        &self,
        touch: &Touch,
        context: &crate::event::EventContext,
        render_tree: &mut Self::Renderables,
        captures: &mut Captures,
        state: &mut Self::State,
    ) -> TouchResult<Self::FocusTree> {
        let mut touch = touch.clone();
        touch.location -= render_tree.offset.into();
        self.inner
            .handle_touch(&touch, context, &mut render_tree.subtree, captures, state)
    }
}
