//! `ViewThatFits` keeps the shown choice's state through measurement, and
//! starts a choice afresh when it is shown again after another was.

use buoyant::{
    app::{App, Harness as _},
    focus::{FocusKey, Role},
    primitives::Size,
    render::ContentShape,
    view::{
        prelude::*,
        rotary::{RotaryEvent, RotaryState},
        scroll_view::ScrollBarVisibility,
    },
};

#[derive(Default)]
struct State {
    turns: u32,
    generation: u32,
}

const DIAL: FocusKey = FocusKey(1);
const SMALL_DIAL: FocusKey = FocusKey(2);
const AFTER: FocusKey = FocusKey(3);

fn dial(height: u32) -> impl View<(), State> + use<> {
    Rotary::new(
        |s: &mut State, event: RotaryEvent| {
            if matches!(event, RotaryEvent::Next) {
                s.turns += 1;
            }
        },
        move |_: RotaryState| Rectangle.frame_sized(50, height),
    )
}

/// A choice inside a stack, which measures it at several sizes.
fn view(_: &State) -> impl View<(), State> + use<> {
    VStack::new((
        ViewThatFits::new(FitAxis::Vertical, dial(100).focus_key(DIAL))
            .or(dial(10).focus_key(SMALL_DIAL)),
        Button::new(|_: &mut State| {}, |_| Circle)
            .frame_sized(20, 20)
            .focus_key(AFTER),
    ))
}

#[test]
fn the_shown_choice_keeps_its_state_through_measurement() {
    let mut app = App::new(State::default(), Size::new(100, 200), view).with_roles(Role::Button);
    assert_eq!(app.focus_forward().key(), Some(DIAL));
    // Capture the dial.
    assert!(app.select().is_handled());
    app.finalize_view();

    // An unrelated change rebuilds the view; the stack measures the choice
    // again, including at sizes where only the small dial fits.
    app.state_mut().generation += 1;
    app.finalize_view();
    assert_eq!(app.focused_key(), Some(DIAL));

    // Still captive: the turn reaches the dial instead of moving focus.
    assert!(app.next().is_handled());
    assert_eq!(app.focused_key(), Some(DIAL));
    assert_eq!(app.state().turns, 1);
}

#[test]
fn a_choice_shown_again_starts_afresh() {
    let mut app = App::new(State::default(), Size::new(100, 200), view).with_roles(Role::Button);
    assert_eq!(app.focus_forward().key(), Some(DIAL));
    assert!(app.select().is_handled());
    app.finalize_view();

    // Too short for the large dial: the small one is shown.
    app.resize(Size::new(100, 40));
    app.finalize_view();
    assert_eq!(app.focused_key(), Some(SMALL_DIAL));

    // Tall again: the large dial returns without its capture.
    app.resize(Size::new(100, 200));
    app.finalize_view();
    assert_eq!(app.focused_key(), Some(DIAL));
    assert!(app.next().is_handled());
    assert_eq!(app.focused_key(), Some(AFTER));
    assert_eq!(app.state().turns, 0);
}

fn row() -> impl View<(), State> + use<> {
    Button::new(|_: &mut State| {}, |_| Rectangle).frame_sized(50, 40)
}

/// A scrolling list as the large choice: its scroll offset is view state.
fn scrolling(_: &State) -> impl View<(), State> + use<> {
    VStack::new((
        ViewThatFits::new(
            FitAxis::Vertical,
            ScrollView::new(VStack::new((row(), row(), row(), row(), row())))
                .with_bar_visibility(ScrollBarVisibility::Never)
                .frame_sized(50, 100),
        )
        .or(Rectangle.frame_sized(50, 10)),
        Button::new(|_: &mut State| {}, |_| Circle).frame_sized(20, 20),
    ))
}

fn top_of(shape: &ContentShape) -> i32 {
    match shape {
        ContentShape::Rectangle(rect) => rect.origin.y,
        other => panic!("unexpected focus shape {other:?}"),
    }
}

#[test]
fn the_shown_choice_keeps_its_scroll_offset_through_measurement() {
    let mut app =
        App::new(State::default(), Size::new(100, 200), scrolling).with_roles(Role::Button);
    assert!(app.focus_forward().is_handled());
    for _ in 0..3 {
        assert!(app.next().is_handled());
    }
    // The fourth row was scrolled up to the view's bottom edge.
    let top = top_of(app.focus_shape());
    assert!(top < 100, "fourth row at {top}");

    app.state_mut().generation += 1;
    app.finalize_view();
    assert_eq!(top_of(app.focus_shape()), top);
}
