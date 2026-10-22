//! A scroll view keeps the focused element in view, with an optional margin
//! beyond it, and can place short content at its end.

use buoyant::{
    app::{App, Harness as _},
    event::Event,
    focus::{FocusKey, Role},
    primitives::{Point, Size},
    render::ContentShape,
    view::{prelude::*, scroll_view::ScrollBarVisibility},
};

#[derive(Default)]
struct State {
    rows: heapless::Vec<u32, 8>,
}

static SLOTS: [usize; 8] = [0, 1, 2, 3, 4, 5, 6, 7];

const ROW: u32 = 40;
const VIEW: u32 = 100;

fn with_rows(count: u32) -> State {
    State {
        rows: (0..count).collect(),
    }
}

fn list(state: &State, margin: u32, at_end: bool, pin: bool) -> impl View<(), State> + use<> {
    let ids = state.rows.clone();
    let keys = ids.clone();
    ScrollView::new(
        ForEach::<8>::new_vertical(&SLOTS[..ids.len()], |_| {
            Button::new(|_: &mut State| {}, |_| Rectangle).frame_sized(50, ROW)
        })
        .keyed(move |&slot| FocusKey(keys[slot])),
    )
    .with_bar_visibility(ScrollBarVisibility::Never)
    .with_focus_margin(margin)
    .with_short_content_at_end(at_end)
    .with_initial_pin_to_end(pin)
}

fn plain(state: &State) -> impl View<(), State> + use<> {
    list(state, 0, false, false)
}

fn with_margin(state: &State) -> impl View<(), State> + use<> {
    list(state, 10, false, false)
}

fn at_end(state: &State) -> impl View<(), State> + use<> {
    list(state, 0, true, false)
}

/// The focused element's top and bottom edges on screen.
fn span(shape: &ContentShape) -> (i32, i32) {
    match shape {
        ContentShape::Rectangle(rect) => (rect.origin.y, rect.origin.y + rect.size.height as i32),
        other => panic!("unexpected focus shape {other:?}"),
    }
}

fn app<V: View<(), State>>(state: State, view: fn(&State) -> V) -> App<V, State, fn(&State) -> V> {
    App::new(state, Size::new(50, VIEW), view).with_roles(Role::Button)
}

#[test]
fn a_focused_row_that_moves_out_of_view_is_followed() {
    let mut app = app(with_rows(5), plain);
    assert_eq!(app.focus_forward().key(), Some(FocusKey(0)));

    // The focused row moves to the end, out of view.
    app.state_mut().rows = [1, 2, 3, 4, 0].into_iter().collect();
    app.finalize_view();
    assert_eq!(app.focused_key(), Some(FocusKey(0)));
    assert_eq!(
        span(app.focus_shape()),
        (VIEW as i32 - ROW as i32, VIEW as i32)
    );
}

#[test]
fn scrolling_by_hand_is_not_undone_by_a_rebuild() {
    let mut app = app(with_rows(5), plain);
    assert!(app.focus_forward().is_handled());

    // Scroll the focused first row out of view.
    assert!(!app.send(Event::Scroll(Point::new(0, -120))).is_handled());
    app.finalize_view();
    let (top, _) = span(app.focus_shape());
    assert!(top < 0, "the first row is at {top}");

    app.force_rebuild();
    assert_eq!(span(app.focus_shape()).0, top);

    // Moving focus follows it again.
    assert!(app.next().is_handled());
    let (top, bottom) = span(app.focus_shape());
    assert!(
        top >= 0 && bottom <= VIEW as i32,
        "the second row spans {top}..{bottom}"
    );
}

#[test]
fn the_margin_keeps_part_of_the_next_row_in_view() {
    let mut app = app(with_rows(5), with_margin);
    assert!(app.focus_forward().is_handled());
    assert!(app.next().is_handled());
    assert_eq!(app.next().key(), Some(FocusKey(2)));
    // The third row stops 10 px short of the bottom edge.
    assert_eq!(span(app.focus_shape()).1, VIEW as i32 - 10);
}

#[test]
fn the_margin_never_scrolls_past_the_content() {
    let mut app = app(with_rows(5), with_margin);
    assert!(app.focus_forward().is_handled());
    for _ in 0..4 {
        assert!(app.next().is_handled());
    }
    assert_eq!(app.focused_key(), Some(FocusKey(4)));
    assert_eq!(span(app.focus_shape()).1, VIEW as i32);

    for _ in 0..4 {
        assert!(app.previous().is_handled());
    }
    assert_eq!(span(app.focus_shape()).0, 0);
}

