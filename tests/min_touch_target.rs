//! A target smaller than the minimum touch target is touched as if it were
//! grown to it, as Jetpack Compose does it.

use buoyant::render::AnimatedJoin;
use buoyant::{
    app::{App, Harness as _},
    focus::Role,
    layout::Alignment,
    primitives::{Point, Size},
    view::{
        popover::Dismissal,
        prelude::*,
        scroll_view::{ScrollBarVisibility, ScrollDirection},
    },
};
use embedded_touch::{Phase, PointerButton, Tool, Touch};

const MIN: Size = Size::new(20, 20);

#[derive(Debug, Default, PartialEq, Eq)]
struct Taps([u8; 3]);

fn button(n: usize, width: u32, height: u32, x: i32, y: i32) -> impl View<(), Taps> {
    Button::new(move |s: &mut Taps| s.0[n] += 1, |_| Rectangle)
        .frame_sized(width, height)
        .offset(x, y)
}

/// Two 10 by 10 buttons at (40, 40) and (56, 40), in front of a shape that
/// fills the screen.
fn pair(_: &Taps) -> impl View<(), Taps> + use<> {
    ZStack::new((
        Rectangle,
        button(1, 10, 10, 56, 40),
        button(0, 10, 10, 40, 40),
    ))
    .with_alignment(Alignment::TopLeading)
}

/// A 10 by 10 button at (40, 40), and a 20 by 20 one beside it at (50, 40).
fn neighbors(_: &Taps) -> impl View<(), Taps> + use<> {
    ZStack::new((
        Rectangle,
        button(1, 20, 20, 50, 40),
        button(0, 10, 10, 40, 40),
    ))
    .with_alignment(Alignment::TopLeading)
}

fn app<V, F>(view: F) -> App<V, Taps, F>
where
    V: ViewLayout<Taps, Renderables: AnimatedJoin>,
    F: Fn(&Taps) -> V,
{
    App::new(Taps::default(), Size::new(100, 100), view)
        .with_roles(Role::Button)
        .with_min_touch_target(MIN)
}

#[test]
fn a_small_button_is_touched_just_outside_it() {
    let mut app = app(pair);
    // The first button grows to (35, 35)-(55, 55).
    app.tap(Point::new(36, 45));
    app.tap(Point::new(45, 54));
    assert_eq!(*app.state(), Taps([2, 0, 0]));
    // Beyond that, the shape behind takes the touch.
    app.tap(Point::new(34, 45));
    app.tap(Point::new(45, 55));
    assert_eq!(*app.state(), Taps([2, 0, 0]));
}

#[test]
fn a_touch_that_starts_near_a_button_and_leaves_its_grown_area_does_nothing() {
    let mut app = app(pair);
    app.drag(Point::new(37, 45), Point::new(30, 45));
    assert_eq!(*app.state(), Taps([0, 0, 0]));
    app.drag(Point::new(37, 45), Point::new(36, 52));
    assert_eq!(*app.state(), Taps([1, 0, 0]));
}

#[test]
fn a_touch_directly_on_a_button_beats_one_near_another() {
    let mut app = app(neighbors);
    // (52, 45) is in the second button and in the first one's grown area.
    // The first, in front, is asked first.
    app.tap(Point::new(52, 45));
    assert_eq!(*app.state(), Taps([0, 1, 0]));
}

#[test]
fn of_two_buttons_a_touch_is_near_the_nearest_wins() {
    let mut app = app(pair);
    // Between the buttons, at 3 from the first and 4 from the second.
    app.tap(Point::new(52, 45));
    assert_eq!(*app.state(), Taps([1, 0, 0]));
    // At 5 from the first and 2 from the second, though the first is in
    // front.
    app.tap(Point::new(54, 45));
    assert_eq!(*app.state(), Taps([1, 1, 0]));
}

/// The same buttons, side by side in a stack with a shape between them.
fn row(_: &Taps) -> impl View<(), Taps> + use<> {
    ZStack::new((
        Rectangle,
        HStack::new((
            Button::new(|s: &mut Taps| s.0[0] += 1, |_| Rectangle).frame_sized(10, 10),
            Rectangle.frame_sized(6, 10),
            Button::new(|s: &mut Taps| s.0[1] += 1, |_| Rectangle).frame_sized(10, 10),
        ))
        .offset(40, 40),
    ))
    .with_alignment(Alignment::TopLeading)
}

#[test]
fn a_shape_beside_a_small_button_does_not_hide_it() {
    let mut app = app(row);
    // On the shape, nearer the first button, then nearer the second, which
    // is asked after the shape.
    app.tap(Point::new(52, 45));
    assert_eq!(*app.state(), Taps([1, 0, 0]));
    app.tap(Point::new(54, 45));
    assert_eq!(*app.state(), Taps([1, 1, 0]));
}

/// A shape in front of a small button hides it, and its grown area.
fn covered(_: &Taps) -> impl View<(), Taps> + use<> {
    ZStack::new((
        button(0, 10, 10, 40, 40),
        Rectangle.frame_sized(30, 30).offset(35, 35),
    ))
    .with_alignment(Alignment::TopLeading)
}

#[test]
fn a_shape_in_front_of_a_small_button_hides_it() {
    let mut app = app(covered);
    app.tap(Point::new(37, 45));
    app.tap(Point::new(45, 45));
    assert_eq!(*app.state(), Taps([0, 0, 0]));
}

