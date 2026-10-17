use embedded_touch::Touch;

use crate::{
    environment::LayoutEnvironment,
    event::{EventContext, EventResult, TouchResult},
    focus::{DefaultFocus, FocusAction, FocusDirection, Seek, SeekResult},
    layout::ResolvedLayout,
    primitives::{Point, ProposedDimension, ProposedDimensions},
    render,
    transition::Opacity,
    view::{Event, ViewLayout, ViewMarker},
};

use super::match_view::{OneOf2, OneOf3, OneOf4};

/// The axis along which the view should be fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FitAxis {
    /// Use the first view with a compact height that fits vertically.
    Vertical,
    /// Use the first view with a compact width that fits horizontally.
    Horizontal,
    /// Use the first view with a compact size that fits the space offered
    Both,
}

impl FitAxis {
    #[must_use]
    const fn components(self) -> (bool, bool) {
        match self {
            Self::Vertical => (false, true),
            Self::Horizontal => (true, false),
            Self::Both => (true, true),
        }
    }
}

/// Picks the first view that fits the available space by comparing each view's
/// preferred size to the available space.
///
/// If no other view fits, the last view is used.
///
/// Unlike other conditional views, [`ViewThatFits`] does not pass through the
/// empty and priority properties of its children.
///
/// If the branch which fits changes, the state will be rebuilt.
///
/// # Examples
///
/// ```
/// # use embedded_graphics::mono_font::MonoFont;
/// # const FONT: MonoFont<'_> = embedded_graphics::mono_font::ascii::FONT_5X8;
/// use buoyant::view::{ViewThatFits, FitAxis, Text};
///
/// let charge_view = || {
///     ViewThatFits::new(FitAxis::Vertical, {
///         Text::new("12 hours, 16 minutes, and 3 seconds to full charge", &FONT)
///     })
///     .or(Text::new("12h 16m 3s remaining", &FONT))
///     .or(Text::new("12h ⚡️", &FONT))
/// };
/// ```
///
/// In units of character size, this could produce the following results
/// for the given offered size:
///
/// > **100x1**
/// >
/// > 12 hours, 16 minutes, and 3 seconds to full charge
///
/// > **16x5**
/// >
/// > 12 hours, 16
/// > minutes, and 3
/// > seconds to full
/// > charge
///
/// > **10x2**
/// >
/// > 12h 16m 3s
/// > remaining
///
/// > **5x1**
/// >
/// > 12h ⚡️
#[derive(Debug, Clone)]
pub struct ViewThatFits<T> {
    axis: FitAxis,
    choices: T,
}

impl<T: ViewMarker> ViewThatFits<(T,)> {
    #[allow(missing_docs)]
    #[must_use]
    pub const fn new(axis: FitAxis, view: T) -> Self {
        Self {
            axis,
            choices: (view,),
        }
    }
}

impl<T: ViewMarker> ViewThatFits<(T,)> {
    /// An alternative view to use if the first one does not fit.
    #[must_use]
    pub fn or<V: ViewMarker>(self, alternate: V) -> ViewThatFits<(T, V)> {
        ViewThatFits {
            axis: self.axis,
            choices: (self.choices.0, alternate),
        }
    }
}

macro_rules! derive_or {
    ($(($n:tt, $type:ident)),*) => {
        impl<T0, $($type),*> ViewThatFits<(T0, $($type),*)> {
            /// An alternative view to use if the first one does not fit.
            #[must_use]
            pub fn or<V: ViewMarker>(self, alternate: V) -> ViewThatFits<(T0, $($type),*, V)> {
                ViewThatFits {
                    axis: self.axis,
                    choices: (self.choices.0, $(self.choices.$n),*, alternate),
                }
            }
        }
    };
}

// 4 is probably enough...? Making macros for this seems tricky
derive_or!((1, T1));
derive_or!((1, T1), (2, T2)); // this derives the 4-tuple variant

impl<T: ViewMarker> ViewMarker for ViewThatFits<(T,)> {
    type Renderables = T::Renderables;
    type Transition = Opacity;
}

