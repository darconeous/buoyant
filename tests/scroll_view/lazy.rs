//! A lazily built list in a scroll view builds only the rows near what the
//! view shows, shows every row it should on every frame, and lets focus move
//! through and reach rows it has not built.

use core::time::Duration;

use buoyant::{
    animation::Animation,
    app::{App, Harness as _},
    focus::{self, FocusKey, FocusState, Role},
    font::CharacterBufferFont,
    layout::HorizontalAlignment,
    match_view,
    primitives::{Point, Size, UnitPoint},
    render::{LazyRows, Render, ScrollRenderable},
    render_target::FixedTextBuffer,
    view::{
        EditBuffer, LazyForEach,
        lazy_foreach::Extent,
        prelude::*,
        rotary::{RotaryEvent, RotaryState},
        scroll_view::{ScrollBarVisibility, ScrollDirection},
    },
};
use embedded_touch::{Phase, Tool, Touch};

const ROWS: usize = 200;
/// Lines the view shows.
const HEIGHT: usize = 40;
/// Rows the list builds: 16 beyond the 40 shown.
const BUILT: usize = 56;
/// What a line with nothing on it shows.
const BLANK: char = ' ';

static FONT: CharacterBufferFont = CharacterBufferFont;

struct State {
    focus: FocusState,
    /// Each row's identity, which keys it and colors it.
    ids: heapless::Vec<u32, ROWS>,
    /// Rows from this one on take no focus.
    plain_from: usize,
    /// The row that is a text field.
    field: Option<usize>,
    /// Turns and exits of each dial, by identity.
    turns: [u32; ROWS],
    exits: [u32; ROWS],
    draft: EditBuffer<heapless::String<16>>,
    /// A popover over the list is open.
    open: bool,
    /// How tall the header above the list is, and how far the view is moved.
    header: u32,
    shift: i32,
}

impl State {
    fn new() -> Self {
        Self {
            focus: FocusState::new(),
            ids: (0..ROWS as u32).collect(),
            plain_from: ROWS,
            field: None,
            turns: [0; ROWS],
            exits: [0; ROWS],
            draft: EditBuffer::default(),
            open: false,
            header: 0,
            shift: 0,
        }
    }
}

fn bind(state: &mut State) -> &mut FocusState {
    &mut state.focus
}

fn key(id: usize) -> FocusKey {
    FocusKey(id as u32)
}

/// The color a row is drawn in, from its identity.
fn color(id: u32) -> char {
    char::from_u32(0x100 + id).unwrap()
}

/// The row at `index`: a dial counting its turns and exits, a text field,
/// or, from `plain_from`, a bar that takes no focus. Each is two pixels wide
/// at the left of the list, so a touch beside it drags the content, and a
/// pixel high.
fn row(
    index: usize,
    id: u32,
    plain_from: usize,
    field: Option<usize>,
    draft: &EditBuffer<heapless::String<16>>,
) -> impl View<char, State> + use<> {
    let slot = id as usize;
    match_view!((Some(index) == field, index < plain_from), {
        (true, _) => TextField::new(draft, &FONT, |s: &mut State| Some(&mut s.draft))
            .frame_sized(2, 1),
        (false, true) => Rotary::new(
            move |s: &mut State, event: RotaryEvent| match event {
                RotaryEvent::Next => s.turns[slot] += 1,
                RotaryEvent::Exit => s.exits[slot] += 1,
                _ => {}
            },
            move |_: RotaryState| Rectangle.foreground_color(color(id)),
        )
        .frame_sized(2, 1),
        (false, false) => Rectangle.foreground_color(color(id)).frame_sized(2, 1),
    })
}

/// The rows, keyed by identity, `N` built at most.
fn rows_of<const N: usize>(
    state: &State,
) -> impl View<char, State, Renderables = LazyRows<impl Render<char> + Clone + use<N>, N>> + use<N>
{
    let (ids, plain_from, field) = (state.ids.clone(), state.plain_from, state.field);
    let keys = ids.clone();
    let draft = state.draft.clone();
    LazyForEach::<N>::new_vertical(ids.len(), Extent::Fixed(1), move |i| {
        row(i, ids[i], plain_from, field, &draft)
    })
    .keyed(move |i| FocusKey(keys[i]))
    .with_alignment(HorizontalAlignment::Leading)
}

