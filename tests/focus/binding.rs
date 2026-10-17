//! Focus bound into application state: requests made there move focus once
//! the view is settled, and the focused key is recorded there.

use buoyant::{
    app::{App, Harness as _},
    event::Event,
    focus::{self, FocusKey, FocusState, Role},
    match_view,
    primitives::{Point, Size},
    render::ContentShape,
    view::{
        prelude::*,
        rotary::{RotaryEvent, RotaryState},
        scroll_view::ScrollBarVisibility,
    },
};

#[derive(Default)]
struct State {
    focus: FocusState,
    /// Turns and exits of each dial, by key.
    turns: [u32; 32],
    exits: [u32; 32],
    /// Changes that rebuild the view without changing it.
    generation: u32,
    open: bool,
    rows: &'static [u32],
    second_screen: bool,
}

fn bind(state: &mut State) -> &mut FocusState {
    &mut state.focus
}

fn key(k: u32) -> FocusKey {
    FocusKey(k)
}

/// A dial that counts its turns and exits.
fn dial(k: u32) -> impl View<(), State> + use<> {
    let i = k as usize;
    Rotary::new(
        move |s: &mut State, event: RotaryEvent| match event {
            RotaryEvent::Next => s.turns[i] += 1,
            RotaryEvent::Exit => s.exits[i] += 1,
            _ => {}
        },
        |_: RotaryState| Rectangle,
    )
    .frame_sized(50, 20)
    .focus_key(key(k))
}

fn app<V: View<(), State>>(state: State, view: fn(&State) -> V) -> App<V, State, fn(&State) -> V> {
    App::new(state, Size::new(100, 200), view)
        .with_roles(Role::Button)
        .with_focus_binding(bind)
}

/// Focuses the dial `k` and captures it.
fn capture<V: View<(), State>>(app: &mut App<V, State, fn(&State) -> V>, k: u32) {
    app.focus(key(k));
    assert_eq!(app.focused_key(), Some(key(k)));
    assert!(app.select().is_handled());
    app.finalize_view();
}

/// Whether the dial `k` is captive: a turn reaches it.
fn is_captive<V: View<(), State>>(app: &mut App<V, State, fn(&State) -> V>, k: u32) -> bool {
    let before = app.state().turns[k as usize];
    _ = app.next();
    app.finalize_view();
    app.state().turns[k as usize] > before
}

fn three_dials(_: &State) -> impl View<(), State> + use<> {
    VStack::new((dial(0), dial(1), dial(2)))
}

#[test]
fn a_request_moves_focus_and_is_recorded() {
    let mut app = app(State::default(), three_dials);
    app.focus(key(2));
    assert_eq!(app.focused_key(), Some(key(2)));
    assert_eq!(app.state().focus.current(), Some(key(2)));
    assert_eq!(app.state().focus.pending(), None);
}

#[test]
fn a_jump_releases_the_captive_control_once() {
    let mut app = app(State::default(), three_dials);
    capture(&mut app, 0);
    app.focus(key(2));
    assert_eq!(app.focused_key(), Some(key(2)));
    assert_eq!(app.state().exits, exits(&[(0, 1)]));
    app.force_rebuild();
    assert_eq!(app.state().exits, exits(&[(0, 1)]));
}

fn exits(counts: &[(usize, u32)]) -> [u32; 32] {
    let mut all = [0; 32];
    for &(i, n) in counts {
        all[i] = n;
    }
    all
}

#[test]
fn requests_that_cannot_be_met_leave_a_captive_control_alone() {
    let mut app = app(State::default(), three_dials);
    capture(&mut app, 1);

    // Not in the view, with and without a rebuild pending.
    app.focus(key(9));
    {
        let mut state = app.state_mut();
        state.generation += 1;
        state.focus.request(key(9));
    }
    app.finalize_view();
    assert_eq!(app.focused_key(), Some(key(1)));
    assert_eq!(app.state().focus.pending(), None);
    assert_eq!(app.state().exits, exits(&[]));
    assert!(is_captive(&mut app, 1));
}