fn list(_: &Taps) -> impl View<(), Taps> + use<> {
    ScrollView::new(VStack::new((
        Rectangle.frame_sized(100, 60),
        Button::new(|s: &mut Taps| s.0[0] += 1, |_| Rectangle).frame_sized(10, 10),
        Rectangle.frame_sized(100, 60),
        Button::new(|s: &mut Taps| s.0[1] += 1, |_| Rectangle).frame_sized(10, 10),
        Rectangle.frame_sized(100, 60),
    )))
    .with_direction(ScrollDirection::Vertical)
    .with_bar_visibility(ScrollBarVisibility::Never)
}

#[test]
fn a_small_button_in_a_scrolled_scroll_view_is_touched_near_it() {
    let mut app = app(list);
    // The buttons are at (45, 60) and (45, 130) in the content.
    app.tap(Point::new(43, 72));
    assert_eq!(*app.state(), Taps([1, 0, 0]));

    // Scrolled by 50, the second shows at (45, 80).
    app.drag(Point::new(50, 95), Point::new(50, 45));
    app.finalize_view();
    app.tap(Point::new(56, 78));
    assert_eq!(*app.state(), Taps([1, 1, 0]));
    // The first, scrolled to (45, 10), is not where it was.
    app.tap(Point::new(43, 72));
    assert_eq!(*app.state(), Taps([1, 1, 0]));
    app.tap(Point::new(43, 8));
    assert_eq!(*app.state(), Taps([2, 1, 0]));
}

#[test]
fn a_drag_that_starts_near_a_button_in_a_scroll_view_scrolls() {
    let mut app = app(list);
    app.drag(Point::new(43, 72), Point::new(43, 22));
    app.finalize_view();
    assert_eq!(*app.state(), Taps([0, 0, 0]));
    // Scrolled by 50, the first button is at (45, 10).
    app.tap(Point::new(45, 15));
    assert_eq!(*app.state(), Taps([1, 0, 0]));
}

/// A small button above a scroll view, whose grown area reaches into it.
fn header(_: &Taps) -> impl View<(), Taps> + use<> {
    VStack::new((
        HStack::new((
            Button::new(|s: &mut Taps| s.0[0] += 1, |_| Rectangle).frame_sized(10, 10),
            Spacer::default(),
        ))
        .frame_sized(100, 10),
        ScrollView::new(Rectangle.frame_sized(100, 200))
            .with_direction(ScrollDirection::Vertical)
            .with_bar_visibility(ScrollBarVisibility::Never),
    ))
}

#[test]
fn a_scroll_view_touched_outranks_a_small_button_near_it_outside() {
    let mut app = app(header);
    app.tap(Point::new(5, 13));
    assert_eq!(*app.state(), Taps([0, 0, 0]));
    app.tap(Point::new(12, 5));
    assert_eq!(*app.state(), Taps([1, 0, 0]));
}

#[derive(Debug, Default)]
struct Menu {
    open: Option<()>,
    taps: Taps,
}

fn menu(state: &Menu) -> impl View<(), Menu> + use<> {
    ZStack::new((
        Rectangle,
        Button::new(|s: &mut Menu| s.taps.0[2] += 1, |_| Rectangle)
            .frame_sized(10, 10)
            .offset(10, 10),
    ))
    .with_alignment(Alignment::TopLeading)
    .popover(state.open.as_ref(), |()| {
        Button::new(|s: &mut Menu| s.taps.0[0] += 1, |_| Rectangle).frame_sized(10, 10)
    })
    .on_blur(|s: &mut Menu| {
        s.open = None;
        Dismissal::Dismiss
    })
}

#[test]
fn an_open_popover_takes_every_touch_and_its_small_buttons_grow() {
    let mut app = App::new(
        Menu {
            open: Some(()),
            ..Menu::default()
        },
        Size::new(100, 100),
        menu,
    )
    .with_roles(Role::Button)
    .with_min_touch_target(MIN);
    // The overlay's button is centered, at (45, 45).
    app.tap(Point::new(43, 50));
    assert_eq!(app.state().taps, Taps([1, 0, 0]));
    assert!(app.state().open.is_some());
    // Near the button behind, the tap is outside the overlay, and dismisses
    // it without touching the button.
    app.tap(Point::new(8, 15));
    assert_eq!(app.state().taps, Taps([1, 0, 0]));
    assert!(app.state().open.is_none());
    app.finalize_view();
    app.tap(Point::new(8, 15));
    assert_eq!(app.state().taps, Taps([1, 0, 1]));
}

#[test]
fn with_no_minimum_only_a_touch_on_a_button_presses_it() {
    let mut app = App::new(Taps::default(), Size::new(100, 100), pair).with_roles(Role::Button);
    app.tap(Point::new(39, 45));
    app.tap(Point::new(52, 45));
    app.tap(Point::new(55, 45));
    assert_eq!(*app.state(), Taps([0, 0, 0]));
    app.tap(Point::new(40, 45));
    app.tap(Point::new(65, 45));
    assert_eq!(*app.state(), Taps([1, 1, 0]));
}

#[test]
fn a_pointer_touches_only_what_is_under_it() {
    let mut app = app(pair);
    let click = |phase| {
        Touch::new(
            0,
            Point::new(37, 45).into(),
            phase,
            Tool::Pointer {
                button: PointerButton::Primary,
            },
        )
    };
    app.send_touch(&click(Phase::Started));
    app.send_touch(&click(Phase::Ended));
    assert_eq!(*app.state(), Taps([0, 0, 0]));
}
