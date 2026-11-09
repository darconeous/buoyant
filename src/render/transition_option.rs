use crate::place::{Built, Place, build_at};
use crate::{
    primitives::{Interpolate, Size},
    render::{AnimatedJoin, ContentShape, IntrinsicShape, Render},
    render_target::RenderTarget,
    transition::{Direction, Transition},
};

use super::AnimationDomain;

/// An optional subtree that can be rendered with a transition.
// `#[repr(u8)]` lays each variant out as its tag and its fields, so that
// `Some` can be built in place.
#[derive(Debug, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum TransitionOption<Subtree, T> {
    Some {
        subtree: Subtree,
        /// Size of the subtree, used for computing offsets
        size: Size,
        /// The transition to apply
        transition: T,
    } = 0,
    None = 1,
}

/// The layout of [`TransitionOption::Some`] (RFC 2195).
#[repr(C)]
struct SomeLayout<Subtree, T> {
    tag: u8,
    subtree: Subtree,
    size: Size,
    transition: T,
}

impl<Subtree, T> TransitionOption<Subtree, T> {
    /// Builds a [`Self::Some`] of `size` and `transition` in `place`, and its
    /// subtree in its place with `build`.
    #[inline(always)]
    #[allow(unsafe_code, clippy::inline_always)]
    pub(crate) fn build_some<'p>(
        mut place: Place<'p, Self>,
        size: Size,
        transition: T,
        build: impl for<'b> FnOnce(Place<'b, Subtree>) -> Built<'b>,
    ) -> Built<'p> {
        let ptr = place.as_mut_ptr().cast::<SomeLayout<Subtree, T>>();
        // SAFETY: RFC 2195 lays `Some` out as `SomeLayout`, whose fields are
        // then places, with `Some`'s discriminant in its tag. With every field
        // filled, so is the enum.
        unsafe {
            build_at(&raw mut (*ptr).subtree, build);
            (&raw mut (*ptr).size).write(size);
            (&raw mut (*ptr).transition).write(transition);
            (&raw mut (*ptr).tag).write(0);
            place.assume_built()
        }
    }

    /// Constructs a new [`Self::Some`] variant
    #[must_use]
    pub const fn new_some(subtree: Subtree, size: Size, transition: T) -> Self {
        Self::Some {
            subtree,
            size,
            transition,
        }
    }
}

impl<Subtree: AnimatedJoin + Clone, T: Transition> AnimatedJoin for TransitionOption<Subtree, T> {
    fn join_from(&mut self, source: &Self, domain: &AnimationDomain) {
        if let (
            Self::Some {
                subtree: source_subtree,
                ..
            },
            Self::Some {
                subtree: target_subtree,
                ..
            },
        ) = (source, self)
        {
            target_subtree.join_from(source_subtree, domain);
        }
    }
}

impl<Subtree: Render<Color> + Clone, T: Transition, Color: Interpolate + Copy> Render<Color>
    for TransitionOption<Subtree, T>
{
    fn realization(&self, app_time: core::time::Duration) -> crate::render::Realization {
        match self {
            Self::Some { subtree, .. } => subtree.realization(app_time),
            Self::None => crate::render::Realization::Complete,
        }
    }

    fn render(&self, render_target: &mut impl RenderTarget<ColorFormat = Color>, style: &Color) {
        if let Self::Some { subtree, .. } = self {
            subtree.render(render_target, style);
        }
    }

    fn render_animated(
        render_target: &mut impl RenderTarget<ColorFormat = Color>,
        source: &Self,
        target: &Self,
        style: &Color,
        domain: &AnimationDomain,
    ) {
        match (source, target) {
            (
                Self::Some {
                    subtree: source, ..
                },
                Self::Some {
                    subtree: target, ..
                },
            ) => {
                Subtree::render_animated(render_target, source, target, style, domain);
            }
            (
                Self::Some {
                    subtree: source_subtree,
                    size,
                    transition,
                    ..
                },
                Self::None,
            ) => {
                if !domain.is_complete() {
                    let opacity = transition.opacity(Direction::Out, domain.factor);
                    let offset = transition.transform(Direction::Out, domain.factor, *size);
                    render_target.with_layer(
                        |l| l.offset(offset).opacity(opacity),
                        |render_target| {
                            Subtree::render_animated(
                                render_target,
                                source_subtree,
                                source_subtree,
                                style,
                                domain,
                            );
                        },
                    );
                }
            }
            (
                Self::None,
                Self::Some {
                    subtree: target_subtree,
                    size,
                    transition,
                    ..
                },
            ) => {
                // What appears has nothing to animate from, but is drawn as
                // an animated tree is, at the domain's time.
                if domain.is_complete() {
                    Subtree::render_animated(
                        render_target,
                        target_subtree,
                        target_subtree,
                        style,
                        domain,
                    );
                } else {
                    let opacity = transition.opacity(Direction::In, domain.factor);
                    let offset = transition.transform(Direction::In, domain.factor, *size);
                    render_target.with_layer(
                        |l| l.offset(offset).opacity(opacity),
                        |render_target| {
                            Subtree::render_animated(
                                render_target,
                                target_subtree,
                                target_subtree,
                                style,
                                domain,
                            );
                        },
                    );
                }
            }
            (Self::None, Self::None) => {}
        }
    }
}

impl<Subtree: IntrinsicShape, T> IntrinsicShape for TransitionOption<Subtree, T> {
    fn content_shape(&self) -> ContentShape {
        match self {
            Self::Some { subtree, .. } => subtree.content_shape(),
            Self::None => ContentShape::Empty,
        }
    }
}
