use core::f32::consts::{LN_2, LOG2_E};
use core::time::Duration;

use crate::{
    primitives::{Interpolate, Point, Size, geometry::Rectangle},
    render::{AnimatedJoin, Capsule, ContentShape, IntrinsicShape, Offset, Render},
    render_target::RenderTarget,
};

// This hacks together scroll functionality from existing primitives, but
// a bespoke implementation will eventually replace it
type ScrolInner<T> = Offset<(Offset<T>, Option<Capsule>, Option<Capsule>)>;

/// This is just a metadata structure that allows [`ScrollView`] to mutate its offset and scroll bars
/// without recomputing a new view tree.
///
/// [`ScrollView`]: crate::view::ScrollView
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrollRenderable<T> {
    pub(crate) scroll_size: Size,
    pub(crate) inner_size: Size,
    pub(crate) inner: ScrolInner<T>,
    /// How the content moves to its offset, which is where it is headed.
    pub(crate) glide: Glide,
}

impl<T> ScrollRenderable<T> {
    pub(crate) fn new(
        scroll_size: Size,
        inner_size: Size,
        inner: ScrolInner<T>,
        glide: Glide,
    ) -> Self {
        Self {
            scroll_size,
            inner_size,
            inner,
            glide,
        }
    }

    /// Where the content is headed, which is where it is once it comes to
    /// rest.
    pub fn offset(&self) -> Point {
        self.inner.subtree.0.offset
    }

    pub fn offset_mut(&mut self) -> &mut Point {
        &mut self.inner.subtree.0.offset
    }

    /// The bounds of the scrollview itself
    pub fn bounds(&self) -> Rectangle {
        Rectangle::new(self.inner.offset, self.scroll_size)
    }

    pub fn inner(&self) -> &T {
        &self.inner.subtree.0.subtree
    }

    pub fn inner_mut(&mut self) -> &mut T {
        &mut self.inner.subtree.0.subtree
    }

    pub(crate) fn set_bars(&mut self, horizontal: Option<Capsule>, vertical: Option<Capsule>) {
        self.inner.subtree.1 = horizontal;
        self.inner.subtree.2 = vertical;
    }
}

impl<T: AnimatedJoin> AnimatedJoin for ScrollRenderable<T> {
    fn join_from(&mut self, source: &Self, domain: &crate::render::AnimationDomain) {
        self.scroll_size = Size::interpolate(source.scroll_size, self.scroll_size, domain.factor);
        self.inner_size = Size::interpolate(source.inner_size, self.inner_size, domain.factor);
        self.inner.join_from(&source.inner, domain);
        // The glide is the scroll view's own, kept across rebuilds: the
        // target's already carries on from the source's.
    }
}

impl<T: Render<C>, C: Interpolate + Copy> Render<C> for ScrollRenderable<T> {
    fn render(&self, render_target: &mut impl RenderTarget<ColorFormat = C>, style: &C) {
        render_target.with_layer(
            |l| l.clip(&Rectangle::new(self.inner.offset, self.scroll_size)),
            |target| {
                self.inner.render(target, style);
            },
        );
    }

    /// Draws the content where its glide has it at the domain's time, which
    /// is where it is headed once it comes to rest.
    fn render_animated(
        render_target: &mut impl RenderTarget<ColorFormat = C>,
        source: &Self,
        target: &Self,
        style: &C,
        domain: &crate::render::AnimationDomain,
    ) {
        let (content, moving) = target.glide.at(domain.app_time);
        if moving {
            render_target.report_active_animation();
        }
        let origin = Point::interpolate(source.inner.offset, target.inner.offset, domain.factor);
        render_target.with_layer(
            |l| l.clip(&Rectangle::new(target.inner.offset, target.scroll_size)),
            |render_target| {
                render_target.with_layer(
                    |l| l.offset(origin),
                    |render_target| {
                        let (from, to) = (&source.inner.subtree, &target.inner.subtree);
                        render_target.with_layer(
                            |l| l.offset(content),
                            |render_target| {
                                T::render_animated(
                                    render_target,
                                    &from.0.subtree,
                                    &to.0.subtree,
                                    style,
                                    domain,
                                );
                            },
                        );
                        // The bars stand where the content is headed.
                        Render::render_animated(render_target, &from.1, &to.1, style, domain);
                        Render::render_animated(render_target, &from.2, &to.2, style, domain);
                    },
                );
            },
        );
    }
}