fn scroll<V: View<char, State>>(content: V) -> ScrollView<V> {
    ScrollView::new(content)
        .with_direction(ScrollDirection::Vertical)
        .with_bar_visibility(ScrollBarVisibility::Never)
}

/// The list, the rows it builds named so that [`built`] can find them.
fn list(
    state: &State,
) -> impl View<
    char,
    State,
    Renderables = ScrollRenderable<LazyRows<impl Render<char> + Clone + use<>, BUILT>>,
> + use<> {
    scroll(rows_of::<BUILT>(state))
}

fn app<V: View<char, State>>(
    state: State,
    view: fn(&State) -> V,
) -> App<V, State, fn(&State) -> V> {
    let mut app = App::new(state, Size::new(4, HEIGHT as u32), view)
        .with_roles(Role::Button | Role::TextEntry)
        .with_focus_binding(bind);
    app.set_time(Duration::from_secs(1));
    // Focus settles on the first row, as it would before anyone touched.
    app.force_rebuild();
    app
}

type TestApp<V> = App<V, State, fn(&State) -> V>;

/// Draws the app `ms` after it started.
fn draw<V: View<char, State>>(app: &mut TestApp<V>, ms: u64) -> FixedTextBuffer<4, HEIGHT> {
    app.set_time(Duration::from_millis(ms));
    let mut buffer = FixedTextBuffer::default();
    app.render_animated(&mut buffer, &BLANK);
    buffer
}

/// What each line shows: a row's identity, a header, or nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Line {
    Row(u32),
    Other(char),
    Blank,
}

fn lines(buffer: &FixedTextBuffer<4, HEIGHT>) -> [Line; HEIGHT] {
    core::array::from_fn(|y| match buffer.text[y][1] {
        BLANK => Line::Blank,
        c if c as u32 >= 0x100 => Line::Row(c as u32 - 0x100),
        c => Line::Other(c),
    })
}

/// The first row shown, from `from` down, where every line from there
/// shows the row after the one above it, until the last row: content
/// dragged past its end shows nothing beyond.
#[track_caller]
fn rows_from(buffer: &FixedTextBuffer<4, HEIGHT>, from: usize) -> u32 {
    let lines = lines(buffer);
    let Line::Row(top) = lines[from] else {
        panic!("line {from} shows no row: {lines:?}");
    };
    for (y, line) in lines.iter().enumerate().skip(from) {
        let row = top + (y - from) as u32;
        let expected = if row < ROWS as u32 {
            Line::Row(row)
        } else {
            Line::Blank
        };
        assert_eq!(
            *line, expected,
            "line {y}, below row {top} at {from}: {lines:?}"
        );
    }
    top
}

/// The first row shown, where every line shows a row.
#[track_caller]
fn complete(buffer: &FixedTextBuffer<4, HEIGHT>) -> u32 {
    rows_from(buffer, 0)
}

/// Sends a touch at `y`, beside the rows, `ms` after the app's start.
fn touch<V: View<char, State>>(app: &mut TestApp<V>, ms: u64, y: i32, phase: Phase) {
    app.set_time(Duration::from_millis(ms));
    app.send_touch(&Touch::new(0, Point::new(3, y).into(), phase, Tool::Finger));
    app.finalize_view();
}

/// Drags the content up by about `rows` without flinging it: the touch is
/// held still before it lifts. Returns when it lifted.
fn drag_by<V: View<char, State>>(app: &mut TestApp<V>, ms: u64, rows: i32) -> u64 {
    touch(app, ms, 39, Phase::Started);
    touch(app, ms + 10, 37, Phase::Moved);
    touch(app, ms + 20, 37 - rows, Phase::Moved);
    touch(app, ms + 300, 37 - rows, Phase::Ended);
    ms + 300
}

/// The rows the list built, in its scroll view, the app's whole view.
fn built<V, R>(app: &TestApp<V>) -> heapless::Vec<usize, BUILT>
where
    V: View<char, State, Renderables = ScrollRenderable<LazyRows<R, BUILT>>>,
{
    app.render_trees().target().inner().built().collect()
}

