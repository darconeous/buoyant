use crate::place::{Built, Place, build_variant};
use crate::render::{ContentShape, IntrinsicShape, Render, RenderTarget};

use super::AnimatedJoin;

/// The index of a variant, by its name.
macro_rules! index_of {
    (V0) => {
        0
    };
    (V1) => {
        1
    };
    (V2) => {
        2
    };
    (V3) => {
        3
    };
    (V4) => {
        4
    };
    (V5) => {
        5
    };
    (V6) => {
        6
    };
    (V7) => {
        7
    };
    (V8) => {
        8
    };
    (V9) => {
        9
    };
}

macro_rules! define_branch {
    ($name:ident, $($variant:ident),+) => {
        // `#[repr(u8)]` lays each variant out as its tag and its payload, so
        // that a variant can be built in place.
        #[derive(Debug, Clone, PartialEq, Eq)]
        #[repr(u8)]
        pub enum $name<$($variant),+> {
            $(
                $variant($variant) = index_of!($variant),
            )+
        }

        paste::paste! {
            impl<$($variant),+> $name<$($variant),+> {
                $(
                    #[doc = "Builds the `" $variant "` variant in `place`, and its payload in its place with `build`."]
                    #[inline(always)]
                    #[allow(unsafe_code, clippy::inline_always)]
                    pub(crate) fn [<build_ $variant:lower>]<'p>(
                        place: Place<'p, Self>,
                        build: impl for<'b> FnOnce(Place<'b, $variant>) -> Built<'b>,
                    ) -> Built<'p> {
                        // SAFETY: `Self` is `#[repr(u8)]`, and the variant's
                        // discriminant is its index.
                        unsafe { build_variant(place, index_of!($variant), build) }
                    }
                )+
            }
        }

        impl<$($variant),+>  AnimatedJoin for $name<$($variant),+>
            where $($variant: AnimatedJoin,)+
        {
            fn join_from(&mut self, source: &Self, domain: &crate::render::AnimationDomain) {
                match (source, self) {
                    $(
                        (Self::$variant(source), Self::$variant(target)) => {
                            target.join_from(source, domain)
                        },
                    )+
                    (_, _) => (),
                }
            }
        }

        impl<C, $($variant),+> Render<C> for $name<$($variant),+>
            where $($variant: Render<C>,)+
        {
            fn render(&self, target: &mut impl RenderTarget<ColorFormat = C>, color: &C) {
                match self {
                    $(
                        Self::$variant(v) => v.render(target, color),
                    )+
                }
            }

            fn render_animated(
                render_target: &mut impl RenderTarget<ColorFormat = C>,
                source: &Self,
                target: &Self,
                style: &C,
                domain: &crate::render::AnimationDomain,
            ) {
                match (source, target) {
                    $(
                        (Self::$variant(source), Self::$variant(target)) => {
                            $variant::render_animated(render_target, source, target, style, domain);
                        },
                    )+
                    // A newly chosen branch has nothing to animate from, but
                    // is drawn as an animated one is, at the domain's time.
                    $(
                        (_, Self::$variant(target)) => {
                            $variant::render_animated(render_target, target, target, style, domain);
                        },
                    )+
                }
            }

            fn realization(&self, app_time: core::time::Duration) -> crate::render::Realization {
                match self {
                    $(
                        Self::$variant(v) => v.realization(app_time),
                    )+
                }
            }
        }

        impl<$($variant),+> IntrinsicShape for $name<$($variant),+>
            where $($variant: IntrinsicShape,)+
        {
            fn content_shape(&self) -> ContentShape {
                match self {
                    $(
                        Self::$variant(v) => v.content_shape(),
                    )+
                }
            }
        }
    }
}

// OneOf1 has no reason to exist, skip it
// Views with only one variant should just use the inner type directly
define_branch!(OneOf2, V0, V1);
define_branch!(OneOf3, V0, V1, V2);
define_branch!(OneOf4, V0, V1, V2, V3);
define_branch!(OneOf5, V0, V1, V2, V3, V4);
define_branch!(OneOf6, V0, V1, V2, V3, V4, V5);
define_branch!(OneOf7, V0, V1, V2, V3, V4, V5, V6);
define_branch!(OneOf8, V0, V1, V2, V3, V4, V5, V6, V7);
define_branch!(OneOf9, V0, V1, V2, V3, V4, V5, V6, V7, V8);
define_branch!(OneOf10, V0, V1, V2, V3, V4, V5, V6, V7, V8, V9);