impl<T: IntrinsicShape> IntrinsicShape for ScrollRenderable<T> {
    fn content_shape(&self) -> ContentShape {
        self.bounds().into()
    }
}

/// How a scroll view's content moves to where it is headed.
///
/// It moves as a critically damped spring, the quickest motion that comes
/// to rest without overshooting. A new destination taken on the way carries on from where
/// the content is at that moment and how fast it is moving, so a wheel
/// turned quickly makes one smooth motion rather than a stutter of
/// separate ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glide {
    /// Where the content is headed.
    to: Point,
    /// Where the content was at `start`, and how fast it was moving then,
    /// in [`Glide::SCALE`]ths of a pixel and of a pixel per second.
    from: [i32; 2],
    velocity: [i32; 2],
    /// In milliseconds of app time. Milliseconds rather than durations keep
    /// the glide, and every render tree with a scroll view, small.
    start: u64,
    /// How long a step of a few rows takes to come to rest, in milliseconds.
    /// Zero moves at once.
    settle: u16,
}

impl Glide {
    /// The fraction of a pixel the glide keeps track of.
    const SCALE: f32 = 64.0;
    /// The spring's angular frequency times its settle time: a step of
    /// about 40 pixels comes within half a pixel of rest by then.
    const STIFFNESS: f32 = 6.4;
    /// Content moving slower than this, in pixels per second, and within
    /// half a pixel of where it is headed, is at rest.
    const REST_SPEED: f32 = 20.0;

    /// Content at rest at `to`.
    #[must_use]
    pub const fn at_rest(to: Point) -> Self {
        Self {
            to,
            from: [to.x * Self::SCALE as i32, to.y * Self::SCALE as i32],
            velocity: [0, 0],
            start: 0,
            settle: 0,
        }
    }

    /// Where the content is headed.
    #[must_use]
    pub const fn destination(&self) -> Point {
        self.to
    }

    /// Sends the content to `to`, from where it is at `now` and as fast as
    /// it is moving then, to come to rest over about `settle`.
    pub fn head_for(&mut self, to: Point, now: Duration, settle: Duration) {
        if to == self.to {
            return;
        }
        if settle.is_zero() {
            *self = Self::at_rest(to);
            return;
        }
        let (at, velocity) = self.sample(now);
        *self = Self {
            to,
            from: at.map(|p| round(p * Self::SCALE)),
            velocity: velocity.map(|v| round(v * Self::SCALE)),
            start: u64::try_from(now.as_millis()).unwrap_or(u64::MAX),
            settle: u16::try_from(settle.as_millis()).unwrap_or(u16::MAX),
        };
    }

    /// Where to draw the content at `now`, and whether it is still moving.
    #[must_use]
    pub fn at(&self, now: Duration) -> (Point, bool) {
        let (at, velocity) = self.sample(now);
        let to = [float(self.to.x), float(self.to.y)];
        let moving = (0..2)
            .any(|axis| abs(at[axis] - to[axis]) >= 0.5 || abs(velocity[axis]) >= Self::REST_SPEED);
        if moving {
            (Point::new(round(at[0]), round(at[1])), true)
        } else {
            (self.to, false)
        }
    }

    /// Where the content is at `now`, and its velocity, in pixels and pixels
    /// per second.
    fn sample(&self, now: Duration) -> ([f32; 2], [f32; 2]) {
        let to = [float(self.to.x), float(self.to.y)];
        if self.settle == 0 {
            return (to, [0.0; 2]);
        }
        let omega = Self::STIFFNESS * 1000.0 / f32::from(self.settle);
        let t = now
            .saturating_sub(Duration::from_millis(self.start))
            .as_secs_f32();
        let decay = exp_neg(omega * t);
        let mut at = [0.0; 2];
        let mut velocity = [0.0; 2];
        for axis in 0..2 {
            // x(t) = (x0 + (v0 + ω x0) t) e^(-ωt), measured from `to`.
            let x0 = float(self.from[axis]) / Self::SCALE - to[axis];
            let v0 = float(self.velocity[axis]) / Self::SCALE;
            let b = v0 + omega * x0;
            at[axis] = to[axis] + (x0 + b * t) * decay;
            velocity[axis] = (v0 - omega * b * t) * decay;
        }
        (at, velocity)
    }
}

