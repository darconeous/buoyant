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
///
/// Flung, the content coasts, slowing at a steady rate, and a bound it
/// reaches on the way takes it as the spring does.
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
    /// For flung content, the time over which its speed falls by a factor
    /// of e, in milliseconds. Zero for a spring alone.
    coast: u16,
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
    /// The fastest flung content meets a bound, in pixels per second: the
    /// spring there takes it from no faster, so it overshoots little.
    const BOUNCE_SPEED: f32 = 1000.0;

    /// Content at rest at `to`.
    #[must_use]
    pub const fn at_rest(to: Point) -> Self {
        Self {
            to,
            from: [to.x * Self::SCALE as i32, to.y * Self::SCALE as i32],
            velocity: [0, 0],
            start: 0,
            settle: 0,
            coast: 0,
        }
    }

    /// Flings the content from `from` at `velocity`, in pixels per second,
    /// at `now`. It coasts to rest at `to`, its speed falling by a factor of
    /// e every `coast`. A `to` short of where it would coast to is a bound:
    /// reaching it, the content springs to rest there as it would over
    /// `settle`.
    #[must_use]
    pub fn fling(
        from: Point,
        velocity: [f32; 2],
        to: Point,
        now: Duration,
        coast: Duration,
        settle: Duration,
    ) -> Self {
        if settle.is_zero() {
            return Self::at_rest(to);
        }
        Self {
            to,
            from: [from.x * Self::SCALE as i32, from.y * Self::SCALE as i32],
            velocity: velocity.map(|v| round(v * Self::SCALE)),
            start: u64::try_from(now.as_millis()).unwrap_or(u64::MAX),
            settle: u16::try_from(settle.as_millis()).unwrap_or(u16::MAX),
            coast: u16::try_from(coast.as_millis()).unwrap_or(u16::MAX),
        }
    }

    /// Where content flung from `from` at `velocity` would coast to rest,
    /// with nothing in its way.
    #[must_use]
    pub fn coast_stop(from: Point, velocity: [f32; 2], coast: Duration) -> Point {
        let travel = velocity.map(|v| round(v * coast.as_secs_f32()));
        from + Point::new(travel[0], travel[1])
    }

    /// Flung content still on its way.
    #[must_use]
    pub fn is_coasting(&self, now: Duration) -> bool {
        self.coast != 0 && self.at(now).1
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
            coast: 0,
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
        let mut at = [0.0; 2];
        let mut velocity = [0.0; 2];
        for axis in 0..2 {
            let x0 = float(self.from[axis]) / Self::SCALE - to[axis];
            let v0 = float(self.velocity[axis]) / Self::SCALE;
            let (x, v) = if self.coast == 0 {
                spring(x0, v0, omega, t)
            } else {
                coast(x0, v0, 1000.0 / f32::from(self.coast), omega, t)
            };
            at[axis] = to[axis] + x;
            velocity[axis] = v;
        }
        (at, velocity)
    }
}

/// A critically damped spring at `t`, from `x0` at `v0`, measured from where
/// it comes to rest: x(t) = (x0 + (v0 + ω x0) t) e^(-ωt).
fn spring(x0: f32, v0: f32, omega: f32, t: f32) -> (f32, f32) {
    let decay = exp_neg(omega * t);
    let b = v0 + omega * x0;
    ((x0 + b * t) * decay, (v0 - omega * b * t) * decay)
}

/// Flung content at `t`, from `x0` at `v0`, measured from where it comes to
/// rest, its speed falling at `rate` per second. Coasting is the spring
/// whose ω is the rate, sent to where the coast ends; a nearer rest is a
/// bound, where the spring of `omega` takes over.
fn coast(x0: f32, v0: f32, rate: f32, omega: f32, t: f32) -> (f32, f32) {
    let travel = v0 / rate;
    let ahead = -x0;
    if travel * ahead > 0.0 && abs(travel) > abs(ahead) + 0.5 {
        // The bound is reached when e^(-rate t) has fallen to `left`.
        let left = 1.0 - ahead / travel;
        let reached = -ln(left) / rate;
        if t < reached {
            let decay = exp_neg(rate * t);
            (x0 + travel * (1.0 - decay), v0 * decay)
        } else {
            let bounce = Glide::BOUNCE_SPEED;
            spring(0.0, (v0 * left).clamp(-bounce, bounce), omega, t - reached)
        }
    } else if travel * ahead > 0.0 || abs(travel - ahead) <= 0.5 {
        spring(x0, v0, rate, t)
    } else {
        // Flung away from its rest, as from beyond a bound.
        spring(x0, v0, omega, t)
    }
}