#[test]
fn requesting_what_is_focused_keeps_it_captive() {
    let mut app = app(State::default(), three_dials);
    capture(&mut app, 1);
    app.focus(key(1));
    assert!(is_captive(&mut app, 1));
    {
        let mut state = app.state_mut();
        state.generation += 1;
        state.focus.request(key(1));
    }
    app.finalize_view();
    assert!(is_captive(&mut app, 1));
    assert_eq!(app.state().exits, exits(&[]));
}

fn two_groups(_: &State) -> impl View<(), State> + use<> {
    VStack::new((
        VStack::new((dial(0), dial(1))).exclusive_focus(focus::GROUP_1),
        VStack::new((dial(2), dial(3))).exclusive_focus(focus::GROUP_2),
    ))
}

#[test]
fn a_request_into_another_group_moves_focus_there() {
    let mut app = app(State::default(), two_groups);
    app.focus(key(0));
    assert_eq!(app.focus_group(), focus::GROUP_1);
    assert!(app.select_group(focus::GROUP_1).is_handled());
    app.finalize_view();

    app.focus(key(2));
    assert_eq!(app.focused_key(), Some(key(2)));
    assert_eq!(app.focus_group(), focus::GROUP_2);
    assert_eq!(app.state().exits, exits(&[(0, 1)]));

    // A rebuild, a turn and a press all stay in the new group.
    app.force_rebuild();
    assert_eq!(app.focused_key(), Some(key(2)));
    assert_eq!(app.next_group(focus::GROUP_2).key(), Some(key(3)));
    assert!(app.select_group(focus::GROUP_2).is_handled());
    app.finalize_view();
    assert_eq!(app.next_group(focus::GROUP_2).key(), Some(key(3)));
    assert_eq!(app.state().turns, exits(&[(3, 1)]));
}

fn screens(state: &State) -> impl View<(), State> + use<> {
    match_view!(state.second_screen, {
        false => VStack::new((dial(0), dial(1))),
        true => VStack::new((dial(2), dial(3))),
    })
}

#[test]
fn a_request_finds_its_target_on_the_screen_now_shown() {
    let mut app = app(State::default(), screens);
    capture(&mut app, 1);

    {
        let mut state = app.state_mut();
        state.second_screen = true;
        state.focus.request(key(3));
    }
    app.finalize_view();
    assert_eq!(app.focused_key(), Some(key(3)));
    // The captive dial was unmounted, not released.
    assert_eq!(app.state().exits, exits(&[]));

    // The first screen's dials are gone.
    app.focus(key(0));
    assert_eq!(app.focused_key(), Some(key(3)));
}

fn fitting(_: &State) -> impl View<(), State> + use<> {
    ViewThatFits::new(
        FitAxis::Vertical,
        VStack::new((dial(10), Rectangle.frame_sized(50, 100))),
    )
    .or(dial(11))
}

#[test]
fn a_request_finds_its_target_in_the_choice_now_shown() {
    let mut app = app(State::default(), fitting);
    app.focus(key(10));
    assert_eq!(app.focused_key(), Some(key(10)));

    app.resize(Size::new(100, 40));
    app.finalize_view();
    app.focus(key(10));
    assert_ne!(app.focused_key(), Some(key(10)));
    app.focus(key(11));
    assert_eq!(app.focused_key(), Some(key(11)));
}

/// Dials in the middle of the screen, a popover between them whose overlay
/// holds a list of dials.
fn with_popover(state: &State) -> impl View<(), State> + use<> {
    let rows = state.rows;
    VStack::new((
        dial(0),
        Button::new(|_: &mut State| {}, |_| Circle)
            .frame_sized(20, 20)
            .focus_key(key(1))
            .popover(state.open.then_some(&()), move |()| {
                ForEach::<4>::new_vertical(rows, |&row| dial(row)).keyed(|&row| key(row))
            })
            .frame_sized(100, 100),
        dial(2),
    ))
}