/// e^-x, for x of zero or more, to about four decimal places.
fn exp_neg(x: f32) -> f32 {
    if x.is_nan() || x <= 0.0 {
        return 1.0;
    }
    if x >= 80.0 {
        return 0.0;
    }
    // e^-x = 2^-n e^-r, with r below ln 2, where the series converges fast.
    let n = (x * LOG2_E) as u16;
    let r = x - f32::from(n) * LN_2;
    let series = 1.0
        - r * (1.0
            - r * (1.0 / 2.0 - r * (1.0 / 6.0 - r * (1.0 / 24.0 - r * (1.0 / 120.0 - r / 720.0)))));
    series * f32::from_bits((127 - u32::from(n)) << 23)
}

/// A pixel quantity as a float, which holds it exactly: offsets, and the
/// glide's fractions of them, stay far below 2^24.
#[expect(
    clippy::cast_precision_loss,
    reason = "pixel quantities stay far below 2^24"
)]
fn float(x: i32) -> f32 {
    x as f32
}

fn abs(x: f32) -> f32 {
    if x < 0.0 { -x } else { x }
}

fn round(x: f32) -> i32 {
    if x < 0.0 {
        (x - 0.5) as i32
    } else {
        (x + 0.5) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SETTLE: Duration = Duration::from_millis(200);

    fn ms(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    #[test]
    fn exp_neg_matches_the_exponential() {
        for i in 0..400_u16 {
            let x = f32::from(i) * 0.05;
            let exact = series_exp_neg(x);
            assert!(abs(exp_neg(x) - exact) <= 1e-4 * exact.max(1e-3), "x = {x}");
        }
    }

    /// e^-x by its series, slowly but surely.
    fn series_exp_neg(x: f32) -> f32 {
        let x = f64::from(x);
        let (mut term, mut sum) = (1.0_f64, 1.0_f64);
        for k in 1..200 {
            term *= -x / f64::from(k);
            sum += term;
        }
        sum as f32
    }

    #[test]
    fn a_step_comes_to_rest_within_its_settle_time() {
        let mut glide = Glide::at_rest(Point::new(0, 0));
        glide.head_for(Point::new(0, -40), ms(1000), SETTLE);
        let (at, moving) = glide.at(ms(1000));
        assert_eq!((at, moving), (Point::new(0, 0), true));
        let (at, moving) = glide.at(ms(1100));
        assert!(moving && at.y < -20 && at.y > -40, "{at:?}");
        assert_eq!(glide.at(ms(1200)), (Point::new(0, -40), false));
    }

    #[test]
    fn it_never_overshoots() {
        let mut glide = Glide::at_rest(Point::new(0, 0));
        glide.head_for(Point::new(0, -200), ms(0), SETTLE);
        let mut last = 0;
        for t in 0..400 {
            let (at, _) = glide.at(ms(t));
            assert!(at.y <= last && at.y >= -200, "{t} ms: {at:?}");
            last = at.y;
        }
    }

    #[test]
    fn a_new_destination_carries_on_from_where_it_is_and_how_fast() {
        let mut glide = Glide::at_rest(Point::new(0, 0));
        glide.head_for(Point::new(0, -40), ms(0), SETTLE);
        let (before, before_speed) = glide.sample(ms(60));
        glide.head_for(Point::new(0, -80), ms(60), SETTLE);
        let (after, after_speed) = glide.sample(ms(60));
        assert!(abs(before[1] - after[1]) < 0.05, "{before:?} {after:?}");
        assert!(abs(before_speed[1] - after_speed[1]) < 0.5);
        // And on to the new destination.
        assert_eq!(glide.at(ms(400)), (Point::new(0, -80), false));
    }

    #[test]
    fn zero_settle_moves_at_once() {
        let mut glide = Glide::at_rest(Point::new(0, 0));
        glide.head_for(Point::new(0, -40), ms(0), Duration::ZERO);
        assert_eq!(glide.at(ms(0)), (Point::new(0, -40), false));
    }
}