impl<T, Captures: ?Sized> ViewLayout<Captures> for ViewThatFits<(T,)>
where
    T: ViewLayout<Captures>,
{
    type Sublayout = T::Sublayout;
    type State = T::State;
    type FocusTree = T::FocusTree;

    fn transition(&self) -> Self::Transition {
        Opacity
    }

    fn build_state(&self, captures: &mut Captures) -> Self::State {
        self.choices.0.build_state(captures)
    }

    fn layout(
        &self,
        offer: &ProposedDimensions,
        env: &impl LayoutEnvironment,
        captures: &mut Captures,
        state: &mut Self::State,
    ) -> ResolvedLayout<Self::Sublayout> {
        self.choices.0.layout(offer, env, captures, state)
    }

    fn render_tree(
        &self,
        layout: &Self::Sublayout,
        origin: Point,
        env: &impl LayoutEnvironment,
        captures: &mut Captures,
        state: &mut Self::State,
    ) -> Self::Renderables {
        self.choices
            .0
            .render_tree(layout, origin, env, captures, state)
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
        self.choices
            .0
            .handle_event(event, context, render_tree, captures, state, focus)
    }

    fn seek(
        &self,
        seek: &Seek,
        render_tree: &Self::Renderables,
        state: &Self::State,
        focus: Option<&Self::FocusTree>,
    ) -> SeekResult<Self::FocusTree> {
        self.choices.0.seek(seek, render_tree, state, focus)
    }

    fn handle_touch(
        &self,
        touch: &Touch,
        context: &EventContext,
        render_tree: &mut Self::Renderables,
        captures: &mut Captures,
        state: &mut Self::State,
    ) -> TouchResult<Self::FocusTree> {
        self.choices
            .0
            .handle_touch(touch, context, render_tree, captures, state)
    }
}

const fn make_compact_offer(from_offer: ProposedDimensions, axis: FitAxis) -> ProposedDimensions {
    match axis {
        FitAxis::Vertical => ProposedDimensions {
            width: from_offer.width,
            height: ProposedDimension::Compact,
        },
        FitAxis::Horizontal => ProposedDimensions {
            width: ProposedDimension::Compact,
            height: from_offer.height,
        },
        FitAxis::Both => ProposedDimensions {
            width: ProposedDimension::Compact,
            height: ProposedDimension::Compact,
        },
    }
}