#[test]
fn an_open_popover_blocks_keys_on_either_side_of_it() {
    let state = State {
        open: true,
        rows: &[20, 21],
        ..State::default()
    };
    let mut app = app(state, with_popover);
    app.force_rebuild();
    assert_eq!(app.focused_key(), Some(key(20)));
    capture(&mut app, 21);

    for blocked in [0, 1, 2] {
        app.focus(key(blocked));
        assert_eq!(app.focused_key(), Some(key(21)));
    }
    assert!(is_captive(&mut app, 21));
    assert_eq!(app.state().exits, exits(&[]));
}

#[test]
fn opening_a_popover_takes_focus_from_a_captive_sibling() {
    for (sibling, rows) in [(0, &[20, 21][..]), (2, &[20, 21][..]), (0, &[][..])] {
        let state = State {
            rows,
            ..State::default()
        };
        let mut app = app(state, with_popover);
        capture(&mut app, sibling);

        app.state_mut().open = true;
        app.finalize_view();
        assert_eq!(app.focused_key(), rows.first().copied().map(key));
        assert_eq!(app.state().exits, exits(&[(sibling as usize, 1)]));

        // The sibling receives nothing more.
        _ = app.next();
        _ = app.previous();
        assert_eq!(app.state().turns, exits(&[]));
        assert_eq!(app.state().exits, exits(&[(sibling as usize, 1)]));
    }
}

#[test]
fn deleting_a_captive_row_in_an_open_popover_focuses_the_nearest() {
    let state = State {
        open: true,
        rows: &[20, 21, 22],
        ..State::default()
    };
    let mut app = app(state, with_popover);
    app.force_rebuild();
    capture(&mut app, 21);

    app.state_mut().rows = &[20, 22];
    app.finalize_view();
    assert_eq!(app.focused_key(), Some(key(22)));
    assert_eq!(app.state().exits, exits(&[]));
}

fn request_button(_: &State) -> impl View<(), State> + use<> {
    VStack::new((
        Button::new(|s: &mut State| s.focus.request(key(2)), |_| Circle)
            .frame_sized(20, 20)
            .focus_key(key(0)),
        dial(1),
        dial(2),
    ))
}

#[test]
fn a_button_action_can_request_focus() {
    let mut app = app(State::default(), request_button);
    assert_eq!(app.focus_forward().key(), Some(key(0)));
    assert!(app.select().is_handled());
    app.finalize_view();
    assert_eq!(app.focused_key(), Some(key(2)));
    assert_eq!(app.state().focus.current(), Some(key(2)));
}

#[derive(Default)]
struct Stack {
    screens: heapless::Vec<FocusState, 4>,
}

fn top(stack: &mut Stack) -> &mut FocusState {
    stack.screens.last_mut().unwrap()
}

fn push_screen(_: &Stack) -> impl View<(), Stack> + use<> {
    let row = |k: u32| {
        Button::new(
            |s: &mut Stack| _ = s.screens.push(FocusState::requesting(key(21))),
            |_| Rectangle,
        )
        .frame_sized(20, 20)
        .focus_key(key(k))
    };
    VStack::new((row(0), row(1)))
}

fn stacked(stack: &Stack) -> impl View<(), Stack> + use<> {
    match_view!(stack.screens.len() > 1, {
        false => push_screen(stack),
        true => VStack::new((
            Button::new(|_: &mut Stack| {}, |_| Circle).frame_sized(20, 20).focus_key(key(20)),
            Button::new(|_: &mut Stack| {}, |_| Circle).frame_sized(20, 20).focus_key(key(21)),
        )),
    })
}

#[test]
fn an_action_that_pushes_a_screen_does_not_record_into_it() {
    let mut stack = Stack::default();
    _ = stack.screens.push(FocusState::new());
    let mut app = App::new(stack, Size::new(100, 200), stacked)
        .with_roles(Role::Button)
        .with_focus_binding(top);
    app.focus(key(1));
    assert!(app.select().is_handled());
    app.finalize_view();

    assert_eq!(app.focused_key(), Some(key(21)));
    assert_eq!(app.state().screens[0].current(), Some(key(1)));
    assert_eq!(app.state().screens[1].current(), Some(key(21)));
}

