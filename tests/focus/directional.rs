//! Moving focus toward a direction on screen follows the structure of the
//! views: a stack moves along its axis as Next and Previous do, defers
//! across it, and is entered across its axis at the child nearest where
//! focus came from.

use buoyant::{
    app::{App, Harness as _},
    event::Event,
    focus::{BoundaryBehavior, Direction, FocusKey, Role},
    primitives::{Point, Size},
    render::ContentShape,
    view::{prelude::*, scroll_view::ScrollBarVisibility},
};

#[derive(Default)]
struct State {
    tapped: heapless::Vec<u32, 8>,
    popover: Option<()>,
}

fn button(key: u32, width: u32, height: u32) -> impl View<(), State> + use<> {
    Button::new(move |s: &mut State| _ = s.tapped.push(key), |_| Rectangle)
        .frame_sized(width, height)
        .focus_key(FocusKey(key))
}

fn app<V: View<(), State>>(size: Size, view: fn(&State) -> V) -> App<V, State, fn(&State) -> V> {
    App::new(State::default(), size, view).with_roles(Role::Button)
}

/// What focus is on, and where.
fn at<V: View<(), State>>(
    app: &App<V, State, fn(&State) -> V>,
) -> (Option<FocusKey>, ContentShape) {
    (app.focused_key(), app.focus_shape().clone())
}

const ROW: u32 = 40;
const TALL: u32 = 250;
const BESIDE: u32 = 60;
const VIEW: u32 = 100;
static SLOTS: [u32; 5] = [0, 1, 2, 3, 4];

/// Five rows, the second taller than the view, between content that takes
/// no focus, scrolled through a step at a time.
fn list(_: &State) -> impl View<(), State> + use<> {
    ScrollView::new(VStack::new((
        Rectangle.frame_sized(50, BESIDE),
        ForEach::<5>::new_vertical(&SLOTS, |&slot| {
            button(slot, 50, if slot == 1 { TALL } else { ROW })
        }),
        Rectangle.frame_sized(50, BESIDE),
    )))
    .with_bar_visibility(ScrollBarVisibility::Never)
    .with_scroll_step(30)
    .bound_focus(BoundaryBehavior::Stop)
}

#[test]
fn up_and_down_in_a_list_are_previous_and_next() {
    let mut wheel = app(Size::new(50, VIEW), list);
    let mut arrows = app(Size::new(50, VIEW), list);
    assert!(wheel.focus_forward().is_handled());
    assert!(arrows.focus_forward().is_handled());

    // Down through the tall row, a step at a time, and past the last row
    // until the content's end is in view; then back up past the first.
    let mut seen = Vec::new();
    for (forward, turns) in [(true, 20), (false, 20)] {
        for _ in 0..turns {
            let (turned, moved) = if forward {
                (wheel.next(), arrows.move_toward(Direction::Down))
            } else {
                (wheel.previous(), arrows.move_toward(Direction::Up))
            };
            assert_eq!(turned.is_handled(), moved.is_handled());
            assert_eq!(at(&arrows), at(&wheel));
            seen.push(at(&arrows));
        }
    }
    // The walk stepped through the tall row and scrolled past both ends.
    let tall = seen
        .iter()
        .filter(|(key, _)| *key == Some(FocusKey(1)))
        .count();
    assert!(tall > 2, "the tall row was stepped through");
    assert_eq!(seen.first().unwrap().0, Some(FocusKey(1)));
    assert_eq!(seen[19].0, Some(FocusKey(4)));
    assert_eq!(seen.last().unwrap().0, Some(FocusKey(0)));
    let top = |shape: &ContentShape| shape.bounding_box().unwrap().origin.y;
    assert_eq!(top(&seen.last().unwrap().1), BESIDE as i32);
    assert_eq!(
        seen[19].1.bounding_box().unwrap().y_end(),
        VIEW as i32 - BESIDE as i32
    );
}

/// Three across, two across, and one the full width.
fn grid(_: &State) -> impl View<(), State> + use<> {
    VStack::new((
        HStack::new((button(0, 30, 20), button(1, 30, 20), button(2, 30, 20))),
        HStack::new((button(10, 45, 20), button(11, 45, 20))),
        button(20, 90, 20),
    ))
}

#[test]
fn down_and_up_enter_a_row_at_the_nearest_child() {
    let mut app = app(Size::new(90, 60), grid);
    assert_eq!(app.focus_forward().key(), Some(FocusKey(0)));

    // Along a row, a move is Next and Previous; it stops at the row's end.
    assert_eq!(app.move_toward(Direction::Right).key(), Some(FocusKey(1)));
    assert_eq!(app.move_toward(Direction::Right).key(), Some(FocusKey(2)));
    assert_eq!(app.move_toward(Direction::Right).key(), Some(FocusKey(2)));

    // Down from the right of three lands on the right of two, and back.
    assert_eq!(app.move_toward(Direction::Down).key(), Some(FocusKey(11)));
    assert_eq!(app.move_toward(Direction::Up).key(), Some(FocusKey(2)));

    // From the left of three, the left of two; then the wide one below.
    assert_eq!(app.move_toward(Direction::Left).key(), Some(FocusKey(1)));
    assert_eq!(app.move_toward(Direction::Left).key(), Some(FocusKey(0)));
    assert_eq!(app.move_toward(Direction::Down).key(), Some(FocusKey(10)));
    assert_eq!(app.move_toward(Direction::Up).key(), Some(FocusKey(0)));
    assert_eq!(app.move_toward(Direction::Down).key(), Some(FocusKey(10)));
    assert_eq!(app.move_toward(Direction::Down).key(), Some(FocusKey(20)));

    // Up from the wide one, centered, lands on the nearer of two.
    assert_eq!(app.move_toward(Direction::Up).key(), Some(FocusKey(11)));

    // Across the bottom, nothing: the wide one has no neighbor.
    assert_eq!(app.move_toward(Direction::Down).key(), Some(FocusKey(20)));
    assert_eq!(app.move_toward(Direction::Left).key(), Some(FocusKey(20)));
}