#[test]
fn a_slow_drag_and_its_fling_show_every_row_on_every_frame() {
    let mut app = app(State::new(), list);
    assert_eq!(complete(&draw(&mut app, 1000)), 0);
    touch(&mut app, 1000, 39, Phase::Started);
    let mut last = 0;
    let mut ms = 1000;
    for y in (-120..39).rev() {
        ms += 10;
        touch(&mut app, ms, y, Phase::Moved);
        let top = complete(&draw(&mut app, ms));
        assert!(top >= last, "{ms}: {top} after {last}");
        last = top;
    }
    assert!(last > 100, "{last}");
    touch(&mut app, ms + 10, -121, Phase::Ended);
    for ms in (ms + 10..ms + 3000).step_by(16) {
        complete(&draw(&mut app, ms));
    }
}

#[test]
fn a_drag_far_in_one_move_is_built_before_it_is_drawn() {
    let mut app = app(State::new(), list);
    complete(&draw(&mut app, 1000));
    touch(&mut app, 1000, 39, Phase::Started);
    touch(&mut app, 1010, 37, Phase::Moved);
    touch(&mut app, 1020, -100, Phase::Moved);
    let top = complete(&draw(&mut app, 1020));
    assert!(top > 100, "{top}");
    // Built where it was drawn, there is nothing left to build.
    assert!(!app.should_redraw());
}

#[test]
fn a_fling_is_built_wherever_a_frame_finds_it() {
    let mut app = app(State::new(), list);
    complete(&draw(&mut app, 1000));
    touch(&mut app, 1000, 39, Phase::Started);
    touch(&mut app, 1010, 29, Phase::Moved);
    touch(&mut app, 1020, 19, Phase::Moved);
    touch(&mut app, 1030, 9, Phase::Moved);
    touch(&mut app, 1040, -1, Phase::Ended);
    // A frame soon after, one long after, and the frame at rest.
    complete(&draw(&mut app, 1045));
    let top = complete(&draw(&mut app, 1400));
    assert!(top > 30, "{top}");
    assert_eq!(complete(&draw(&mut app, 4000)), 160);
    assert!(!app.should_redraw());
}

#[test]
fn moving_near_the_edge_of_what_is_built_builds_more_for_the_next_frame() {
    let mut app = app(State::new(), list);
    complete(&draw(&mut app, 1000));
    // Rows 0 to 55 are built, the view needing more once it shows row 48.
    assert_eq!(built(&app).len(), BUILT);
    touch(&mut app, 1000, 39, Phase::Started);
    let mut ms = 1000;
    let mut y = 39;
    loop {
        ms += 10;
        y -= 1;
        touch(&mut app, ms, y, Phase::Moved);
        let top = complete(&draw(&mut app, ms));
        if top + HEIGHT as u32 > 48 {
            assert!(top + HEIGHT as u32 <= 56, "{top}");
            assert!(app.should_redraw(), "the frame at {top} asks for more");
            break;
        }
        assert!(!app.should_redraw(), "the frame at {top} asks for more");
    }
    // The next frame is drawn from rows built around it, besides the
    // focused first row.
    let top = complete(&draw(&mut app, ms + 1)) as usize;
    assert!(!app.should_redraw());
    let around: heapless::Vec<usize, BUILT> = [0].into_iter().chain(top - 7..top + 48).collect();
    assert_eq!(built(&app), around);
}

#[test]
fn next_reaches_every_row_and_the_end_only_at_the_end() {
    let mut app = app(State::new(), list);
    app.focus(key(0));
    assert_eq!(app.focused_key(), Some(key(0)));
    let mut ms = 1000;
    for row in 1..ROWS {
        assert!(app.next().is_handled(), "{row}");
        app.finalize_view();
        assert_eq!(app.focused_key(), Some(key(row)));
        ms += 20;
        complete(&draw(&mut app, ms));
    }
    let top = complete(&draw(&mut app, ms + 1000)) as usize;
    assert!((top..top + HEIGHT).contains(&(ROWS - 1)), "{top}");
    assert!(!app.next().is_handled());
    assert_eq!(app.focused_key(), Some(key(ROWS - 1)));
    for row in (0..ROWS - 1).rev() {
        assert!(app.previous().is_handled(), "{row}");
        app.finalize_view();
        assert_eq!(app.focused_key(), Some(key(row)));
        ms += 20;
        complete(&draw(&mut app, ms));
    }
}