#[test]
fn short_content_sits_at_the_end() {
    let mut app = app(with_rows(2), at_end);
    assert!(app.focus_forward().is_handled());
    assert_eq!(
        span(app.focus_shape()),
        (VIEW as i32 - 2 * ROW as i32, VIEW as i32 - ROW as i32)
    );
    assert!(app.next().is_handled());
    assert_eq!(span(app.focus_shape()).1, VIEW as i32);
}

fn pinned(state: &State) -> impl View<(), State> + use<> {
    list(state, 0, false, true)
}

#[test]
fn following_focus_away_from_the_end_unpins_the_content() {
    let mut app = App::new(with_rows(5), Size::new(50, VIEW), pinned)
        .with_roles(Role::Button)
        .with_focus_at_end();
    // Pinned to the end: the last row is at the bottom.
    assert_eq!(app.focus_backward().key(), Some(FocusKey(4)));
    assert_eq!(span(app.focus_shape()).1, VIEW as i32);

    for _ in 0..4 {
        assert!(app.previous().is_handled());
    }
    assert_eq!(span(app.focus_shape()).0, 0);

    // A rebuild keeps the first row in view rather than pinning to the end.
    app.force_rebuild();
    assert_eq!(app.focused_key(), Some(FocusKey(0)));
    assert_eq!(span(app.focus_shape()).0, 0);
}

const TALL: u32 = 250;
const STEP: u32 = 30;

/// Five rows, the second taller than the view.
fn with_tall_row(state: &State) -> impl View<(), State> + use<> {
    let keys = state.rows.clone();
    ScrollView::new(
        ForEach::<8>::new_vertical(&SLOTS[..keys.len()], |&slot| {
            let height = if slot == 1 { TALL } else { ROW };
            Button::new(|_: &mut State| {}, |_| Rectangle).frame_sized(50, height)
        })
        .keyed(move |&slot| FocusKey(keys[slot])),
    )
    .with_bar_visibility(ScrollBarVisibility::Never)
    .with_scroll_step(STEP)
}

#[test]
fn a_row_taller_than_the_view_is_stepped_through_going_down() {
    let mut app = app(with_rows(5), with_tall_row);
    assert!(app.focus_forward().is_handled());
    assert_eq!(app.next().key(), Some(FocusKey(1)));
    // Entered from above: its top is at the top.
    assert_eq!(span(app.focus_shape()), (0, TALL as i32));

    let mut bottom = TALL as i32;
    while bottom > VIEW as i32 {
        assert_eq!(app.next().key(), Some(FocusKey(1)));
        let (_, now) = span(app.focus_shape());
        assert_eq!(now, (bottom - STEP as i32).max(VIEW as i32));
        bottom = now;
    }
    // Its last line is in view, so focus moves on.
    assert_eq!(app.next().key(), Some(FocusKey(2)));
}

#[test]
fn a_row_taller_than_the_view_is_stepped_through_going_up() {
    let mut app = app(with_rows(5), with_tall_row);
    assert!(app.focus_forward().is_handled());
    while app.focused_key() != Some(FocusKey(2)) {
        assert!(app.next().is_handled());
    }
    assert_eq!(app.previous().key(), Some(FocusKey(1)));
    // Entered from below: its bottom is at the bottom.
    assert_eq!(span(app.focus_shape()).1, VIEW as i32);

    let mut steps = 0;
    while span(app.focus_shape()).0 < 0 {
        assert_eq!(app.previous().key(), Some(FocusKey(1)));
        steps += 1;
    }
    assert_eq!(steps, (TALL - VIEW).div_ceil(STEP));
    assert_eq!(app.previous().key(), Some(FocusKey(0)));
}

const BESIDE: u32 = 60;