fn bounded_column(_: &State) -> impl View<(), State> + use<> {
    VStack::new((button(0, 50, 20), button(1, 50, 20), button(2, 50, 20)))
        .bound_focus(BoundaryBehavior::Wrap)
}

#[test]
fn bound_focus_stops_a_directional_move() {
    let mut app = app(Size::new(50, 60), bounded_column);
    assert_eq!(app.focus_forward().key(), Some(FocusKey(0)));
    assert_eq!(app.move_toward(Direction::Down).key(), Some(FocusKey(1)));
    assert_eq!(app.move_toward(Direction::Down).key(), Some(FocusKey(2)));

    // The wheel wraps here; a directional move does not.
    assert_eq!(app.move_toward(Direction::Down).key(), Some(FocusKey(2)));
    assert_eq!(app.move_toward(Direction::Right).key(), Some(FocusKey(2)));
    assert_eq!(app.next().key(), Some(FocusKey(0)));
    assert_eq!(app.move_toward(Direction::Up).key(), Some(FocusKey(0)));
    assert_eq!(app.move_toward(Direction::Left).key(), Some(FocusKey(0)));
}

fn with_popover(state: &State) -> impl View<(), State> + use<> {
    VStack::new((button(0, 50, 20), button(1, 50, 20))).popover(state.popover.as_ref(), |()| {
        VStack::new((button(10, 40, 20), button(11, 40, 20)))
    })
}

#[test]
fn a_popover_keeps_a_directional_move_in_its_overlay() {
    let mut app = app(Size::new(50, 60), with_popover);
    assert_eq!(app.focus_forward().key(), Some(FocusKey(0)));
    app.state_mut().popover = Some(());
    app.finalize_view();
    assert_eq!(app.focused_key(), Some(FocusKey(10)));

    assert_eq!(app.move_toward(Direction::Down).key(), Some(FocusKey(11)));
    // Past the overlay's edges, focus stays in it, where it was.
    assert_eq!(app.move_toward(Direction::Down).key(), Some(FocusKey(11)));
    assert_eq!(app.move_toward(Direction::Right).key(), Some(FocusKey(11)));
    assert_eq!(app.move_toward(Direction::Up).key(), Some(FocusKey(10)));
    assert_eq!(app.move_toward(Direction::Up).key(), Some(FocusKey(10)));
    assert_eq!(app.move_toward(Direction::Left).key(), Some(FocusKey(10)));
}

fn rows(_: &State) -> impl View<(), State> + use<> {
    ScrollView::new(ForEach::<5>::new_vertical(&SLOTS, |&slot| {
        button(slot, 50, ROW)
    }))
    .with_bar_visibility(ScrollBarVisibility::Never)
}

#[test]
fn focus_lands_under_a_point_in_a_scrolled_view() {
    let mut app = app(Size::new(50, VIEW), rows);
    assert_eq!(app.focus_forward().key(), Some(FocusKey(0)));
    app.send(Event::Scroll(Point::new(0, -50)));
    app.finalize_view();

    // Scrolled by 50, a point 50 down the view is 100 into the rows.
    assert!(app.focus_at(Point::new(25, 50)));
    assert_eq!(app.focused_key(), Some(FocusKey(2)));
    assert_eq!(
        app.focus_shape().bounding_box().unwrap().origin,
        Point::new(0, 2 * ROW as i32 - 50)
    );
    assert!(app.focus_at(Point::new(25, 95)));
    assert_eq!(app.focused_key(), Some(FocusKey(3)));

    // Outside the view, what is scrolled there is not under the point.
    assert!(!app.focus_at(Point::new(25, 120)));
    assert_eq!(app.focused_key(), Some(FocusKey(3)));
}

fn with_pointer(_: &State) -> impl View<(), State> + use<> {
    VStack::new((
        button(0, 50, 20),
        Button::new(|s: &mut State| _ = s.tapped.push(1), |_| Rectangle)
            .with_role(Role::Pointer)
            .frame_sized(50, 20),
        button(2, 50, 20),
    ))
}

#[test]
fn a_pointer_button_is_passed_over_and_taps_without_focus() {
    let mut app = app(Size::new(50, 60), with_pointer);
    assert_eq!(app.focus_forward().key(), Some(FocusKey(0)));
    assert_eq!(app.next().key(), Some(FocusKey(2)));
    assert_eq!(app.previous().key(), Some(FocusKey(0)));
    assert_eq!(app.move_toward(Direction::Down).key(), Some(FocusKey(2)));
    assert!(!app.focus_at(Point::new(25, 30)));

    // A tap runs its action, and focus stays where it was.
    app.tap(Point::new(25, 30));
    app.finalize_view();
    assert_eq!(app.state().tapped, [1]);
    assert_eq!(app.focused_key(), Some(FocusKey(2)));
}

#[test]
fn a_cancelled_touch_does_not_tap() {
    let mut app = app(Size::new(90, 60), grid);
    app.touch_down(Point::new(10, 10));
    app.cancel_touch();
    app.touch_up(Point::new(10, 10));
    app.finalize_view();
    assert!(app.state().tapped.is_empty());

    // The next touch is answered.
    app.tap(Point::new(10, 10));
    app.finalize_view();
    assert_eq!(app.state().tapped, [0]);
}