#[test]
fn a_request_reaches_a_row_not_built() {
    let mut app = app(State::new(), list);
    complete(&draw(&mut app, 1000));
    assert!(!built(&app).contains(&150));
    app.focus(key(150));
    assert_eq!(app.focused_key(), Some(key(150)));
    // Every frame of the way there shows every row.
    let mut top = 0;
    for ms in (1000..1600).step_by(16) {
        top = complete(&draw(&mut app, ms)) as usize;
    }
    assert!((top..top + HEIGHT).contains(&150), "{top}");
}

#[test]
fn a_request_for_a_row_that_takes_no_focus_changes_nothing() {
    let mut state = State::new();
    state.plain_from = 100;
    let mut app = app(state, list);
    app.focus(key(5));
    assert!(app.select().is_handled());
    app.finalize_view();
    let before = draw(&mut app, 1500).text;
    app.focus(key(150));
    assert_eq!(app.focused_key(), Some(key(5)));
    // The captive dial was not torn down.
    assert_eq!(app.state().exits[5], 0);
    assert_eq!(draw(&mut app, 1500).text, before);
}

/// A button above the list, in the default group, and the list in group 1.
fn in_a_group(state: &State) -> impl View<char, State> + use<> {
    VStack::new((
        Button::new(|_: &mut State| {}, |_| Rectangle.foreground_color('b'))
            .frame_sized(2, 1)
            .focus_key(FocusKey(1000)),
        scroll(rows_of::<BUILT>(state)).exclusive_focus(focus::GROUP_1),
    ))
}

#[test]
fn a_request_reaches_an_unbuilt_row_in_another_group() {
    let mut app = app(State::new(), in_a_group);
    app.focus(FocusKey(1000));
    assert_eq!(app.focused_key(), Some(FocusKey(1000)));
    app.focus(key(150));
    assert_eq!(app.focused_key(), Some(key(150)));
    assert_eq!(app.focus_group(), focus::GROUP_1);
}

/// The list under a popover holding a dial.
fn under_a_popover(state: &State) -> impl View<char, State> + use<> {
    scroll(rows_of::<BUILT>(state)).popover(state.open.then_some(&()), |()| {
        Button::new(|_: &mut State| {}, |_| Rectangle.foreground_color('p'))
            .frame_sized(2, 1)
            .focus_key(FocusKey(1000))
    })
}

#[test]
fn a_request_while_a_popover_is_open_stays_in_it() {
    let mut state = State::new();
    state.open = true;
    let mut app = app(state, under_a_popover);
    app.force_rebuild();
    assert_eq!(app.focused_key(), Some(FocusKey(1000)));
    app.focus(key(150));
    assert_eq!(app.focused_key(), Some(FocusKey(1000)));
}

#[test]
fn a_captive_dial_scrolled_away_stays_built_and_captive() {
    let mut app = app(State::new(), list);
    app.focus(key(5));
    assert!(app.select().is_handled());
    app.finalize_view();
    let lifted = drag_by(&mut app, 1000, 150);
    app.force_rebuild();
    let top = complete(&draw(&mut app, lifted + 1000));
    assert!(top > 100, "{top}");
    assert!(built(&app).contains(&5), "{:?}", built(&app));
    assert_eq!(app.focused_key(), Some(key(5)));
    // Still captive: the turn is the dial's.
    assert!(app.next().is_handled());
    app.finalize_view();
    assert_eq!(app.state().turns[5], 1);
    assert_eq!(app.focused_key(), Some(key(5)));
}