/// Each dial grows while it is focused.
fn growing(state: &State) -> impl View<(), State> + use<> {
    let height = |k: u32| {
        if state.focus.current() == Some(key(k)) {
            60
        } else {
            20
        }
    };
    VStack::new((
        dial(0).frame_sized(50, height(0)),
        dial(1).frame_sized(50, height(1)),
    ))
}

fn span(shape: &ContentShape) -> (i32, i32) {
    match shape {
        ContentShape::Rectangle(rect) => (rect.origin.y, rect.origin.y + rect.size.height as i32),
        other => panic!("unexpected focus shape {other:?}"),
    }
}

#[test]
fn views_depending_on_focus_settle_before_the_next_input() {
    let mut app = app(State::default(), growing);
    app.focus(key(0));
    assert_eq!(span(app.focus_shape()), (20, 40));

    // Moving focus regrows both dials before the next input arrives.
    assert_eq!(app.next().key(), Some(key(1)));
    app.finalize_view();
    assert_eq!(app.state().focus.current(), Some(key(1)));
    assert_eq!(span(app.focus_shape()), (40, 60));

    assert_eq!(app.previous().key(), Some(key(0)));
    app.finalize_view();
    assert_eq!(span(app.focus_shape()), (20, 40));
}

static ROWS: [u32; 8] = [0, 1, 2, 3, 4, 5, 6, 7];

fn scrolling(_: &State) -> impl View<(), State> + use<> {
    ScrollView::new(
        ForEach::<8>::new_vertical(&ROWS, |_| {
            Button::new(|_: &mut State| {}, |_| Rectangle).frame_sized(50, 40)
        })
        .keyed(|&row| key(row)),
    )
    .with_bar_visibility(ScrollBarVisibility::Never)
    .frame_sized(50, 100)
}

#[test]
fn a_jump_into_a_scroll_view_brings_the_target_into_view() {
    let mut app = app(State::default(), scrolling);
    app.focus(key(0));

    app.focus(key(7));
    assert_eq!(app.focused_key(), Some(key(7)));
    let (top, bottom) = span(app.focus_shape());
    assert!(top >= 0 && bottom <= 100, "row 7 spans {top}..{bottom}");

    // Even after scrolling by hand.
    assert!(!app.send(Event::Scroll(Point::new(0, 400))).is_handled());
    app.finalize_view();
    app.focus(key(6));
    assert_eq!(app.focused_key(), Some(key(6)));
    let (top, bottom) = span(app.focus_shape());
    assert!(top >= 0 && bottom <= 100, "row 6 spans {top}..{bottom}");
}

fn pinned_history(state: &State) -> impl View<(), State> + use<> {
    match_view!(state.second_screen, {
        false => Button::new(|_: &mut State| {}, |_| Circle)
            .frame_sized(20, 20)
            .focus_key(key(8)),
        true => VStack::new((
            ScrollView::new(
                ForEach::<8>::new_vertical(&ROWS[..5], |_| {
                    Button::new(|_: &mut State| {}, |_| Rectangle).frame_sized(50, 40)
                })
                .keyed(|&row| key(row)),
            )
            .with_bar_visibility(ScrollBarVisibility::Never)
            .with_initial_pin_to_end(true)
            .frame_sized(50, 100),
            Button::new(|_: &mut State| {}, |_| Circle)
                .frame_sized(50, 20)
                .focus_key(key(9)),
        )),
    })
}

#[test]
fn a_met_request_does_not_pass_through_the_screen_first() {
    let mut app = App::new(State::default(), Size::new(50, 120), pinned_history)
        .with_roles(Role::Button)
        .with_focus_binding(bind);
    app.focus(key(8));

    // A new screen opens with focus requested below its history, which
    // must stay pinned to its end rather than follow focus to its top.
    {
        let mut state = app.state_mut();
        state.second_screen = true;
        state.focus.request(key(9));
    }
    app.finalize_view();
    assert_eq!(app.focused_key(), Some(key(9)));
    app.tap(Point::new(25, 90));
    app.finalize_view();
    assert_eq!(app.focused_key(), Some(key(4)));
}
