use core::time::Duration;

use crate::{
    primitives::Interpolate,
    render::{AnimatedJoin, AnimationDomain, ContentShape, IntrinsicShape, Realization, Render},
    render_target::RenderTarget,
};

/// A subtree fading in from a time kept outside the render trees, so that
/// the fade goes on however the trees around it are rebuilt or moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FadeIn<T> {
    pub subtree: T,
    /// When the fade began, if it did.
    pub started: Option<Duration>,
    pub duration: Duration,
}

impl<T> FadeIn<T> {
    /// How opaque the subtree is at `now`, while it is still fading.
    fn alpha(&self, now: Duration) -> Option<u8> {
        let elapsed = now.saturating_sub(self.started?);
        (elapsed < self.duration)
            .then(|| (elapsed.as_millis() * 255 / self.duration.as_millis().max(1)) as u8)
    }
}

impl<T: AnimatedJoin> AnimatedJoin for FadeIn<T> {
    fn join_from(&mut self, source: &Self, domain: &AnimationDomain) {
        self.subtree.join_from(&source.subtree, domain);
    }
}

impl<T: Render<C>, C: Interpolate + Copy> Render<C> for FadeIn<T> {
    fn realization(&self, app_time: Duration) -> Realization {
        self.subtree.realization(app_time)
    }

    fn render(&self, render_target: &mut impl RenderTarget<ColorFormat = C>, style: &C) {
        self.subtree.render(render_target, style);
    }

    fn render_animated(
        render_target: &mut impl RenderTarget<ColorFormat = C>,
        source: &Self,
        target: &Self,
        style: &C,
        domain: &AnimationDomain,
    ) {
        let (from, to) = (&source.subtree, &target.subtree);
        let Some(alpha) = target.alpha(domain.app_time) else {
            T::render_animated(render_target, from, to, style, domain);
            return;
        };
        render_target.report_active_animation();
        render_target.with_layer(
            |l| l.opacity(alpha),
            |render_target| T::render_animated(render_target, from, to, style, domain),
        );
    }
}

impl<T: IntrinsicShape> IntrinsicShape for FadeIn<T> {
    fn content_shape(&self) -> ContentShape {
        self.subtree.content_shape()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fade(started: Option<u64>) -> FadeIn<()> {
        FadeIn {
            subtree: (),
            started: started.map(Duration::from_millis),
            duration: Duration::from_millis(400),
        }
    }

    #[test]
    fn it_fades_only_while_it_lasts() {
        let at = Duration::from_millis;
        assert_eq!(fade(None).alpha(at(1000)), None);
        assert_eq!(fade(Some(1000)).alpha(at(1000)), Some(0));
        assert_eq!(fade(Some(1000)).alpha(at(1200)), Some(127));
        assert_eq!(fade(Some(1000)).alpha(at(1400)), None);
    }
}