#[test]
fn an_active_text_field_scrolled_away_stays_active() {
    let mut state = State::new();
    state.field = Some(5);
    let mut app = app(state, list);
    app.focus(key(5));
    assert!(app.select().is_handled());
    app.finalize_view();
    let lifted = drag_by(&mut app, 1000, 150);
    app.force_rebuild();
    draw(&mut app, lifted + 1000);
    assert!(built(&app).contains(&5), "{:?}", built(&app));
    // Active, the field keeps focus on a turn, moving its cursor.
    assert!(app.next().is_handled());
    app.finalize_view();
    assert_eq!(app.focused_key(), Some(key(5)));
}

#[test]
fn a_blurred_text_field_that_keeps_focus_stays_built() {
    let mut state = State::new();
    state.field = Some(5);
    let mut app = app(state, list);
    app.focus(key(5));
    assert!(app.select().is_handled());
    // Blurred, the active field stops editing but keeps focus.
    assert!(app.blur().is_handled());
    app.finalize_view();
    assert_eq!(app.focused_key(), Some(key(5)));
    let lifted = drag_by(&mut app, 1000, 150);
    app.force_rebuild();
    draw(&mut app, lifted + 1000);
    assert!(built(&app).contains(&5), "{:?}", built(&app));
    assert_eq!(app.focused_key(), Some(key(5)));
    // Inactive now, it lets a turn move focus on.
    assert!(app.next().is_handled());
    app.finalize_view();
    assert_eq!(app.focused_key(), Some(key(6)));
}

#[test]
fn a_turn_from_a_row_out_of_view_past_rows_that_take_no_focus_comes_back() {
    let mut state = State::new();
    state.field = Some(5);
    state.plain_from = 6;
    let mut app = app(state, list);
    app.focus(key(5));
    assert!(app.select().is_handled());
    assert!(app.blur().is_handled());
    app.finalize_view();
    let lifted = drag_by(&mut app, 1000, 140);
    app.force_rebuild();
    draw(&mut app, lifted + 1000);
    // Every row after it takes no focus: the turn ends back on it, which
    // stayed built while the rows it passed were.
    app.next();
    app.finalize_view();
    assert_eq!(app.focused_key(), Some(key(5)));
    assert!(built(&app).contains(&5), "{:?}", built(&app));
    let top = complete(&draw(&mut app, lifted + 2000));
    assert_eq!(top, 160);
}

#[test]
fn a_captive_dial_torn_down_by_a_request_is_let_go() {
    let mut app = app(State::new(), list);
    app.focus(key(5));
    assert!(app.select().is_handled());
    app.finalize_view();
    let lifted = drag_by(&mut app, 1000, 150);
    app.force_rebuild();
    draw(&mut app, lifted + 1000);
    assert!(built(&app).contains(&5));
    let shown = complete(&draw(&mut app, lifted + 1000)) as usize + 10;
    app.focus(key(shown));
    assert_eq!(app.focused_key(), Some(key(shown)));
    assert_eq!(app.state().exits[5], 1);
    app.force_rebuild();
    assert!(!built(&app).contains(&5), "{:?}", built(&app));
}

#[test]
fn the_focused_row_follows_its_key_and_is_let_go_when_it_goes() {
    let mut app = app(State::new(), list);
    app.focus(key(5));
    assert!(app.select().is_handled());
    app.finalize_view();
    let lifted = drag_by(&mut app, 1000, 150);
    app.force_rebuild();
    draw(&mut app, lifted + 1000);

    // Moved to the end of the list, the row is found there, still captive.
    let ids: heapless::Vec<u32, ROWS> = (0..ROWS as u32).filter(|&id| id != 5).chain([5]).collect();
    app.state_mut().ids = ids;
    app.finalize_view();
    assert!(built(&app).contains(&(ROWS - 1)), "{:?}", built(&app));
    assert_eq!(app.focused_key(), Some(key(5)));
    assert!(app.next().is_handled());
    assert_eq!(app.state().turns[5], 1);

    // Gone, it is no longer kept.
    app.state_mut().ids.pop();
    app.finalize_view();
    draw(&mut app, lifted + 2000);
    let top = complete(&draw(&mut app, lifted + 2000)) as usize;
    assert!(
        built(&app).iter().all(|&row| row + 30 >= top),
        "{:?}",
        built(&app)
    );
}