/// The views of a [`ViewThatFits`] with several choices.
///
/// Each choice has its own state slot, built the first time the choice is
/// measured. Measuring never discards a slot, so a parent that measures at
/// several sizes cannot wipe the state of the choice that ends up shown.
/// Rendering mounts the chosen view: it keeps that choice's state and drops
/// the others', so a choice that is shown again later starts afresh.
macro_rules! impl_view_that_fits {
    ($one_of:ident, $last:tt, $(($idx:tt, $T:ident, $V:ident, $or_init:ident)),+) => {
        impl<$($T),+> ViewMarker for ViewThatFits<($($T,)+)>
        where
            $($T: ViewMarker,)+
        {
            type Renderables = render::$one_of<$($T::Renderables),+>;
            type Transition = Opacity;
        }

        impl<$($T,)+ Captures: ?Sized> ViewLayout<Captures> for ViewThatFits<($($T,)+)>
        where
            $($T: ViewLayout<Captures>,)+
        {
            type Sublayout = $one_of<$(ResolvedLayout<$T::Sublayout>),+>;
            type State = ($(Option<$T::State>,)+);
            type FocusTree = $one_of<$($T::FocusTree),+>;

            fn transition(&self) -> Self::Transition {
                Opacity
            }

            fn build_state(&self, _captures: &mut Captures) -> Self::State {
                Default::default()
            }

            #[allow(clippy::eq_op)]
            fn layout(
                &self,
                offer: &ProposedDimensions,
                env: &impl LayoutEnvironment,
                captures: &mut Captures,
                state: &mut Self::State,
            ) -> ResolvedLayout<Self::Sublayout> {
                let (horizontal, vertical) = self.axis.components();
                let subview_offer = make_compact_offer(*offer, self.axis);
                $(
                    let slot = state
                        .$idx
                        .get_or_insert_with(|| self.choices.$idx.build_state(captures));
                    // The last choice is used when nothing else fits.
                    let fits = $idx == $last || {
                        let layout = self.choices.$idx.layout(&subview_offer, env, captures, slot);
                        offer.contains(layout.resolved_size, horizontal, vertical)
                    };
                    if fits {
                        // Lay out again with the original offer, not the compact one.
                        let layout = self.choices.$idx.layout(offer, env, captures, slot);
                        return ResolvedLayout {
                            resolved_size: layout.resolved_size,
                            sublayouts: $one_of::$V(layout),
                        };
                    }
                )+
                unreachable!("the last choice always fits")
            }

            fn render_tree(
                &self,
                layout: &Self::Sublayout,
                origin: Point,
                env: &impl LayoutEnvironment,
                captures: &mut Captures,
                state: &mut Self::State,
            ) -> Self::Renderables {
                match layout {
                    $(
                        $one_of::$V(l) => {
                            let mut mounted = state
                                .$idx
                                .take()
                                .unwrap_or_else(|| self.choices.$idx.build_state(captures));
                            let tree = self.choices.$idx.render_tree(
                                &l.sublayouts,
                                origin,
                                env,
                                captures,
                                &mut mounted,
                            );
                            // Only the mounted choice keeps its state.
                            *state = Default::default();
                            state.$idx = Some(mounted);
                            render::$one_of::$V(tree)
                        }
                    )+
                }
            }

            fn handle_event(
                &self,
                event: &Event,
                context: &crate::event::EventContext,
                render_tree: &mut Self::Renderables,
                captures: &mut Captures,
                state: &mut Self::State,
                focus: &mut Self::FocusTree,
            ) -> EventResult {
                match render_tree {
                    $(
                        render::$one_of::$V(tree) => {
                            let Some(slot) = state.$idx.as_mut() else {
                                return EventResult::Deferred;
                            };
                            let inner_focus = match event {
                                Event::Focus {
                                    action:
                                        FocusAction::Focus(FocusDirection::Backward)
                                        | FocusAction::Previous,
                                    ..
                                } => focus.$or_init(DefaultFocus::default_last()),
                                _ => focus.$or_init(DefaultFocus::default_first()),
                            };
                            self.choices
                                .$idx
                                .handle_event(event, context, tree, captures, slot, inner_focus)
                        }
                    )+
                }
            }

            fn seek(
                &self,
                seek: &Seek,
                render_tree: &Self::Renderables,
                state: &Self::State,
                focus: Option<&Self::FocusTree>,
            ) -> SeekResult<Self::FocusTree> {
                // Only the mounted choice can be focused.
                match render_tree {
                    $(
                        render::$one_of::$V(tree) => {
                            let Some(slot) = state.$idx.as_ref() else {
                                return SeekResult::Absent;
                            };
                            let focus = match focus {
                                Some($one_of::$V(f)) => Some(f),
                                _ => None,
                            };
                            self.choices.$idx.seek(seek, tree, slot, focus).map($one_of::$V)
                        }
                    )+
                }
            }

            fn handle_touch(
                &self,
                touch: &Touch,
                context: &EventContext,
                render_tree: &mut Self::Renderables,
                captures: &mut Captures,
                state: &mut Self::State,
            ) -> TouchResult<Self::FocusTree> {
                match render_tree {
                    $(
                        render::$one_of::$V(tree) => {
                            let Some(slot) = state.$idx.as_mut() else {
                                return TouchResult::Deferred;
                            };
                            match self.choices.$idx.handle_touch(touch, context, tree, captures, slot) {
                                TouchResult::Focused(f) => TouchResult::Focused($one_of::$V(f)),
                                TouchResult::Handled => TouchResult::Handled,
                                TouchResult::Deferred => TouchResult::Deferred,
                            }
                        }
                    )+
                }
            }
        }
    };
}

impl_view_that_fits!(OneOf2, 1, (0, T0, V0, v0_or_init), (1, T1, V1, v1_or_init));
impl_view_that_fits!(
    OneOf3,
    2,
    (0, T0, V0, v0_or_init),
    (1, T1, V1, v1_or_init),
    (2, T2, V2, v2_or_init)
);
impl_view_that_fits!(
    OneOf4,
    3,
    (0, T0, V0, v0_or_init),
    (1, T1, V1, v1_or_init),
    (2, T2, V2, v2_or_init),
    (3, T3, V3, v3_or_init)
);