/// Three rows between content that takes no focus, at most `step` at a
/// time, with focus kept from leaving.
fn between(state: &State, step: u32) -> impl View<(), State> + use<> {
    let keys = state.rows.clone();
    ScrollView::new(VStack::new((
        Rectangle.frame_sized(50, BESIDE),
        ForEach::<8>::new_vertical(&SLOTS[..keys.len()], |_| {
            Button::new(|_: &mut State| {}, |_| Rectangle).frame_sized(50, ROW)
        })
        .keyed(move |&slot| FocusKey(keys[slot])),
        Rectangle.frame_sized(50, BESIDE),
    )))
    .with_bar_visibility(ScrollBarVisibility::Never)
    .with_scroll_step(step)
    .bound_focus(buoyant::focus::BoundaryBehavior::Stop)
}

fn between_at_once(state: &State) -> impl View<(), State> + use<> {
    between(state, 0)
}

fn between_by_step(state: &State) -> impl View<(), State> + use<> {
    between(state, 20)
}

#[test]
fn turning_past_the_last_row_shows_what_follows() {
    let mut app = app(with_rows(3), between_at_once);
    assert!(app.focus_forward().is_handled());
    assert_eq!(app.next().key(), Some(FocusKey(1)));
    assert_eq!(app.next().key(), Some(FocusKey(2)));
    assert_eq!(span(app.focus_shape()).1, VIEW as i32);

    // Without a step, the rest scrolls into view at once; focus stays.
    assert_eq!(app.next().key(), Some(FocusKey(2)));
    assert_eq!(span(app.focus_shape()).1, VIEW as i32 - BESIDE as i32);
    assert_eq!(app.next().key(), Some(FocusKey(2)));
    assert_eq!(span(app.focus_shape()).1, VIEW as i32 - BESIDE as i32);

    // A rebuild leaves it there.
    app.force_rebuild();
    assert_eq!(span(app.focus_shape()).1, VIEW as i32 - BESIDE as i32);
}

#[test]
fn turning_past_the_first_row_scrolls_back_a_step_at_a_time() {
    let mut app = app(with_rows(3), between_by_step);
    assert!(app.focus_forward().is_handled());
    assert_eq!(app.next().key(), Some(FocusKey(1)));
    assert_eq!(app.next().key(), Some(FocusKey(2)));
    assert_eq!(app.previous().key(), Some(FocusKey(1)));
    assert_eq!(app.previous().key(), Some(FocusKey(0)));
    // Scrolled down to the third row, the first is now at the top.
    assert_eq!(span(app.focus_shape()).0, 0);

    for top in [20, 40, 60] {
        assert_eq!(app.previous().key(), Some(FocusKey(0)));
        assert_eq!(span(app.focus_shape()).0, top);
    }
    // The content's start is in view: focus stays, and nothing moves.
    assert_eq!(app.previous().key(), Some(FocusKey(0)));
    assert_eq!(span(app.focus_shape()).0, BESIDE as i32);
}

const PAGE: u32 = 250;

/// A page taller than the view, with nothing in it to focus, under `key`.
fn page(_: &State, self_focus: bool) -> impl View<(), State> + use<> {
    ScrollView::new(Rectangle.frame_sized(50, PAGE))
        .with_bar_visibility(ScrollBarVisibility::Never)
        .with_scroll_step(STEP)
        .with_self_focus(self_focus)
        .focus_key(FocusKey(9))
}

fn read(state: &State) -> impl View<(), State> + use<> {
    page(state, true)
}

fn unread(state: &State) -> impl View<(), State> + use<> {
    page(state, false)
}

#[test]
fn content_with_nothing_to_focus_takes_focus_and_is_stepped_through() {
    let mut app = app(State::default(), read);
    assert_eq!(app.focus_forward().key(), Some(FocusKey(9)));
    assert_eq!(span(app.focus_shape()), (0, PAGE as i32));

    let mut steps = 0;
    while span(app.focus_shape()).1 > VIEW as i32 {
        assert_eq!(app.next().key(), Some(FocusKey(9)));
        steps += 1;
    }
    assert_eq!(steps, (PAGE - VIEW).div_ceil(STEP));
    // At its end, focus moves on, and there is nowhere to go.
    assert!(!app.next().is_handled());

    app.focus_backward();
    while span(app.focus_shape()).0 < 0 {
        assert_eq!(app.previous().key(), Some(FocusKey(9)));
    }
    assert_eq!(span(app.focus_shape()).0, 0);
}

#[test]
fn content_with_nothing_to_focus_takes_none_unless_asked() {
    let mut app = app(State::default(), unread);
    assert!(!app.focus_forward().is_handled());
}