/// A header above the rows, inside the scroll view.
fn below_a_header(state: &State) -> impl View<char, State> + use<> {
    scroll(VStack::new((
        Rectangle.foreground_color('h').frame_sized(4, state.header),
        rows_of::<BUILT>(state),
    )))
}

/// The rows built from zero, below a header.
fn in_a_geometry_group(state: &State) -> impl View<char, State> + use<> {
    scroll(VStack::new((
        Rectangle.foreground_color('h').frame_sized(4, state.header),
        rows_of::<BUILT>(state).geometry_group(),
    )))
}

/// Shows rows through a drag, from below a header 10 lines high.
fn drag_through<V: View<char, State>>(view: fn(&State) -> V) {
    let mut state = State::new();
    state.header = 10;
    let mut app = app(state, view);
    let buffer = draw(&mut app, 1000);
    assert_eq!(lines(&buffer)[9], Line::Other('h'));
    assert_eq!(rows_from(&buffer, 10), 0);
    touch(&mut app, 1000, 39, Phase::Started);
    let mut ms = 1000;
    for y in (-120..39).rev() {
        ms += 10;
        touch(&mut app, ms, y, Phase::Moved);
        let buffer = draw(&mut app, ms);
        let from = lines(&buffer)
            .iter()
            .position(|line| matches!(line, Line::Row(_)))
            .unwrap();
        rows_from(&buffer, from);
    }
}

#[test]
fn a_list_below_a_header_shows_every_row() {
    drag_through(below_a_header);
}

#[test]
fn a_list_in_a_geometry_group_shows_every_row() {
    drag_through(in_a_geometry_group);
}

/// Two lists, one after the other.
fn two_lists(state: &State) -> impl View<char, State> + use<> {
    let second: heapless::Vec<u32, ROWS> = (0..ROWS as u32).map(|id| 1000 + id).collect();
    let keys = second.clone();
    scroll(VStack::new((
        rows_of::<BUILT>(state),
        LazyForEach::<BUILT>::new_vertical(ROWS, Extent::Fixed(1), move |i| {
            Rectangle
                .foreground_color(color(second[i]))
                .frame_sized(2, 1)
        })
        .keyed(move |i| FocusKey(keys[i]))
        .with_alignment(HorizontalAlignment::Leading),
    )))
}

#[test]
fn two_lists_one_after_the_other_show_every_row() {
    let mut app = app(State::new(), two_lists);
    complete(&draw(&mut app, 1000));
    touch(&mut app, 1000, 39, Phase::Started);
    let mut ms = 1000;
    let mut y = 39;
    // Through the end of the first and into the second.
    while y > -300 {
        ms += 10;
        y -= 3;
        touch(&mut app, ms, y, Phase::Moved);
        let lines = lines(&draw(&mut app, ms));
        assert!(
            lines.iter().all(|line| matches!(line, Line::Row(_))),
            "{ms}: {lines:?}"
        );
    }
    let lines = lines(&draw(&mut app, ms));
    assert!(
        lines
            .iter()
            .all(|line| matches!(line, Line::Row(id) if *id >= 1000)),
        "{lines:?}"
    );
}

fn scaled(state: &State) -> impl View<char, State> + use<> {
    scroll(rows_of::<BUILT>(state).scale_effect(1, UnitPoint::center()))
}

#[test]
fn a_scaled_list_builds_its_first_rows() {
    let mut app = app(State::new(), scaled);
    assert_eq!(complete(&draw(&mut app, 1000)), 0);
    assert!(!app.should_redraw());
}

/// The scroll view, moved down by the state's shift, with an animation.
fn moved(state: &State) -> impl View<char, State> + use<> {
    scroll(rows_of::<BUILT>(state))
        .offset(0, state.shift)
        .animated(Animation::linear(Duration::from_millis(400)), state.shift)
}