/// ln x, for x above zero, to about six decimal places.
fn ln(x: f32) -> f32 {
    // x = m 2^e, with m from one to two: ln m = 2 atanh((m - 1) / (m + 1)).
    let bits = x.to_bits();
    let exponent = float(((bits >> 23) & 0xff) as i32 - 127);
    let m = f32::from_bits((bits & 0x007f_ffff) | 0x3f80_0000);
    let z = (m - 1.0) / (m + 1.0);
    let z2 = z * z;
    let series =
        2.0 * z * (1.0 + z2 * (1.0 / 3.0 + z2 * (1.0 / 5.0 + z2 * (1.0 / 7.0 + z2 / 9.0))));
    exponent * LN_2 + series
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

    const COAST: Duration = Duration::from_millis(300);

    #[test]
    fn ln_matches_the_logarithm() {
        for i in 1..400_u16 {
            let x = f32::from(i) / 100.0;
            assert!(
                abs(series_exp_neg(-ln(x)) - x) <= 2e-5 * x.max(1.0),
                "x = {x}"
            );
        }
    }

    #[test]
    fn flung_content_coasts_to_rest() {
        let from = Point::new(0, 0);
        let velocity = [0.0, -1000.0];
        let to = Glide::coast_stop(from, velocity, COAST);
        assert_eq!(to, Point::new(0, -300));
        let glide = Glide::fling(from, velocity, to, ms(0), COAST, SETTLE);
        // It slows by e every 300 ms.
        let (at, speed) = glide.sample(ms(300));
        assert!(abs(at[1] - -300.0 * (1.0 - 0.3679)) < 1.0, "{at:?}");
        assert!(abs(speed[1] - -367.9) < 1.0, "{speed:?}");
        let mut last = 0;
        let mut rest = None;
        for t in 0..5000 {
            let (at, moving) = glide.at(ms(t));
            assert!(at.y <= last && at.y >= -300, "{t} ms: {at:?}");
            last = at.y;
            if !moving {
                rest = rest.or(Some(t));
            }
        }
        assert!(rest.is_some_and(|t| t < 3000), "{rest:?}");
        assert!(glide.is_coasting(ms(100)) && !glide.is_coasting(ms(5000)));
    }

    #[test]
    fn flung_content_springs_back_from_a_bound_it_reaches() {
        let glide = Glide::fling(
            Point::new(0, 0),
            [0.0, -1000.0],
            Point::new(0, -100),
            ms(0),
            COAST,
            SETTLE,
        );
        // e^(-t / 300 ms) falls to 2/3 when it reaches the bound.
        let reached = 121;
        let (at, speed) = glide.sample(ms(reached));
        assert!(
            abs(at[1] - -100.0) < 1.0 && abs(speed[1] - -666.7) < 10.0,
            "{at:?} {speed:?}"
        );
        let mut furthest = 0;
        for t in 0..1000 {
            furthest = furthest.min(glide.at(ms(t)).0.y);
        }
        assert!(furthest < -100 && furthest > -110, "{furthest}");
        assert_eq!(glide.at(ms(1000)), (Point::new(0, -100), false));
    }

    #[test]
    fn content_flung_outward_from_a_bound_springs_back() {
        let glide = Glide::fling(
            Point::new(0, 0),
            [0.0, 400.0],
            Point::new(0, 0),
            ms(0),
            COAST,
            SETTLE,
        );
        let furthest = (0..400).map(|t| glide.at(ms(t)).0.y).max();
        assert!(furthest.is_some_and(|y| y > 0 && y < 10), "{furthest:?}");
        assert_eq!(glide.at(ms(600)), (Point::new(0, 0), false));
    }

    #[test]
    fn zero_settle_moves_at_once() {
        let mut glide = Glide::at_rest(Point::new(0, 0));
        glide.head_for(Point::new(0, -40), ms(0), Duration::ZERO);
        assert_eq!(glide.at(ms(0)), (Point::new(0, -40), false));
    }
}