#[test]
fn a_scroll_view_moved_far_shows_what_it_built_all_the_way() {
    let mut app = app(State::new(), moved);
    let lifted = drag_by(&mut app, 1000, 50);
    let top = complete(&draw(&mut app, lifted + 1000));
    app.state_mut().shift = 30;
    app.set_time(Duration::from_millis(lifted + 1000));
    app.finalize_view();
    for ms in (lifted + 1000..lifted + 1500).step_by(16) {
        let buffer = draw(&mut app, ms);
        let lines = lines(&buffer);
        // Whatever of the view is on screen shows the rows it did.
        let from = lines.iter().position(|line| *line != Line::Blank).unwrap();
        assert!(lines[..from].iter().all(|line| *line == Line::Blank));
        assert_eq!(rows_from(&buffer, from), top, "{ms}");
    }
}

/// The scroll view below a header, the two in a geometry group, with an
/// animation of the header's height.
fn below_a_growing_header(state: &State) -> impl View<char, State> + use<> {
    VStack::new((
        Rectangle.foreground_color('h').frame_sized(4, state.header),
        scroll(rows_of::<BUILT>(state)).geometry_group(),
    ))
    .animated(Animation::linear(Duration::from_millis(400)), state.header)
}

/// The rows below a header within the scroll view, with an animation of
/// the header's height.
fn below_a_growing_inner_header(state: &State) -> impl View<char, State> + use<> {
    scroll(
        VStack::new((
            Rectangle.foreground_color('h').frame_sized(4, state.header),
            rows_of::<BUILT>(state),
        ))
        .animated(Animation::linear(Duration::from_millis(400)), state.header),
    )
}

#[test]
fn a_scroll_view_moved_by_a_growing_header_shows_every_row() {
    let mut app = app(State::new(), below_a_growing_header);
    let lifted = drag_by(&mut app, 1000, 50);
    let top = complete(&draw(&mut app, lifted + 1000));
    app.state_mut().header = 30;
    app.set_time(Duration::from_millis(lifted + 1000));
    app.finalize_view();
    // The view, shorter now, shows the rows it did from its top.
    for ms in (lifted + 1000..lifted + 1500).step_by(16) {
        let lines = lines(&draw(&mut app, ms));
        let from = lines
            .iter()
            .position(|line| matches!(line, Line::Row(_)))
            .unwrap();
        let shown = lines[from..]
            .iter()
            .take_while(|line| matches!(line, Line::Row(_)))
            .count();
        assert!(shown >= 10, "{ms}: {lines:?}");
        for (y, line) in lines[from..from + shown].iter().enumerate() {
            assert_eq!(*line, Line::Row(top + y as u32), "{ms}: {lines:?}");
        }
        assert!(
            lines[from + shown..]
                .iter()
                .all(|line| *line == Line::Blank)
        );
    }
}

#[test]
fn a_list_below_a_growing_header_takes_its_place_at_once() {
    let mut app = app(State::new(), below_a_growing_inner_header);
    let lifted = drag_by(&mut app, 1000, 50);
    let top = complete(&draw(&mut app, lifted + 1000));
    app.state_mut().header = 30;
    app.set_time(Duration::from_millis(lifted + 1000));
    app.finalize_view();
    // The rows are 30 lines lower at once, and stay there as the header
    // grows into the room left above them.
    for ms in (lifted + 1000..lifted + 1500).step_by(16) {
        let buffer = draw(&mut app, ms);
        let from = lines(&buffer)
            .iter()
            .position(|line| matches!(line, Line::Row(_)))
            .unwrap();
        assert_eq!(rows_from(&buffer, from), top + from as u32 - 30, "{ms}");
    }
}

/// Rows four lines high, removed with an animation.
fn tall_rows(state: &State) -> impl View<char, State> + use<> {
    let ids = state.ids.clone();
    let (keys, count) = (ids.clone(), ids.len());
    scroll(
        LazyForEach::<BUILT>::new_vertical(count, Extent::Fixed(4), move |i| {
            Rectangle.foreground_color(color(ids[i])).frame_sized(2, 4)
        })
        .keyed(move |i| FocusKey(keys[i]))
        .with_alignment(HorizontalAlignment::Leading)
        .animated(Animation::linear(Duration::from_millis(400)), count),
    )
}

#[test]
fn rows_animate_by_identity() {
    let mut app = app(State::new(), tall_rows);
    draw(&mut app, 1000);
    app.state_mut().ids.remove(0);
    app.set_time(Duration::from_secs(1));
    app.finalize_view();
    // Halfway, row 1 is halfway up from where it was.
    let lines = lines(&draw(&mut app, 1200));
    assert_eq!(lines[2], Line::Row(1), "{lines:?}");
    assert_eq!(lines[6], Line::Row(2), "{lines:?}");
}

fn empty(_: &State) -> impl View<char, State> + use<> {
    scroll(LazyForEach::<BUILT>::new_vertical(
        0,
        Extent::Fixed(1),
        |_| Rectangle.foreground_color('x').frame_sized(2, 1),
    ))
}

#[test]
fn an_empty_list_draws_nothing_and_asks_for_nothing() {
    let mut app = app(State::new(), empty);
    assert!(
        lines(&draw(&mut app, 1000))
            .iter()
            .all(|line| *line == Line::Blank)
    );
    assert!(!app.should_redraw());
}

/// The rows below a header taller than the view.
fn out_of_view(state: &State) -> impl View<char, State> + use<> {
    scroll(VStack::new((
        Rectangle.foreground_color('h').frame_sized(4, 100),
        rows_of::<BUILT>(state),
    )))
}

#[test]
fn a_list_out_of_view_asks_for_nothing() {
    let mut app = app(State::new(), out_of_view);
    assert!(
        lines(&draw(&mut app, 1000))
            .iter()
            .all(|line| *line == Line::Other('h'))
    );
    assert!(!app.should_redraw());
    draw(&mut app, 1100);
    assert!(!app.should_redraw());
}

fn unscrolled(state: &State) -> impl View<char, State> + use<> {
    rows_of::<BUILT>(state)
}

#[test]
fn a_list_outside_a_scroll_view_builds_its_first_rows() {
    let mut app = app(State::new(), unscrolled);
    assert_eq!(complete(&draw(&mut app, 1000)), 0);
    assert!(!app.should_redraw());
}

fn too_few(state: &State) -> impl View<char, State> + use<> {
    scroll(rows_of::<30>(state))
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "cannot build the 40 rows it shows")]
fn a_list_built_too_small_panics_in_debug_builds() {
    let mut app = app(State::new(), too_few);
    draw(&mut app, 1000);
}

#[cfg_attr(debug_assertions, allow(dead_code))]
fn just_one(state: &State) -> impl View<char, State> + use<> {
    scroll(rows_of::<1>(state))
}

/// Built too small, a release build leaves rows out, keeps the focused one,
/// and asks for nothing more.
#[cfg(not(debug_assertions))]
#[test]
fn a_list_built_too_small_leaves_rows_out() {
    let mut app = app(State::new(), too_few);
    app.focus(key(5));
    let lifted = drag_by(&mut app, 1000, 100);
    app.force_rebuild();
    let lines = lines(&draw(&mut app, lifted + 1000));
    // The middle rows are drawn, the edges not.
    assert!(matches!(lines[20], Line::Row(_)), "{lines:?}");
    assert_eq!(lines[0], Line::Blank);
    assert_eq!(lines[HEIGHT - 1], Line::Blank);
    assert!(!app.should_redraw());
    // The focused row is kept, and a turn from it reaches the next.
    assert_eq!(app.focused_key(), Some(key(5)));
    assert!(app.next().is_handled());
    app.finalize_view();
    assert_eq!(app.focused_key(), Some(key(6)));
    assert!(app.previous().is_handled());
    app.finalize_view();
    assert_eq!(app.focused_key(), Some(key(5)));
}

/// With room for one row, the focused one, a turn finds no room for the
/// next.
#[cfg(not(debug_assertions))]
#[test]
fn a_list_built_for_one_row_has_no_room_for_a_turn() {
    let mut one = app(State::new(), just_one);
    assert_eq!(one.focused_key(), Some(key(0)));
    let lifted = drag_by(&mut one, 1000, 100);
    one.force_rebuild();
    draw(&mut one, lifted + 1000);
    assert!(!one.next().is_handled());
    assert_eq!(one.focused_key(), Some(key(0)));
}
