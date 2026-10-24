//! A lazily built list of rows of natural height, in a scroll view, holds
//! what the view shows still while its rows are measured, while rows are
//! added and removed around it, and while it forgets heights; shows every
//! row it should on every frame; and reaches both its true ends.

use core::{cell::Cell, time::Duration};

use buoyant::{
    animation::Animation,
    app::{App, Harness as _},
    environment::LayoutEnvironment,
    event::{Event, EventContext, EventResult, TouchResult},
    focus::{FocusKey, FocusState, Role, Seek, SeekResult},
    layout::{HorizontalAlignment, ResolvedLayout, VerticalAlignment},
    match_view,
    primitives::{Point, ProposedDimensions, Size},
    render::{LazyRows, Render, ScrollRenderable},
    render_target::FixedTextBuffer,
    view::{
        LazyForEach, ViewLayout, ViewMarker,
        lazy_foreach::{Extent, Natural},
        prelude::*,
        scroll_view::{ScrollBarVisibility, ScrollDirection},
    },
};
use embedded_touch::{Phase, Tool, Touch};

/// Rows the list holds at most, and identities rows can have.
const CAPACITY: usize = 256;
const IDS: usize = 512;
/// Lines the view shows.
const HEIGHT: usize = 40;
/// Rows the list builds: the shortest rows are a line tall.
const BUILT: usize = 56;
/// Heights the list remembers of rows it no longer builds.
const REMEMBERED: usize = 16;
/// What a line with nothing on it shows.
const BLANK: char = ' ';

struct State {
    focus: FocusState,
    /// Each row's identity, which keys it and colors it.
    ids: heapless::Vec<u32, CAPACITY>,
    /// Each identity's height.
    heights: [u32; IDS],
    /// How tall the list takes a row not yet built to be.
    estimate: u32,
    /// Rows from this one on take no focus: by default, all of them.
    plain_from: usize,
    /// How wide the list is.
    width: u32,
    /// Changes when the end is called for.
    generation: u32,
    /// Changes when a row's height does.
    version: u32,
    /// A list beside the shown one has been built, and is now only laid out.
    undrawn: bool,
}

impl State {
    fn new(rows: usize) -> Self {
        Self {
            focus: FocusState::new(),
            ids: (0..rows as u32).collect(),
            heights: core::array::from_fn(|id| 1 + (id as u32 * 7) % 5),
            estimate: 2,
            plain_from: 0,
            width: 4,
            generation: 0,
            version: 0,
            undrawn: false,
        }
    }

    fn height(&self, index: usize) -> u32 {
        self.heights[self.ids[index] as usize]
    }

    /// Where row `index` starts in the content as its rows truly are.
    fn top_of(&self, index: usize) -> i32 {
        (0..index).map(|i| self.height(i) as i32).sum()
    }

    fn total(&self) -> i32 {
        self.top_of(self.ids.len())
    }

    fn grow(&mut self, index: usize, by: u32) {
        let id = self.ids[index] as usize;
        self.heights[id] += by;
        self.version += 1;
    }
}

fn bind(state: &mut State) -> &mut FocusState {
    &mut state.focus
}

fn key(id: u32) -> FocusKey {
    FocusKey(id)
}

/// The color a row is drawn in, from its identity.
fn color(id: u32) -> char {
    char::from_u32(0x100 + id).unwrap()
}

/// The row at `index`, as tall as its identity says, two pixels wide at the
/// left, so a touch beside it drags the content: a button, or from
/// `plain_from` a bar that takes no focus.
fn row(index: usize, id: u32, height: u32, plain_from: usize) -> impl View<char, State> + use<> {
    match_view!(index < plain_from, {
        true => Button::new(|_: &mut State| {}, move |_| Rectangle.foreground_color(color(id)))
            .frame_sized(2, height),
        false => Rectangle.foreground_color(color(id)).frame_sized(2, height),
    })
}

/// The rows, keyed by identity, `N` built at most and the heights of `M`
/// more remembered, each `wide` pixels wide.
fn natural_rows<const N: usize, const M: usize>(
    state: &State,
    wide: u32,
) -> impl View<char, State, Renderables = LazyRows<impl Render<char> + Clone + use<N, M>, N>> + use<N, M>
{
    let (ids, heights, plain_from) = (state.ids.clone(), state.heights, state.plain_from);
    let keys = ids.clone();
    LazyForEach::<N>::new_natural(ids.len(), Natural::<M>::new(state.estimate), move |i| {
        let id = ids[i];
        row(i, id, heights[id as usize], plain_from)
            .frame()
            .with_width(wide)
    })
    .keyed(move |i| FocusKey(keys[i]))
    .with_alignment(HorizontalAlignment::Leading)
}

fn rows_of<const M: usize>(
    state: &State,
) -> impl View<char, State, Renderables = LazyRows<impl Render<char> + Clone + use<M>, BUILT>> + use<M>
{
    natural_rows::<BUILT, M>(state, 2)
}

fn scroll<V: View<char, State>>(content: V) -> ScrollView<V> {
    ScrollView::new(content)
        .with_direction(ScrollDirection::Vertical)
        .with_bar_visibility(ScrollBarVisibility::Never)
}

/// The list from its first row.
fn list(
    state: &State,
) -> impl View<
    char,
    State,
    Renderables = ScrollRenderable<LazyRows<impl Render<char> + Clone + use<>, BUILT>>,
> + use<> {
    scroll(rows_of::<REMEMBERED>(state))
}

/// The list as a chat's: from its end, held there, short content at the end,
/// and back to its end when the generation changes.
fn chat(
    state: &State,
) -> impl View<
    char,
    State,
    Renderables = ScrollRenderable<LazyRows<impl Render<char> + Clone + use<>, BUILT>>,
> + use<> {
    scroll(rows_of::<REMEMBERED>(state))
        .with_initial_pin_to_end(true)
        .with_short_content_at_end(true)
        .with_end_generation(state.generation)
}

type TestApp<V> = App<V, State, fn(&State) -> V>;

fn app<V: View<char, State>>(state: State, view: fn(&State) -> V) -> TestApp<V> {
    let mut app = App::new(state, Size::new(4, HEIGHT as u32), view)
        .with_roles(Role::Button | Role::TextEntry)
        .with_focus_binding(bind);
    app.set_time(Duration::from_secs(1));
    app.force_rebuild();
    app
}

type Buffer = FixedTextBuffer<4, HEIGHT>;

/// Draws the app `ms` after it started.
fn draw<V: View<char, State>>(app: &mut TestApp<V>, ms: u64) -> Buffer {
    app.set_time(Duration::from_millis(ms));
    let mut buffer = FixedTextBuffer::default();
    app.render_animated(&mut buffer, &BLANK);
    buffer
}

/// What each line shows: a row's identity, or nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Line {
    Row(u32),
    Blank,
}

fn lines(buffer: &Buffer) -> [Line; HEIGHT] {
    core::array::from_fn(|y| match buffer.text[y][1] {
        BLANK => Line::Blank,
        c if c as u32 >= 0x100 => Line::Row(c as u32 - 0x100),
        c => panic!("line {y} shows {c:?}"),
    })
}

/// Checks that `buffer` shows the rows as they are, each its height, one
/// after another, with nothing above the first row or below the last, and
/// returns where in the content its top line is. `None` for a row taller
/// than the view filling it, which says nothing of where it is.
#[track_caller]
fn at(state: &State, buffer: &Buffer) -> Option<i32> {
    let lines = lines(buffer);
    // Runs of lines alike: what they show, and from where up to where.
    let mut runs = heapless::Vec::<(Line, usize, usize), HEIGHT>::new();
    for (y, &line) in lines.iter().enumerate() {
        match runs.last_mut() {
            Some((was, _, end)) if *was == line => *end = y + 1,
            _ => runs.push((line, y, y + 1)).unwrap(),
        }
    }
    let index_of = |id: u32| {
        state
            .ids
            .iter()
            .position(|&row| row == id)
            .unwrap_or_else(|| panic!("row {id} is gone: {lines:?}"))
    };
    let mut first: Option<(usize, usize, usize)> = None;
    let mut previous: Option<usize> = None;
    for (slot, &(line, start, end)) in runs.iter().enumerate() {
        match line {
            Line::Blank => {
                let before = slot.checked_sub(1).map(|s| runs[s].0);
                let after = runs.get(slot + 1).map(|run| run.0);
                let fine = match (before, after) {
                    // Content on its way in or out shows nothing.
                    (None, None) => return None,
                    (None, Some(Line::Row(id))) => index_of(id) == 0,
                    (Some(Line::Row(id)), None) => index_of(id) + 1 == state.ids.len(),
                    _ => false,
                };
                assert!(fine, "blank lines {start} to {end}: {lines:?}");
            }
            Line::Row(id) => {
                let index = index_of(id);
                if let Some(previous) = previous {
                    assert_eq!(index, previous + 1, "line {start}: {lines:?}");
                }
                previous = Some(index);
                let (height, len) = (state.height(index) as usize, end - start);
                if start > 0 && end < HEIGHT {
                    assert_eq!(len, height, "row {index} at {start}: {lines:?}");
                } else {
                    assert!(len <= height, "row {index} at {start}: {lines:?}");
                }
                first.get_or_insert((index, start, end));
            }
        }
    }
    let Some((index, start, end)) = first else {
        return Some(0);
    };
    let height = state.height(index) as usize;
    if start == 0 && end == HEIGHT && height > HEIGHT {
        return None;
    }
    let top = if start == 0 {
        end as i32 - height as i32
    } else {
        start as i32
    };
    Some(state.top_of(index) - top)
}

#[track_caller]
fn complete(state: &State, buffer: &Buffer) -> i32 {
    at(state, buffer).expect("a row fills the view")
}

/// Checks that `buffer` shows the end of the content at its last line.
#[track_caller]
fn at_the_end(state: &State, buffer: &Buffer) {
    let at = complete(state, buffer);
    assert_eq!(at + HEIGHT as i32, state.total(), "{:?}", lines(buffer));
}

/// Draws the app at `ms` and checks it, as [`at`] does.
#[track_caller]
fn shown_at<V: View<char, State>>(app: &mut TestApp<V>, ms: u64) -> Option<i32> {
    let frame = draw(app, ms);
    at(app.state(), &frame)
}

/// Draws the app at `ms` and returns where its top line is, as
/// [`complete`] does.
#[track_caller]
fn top_at<V: View<char, State>>(app: &mut TestApp<V>, ms: u64) -> i32 {
    let frame = draw(app, ms);
    complete(app.state(), &frame)
}

/// Draws the app at `ms` and checks that it shows the end.
#[track_caller]
fn end_at<V: View<char, State>>(app: &mut TestApp<V>, ms: u64) {
    let frame = draw(app, ms);
    at_the_end(app.state(), &frame);
}

/// Draws the app from `from` until `to`, as [`settle`] does, and checks
/// that it comes to show the end.
#[track_caller]
fn end_after<V: View<char, State>>(app: &mut TestApp<V>, from: u64, to: u64) {
    let frame = settle(app, from, to);
    at_the_end(app.state(), &frame);
}

/// Sends a touch at `y`, beside the rows, `ms` after the app's start.
fn touch<V: View<char, State>>(app: &mut TestApp<V>, ms: u64, y: i32, phase: Phase) {
    app.set_time(Duration::from_millis(ms));
    app.send_touch(&Touch::new(0, Point::new(3, y).into(), phase, Tool::Finger));
    app.finalize_view();
}

/// Drags the content by `by`, a line a frame, checking that every frame
/// after the first moves what is shown exactly with the finger. Holds still
/// before lifting, so that nothing flings. Returns when it lifted.
#[track_caller]
fn drag<V: View<char, State>>(app: &mut TestApp<V>, ms: u64, from: i32, by: i32) -> u64 {
    let step = by.signum();
    touch(app, ms, from, Phase::Started);
    touch(app, ms + 10, from + step, Phase::Moved);
    let mut last = top_at(app, ms + 10);
    let mut ms = ms + 10;
    let mut y = from + step;
    while y != from + by {
        ms += 10;
        y += step;
        touch(app, ms, y, Phase::Moved);
        let now = top_at(app, ms);
        assert_eq!(now, last - step, "{ms}: at {now}, after {last}");
        last = now;
    }
    touch(app, ms + 300, y, Phase::Moved);
    touch(app, ms + 310, y, Phase::Ended);
    ms + 310
}

/// Flings the content by about `by` a frame, five frames.
fn fling<V: View<char, State>>(app: &mut TestApp<V>, ms: u64, by: i32) -> u64 {
    let from = if by < 0 { 39 } else { 0 };
    touch(app, ms, from, Phase::Started);
    for frame in 1..=5 {
        touch(app, ms + frame * 10, from + by * frame as i32, Phase::Moved);
    }
    touch(app, ms + 60, from + by * 6, Phase::Ended);
    ms + 60
}

/// Draws a frame every 16 ms from `from` until `to`, each checked, and
/// returns the last.
#[track_caller]
fn settle<V: View<char, State>>(app: &mut TestApp<V>, from: u64, to: u64) -> Buffer {
    let mut buffer = draw(app, from);
    for ms in (from..to).step_by(16) {
        buffer = draw(app, ms);
        at(app.state(), &buffer);
    }
    buffer
}

fn built<V, R>(app: &TestApp<V>) -> heapless::Vec<usize, BUILT>
where
    V: View<char, State, Renderables = ScrollRenderable<LazyRows<R, BUILT>>>,
{
    app.render_trees().target().inner().built().collect()
}

// ─── Holding still ───────────────────────────────────────────────────────

#[test]
fn scrolling_back_through_rows_never_seen_follows_the_finger() {
    let mut app = app(State::new(150), chat);
    end_at(&mut app, 1000);
    let lifted = drag(&mut app, 1000, 0, 300);
    let rest = draw(&mut app, lifted + 16);
    at(app.state(), &rest);
    assert_eq!(draw(&mut app, lifted + 1000).text, rest.text);
    assert_eq!(draw(&mut app, lifted + 2000).text, rest.text);
}

#[test]
fn scrolling_down_through_rows_never_seen_follows_the_finger() {
    let mut app = app(State::new(150), list);
    assert_eq!(top_at(&mut app, 1000), 0);
    let lifted = drag(&mut app, 1000, 39, -300);
    let rest = draw(&mut app, lifted + 16);
    assert_eq!(draw(&mut app, lifted + 1000).text, rest.text);
}

/// The list dragged to about the middle, at rest.
fn in_the_middle<V: View<char, State>>(app: &mut TestApp<V>) -> (u64, Buffer) {
    let lifted = drag(app, 1000, 39, -200);
    let rest = settle(app, lifted, lifted + 1000);
    (lifted + 1000, rest)
}

/// The index of the row whose top line is the view's first, or failing one,
/// of the first row whose top line shows.
fn first_whole(state: &State, buffer: &Buffer) -> usize {
    let at = complete(state, buffer);
    (0..state.ids.len())
        .find(|&index| state.top_of(index) >= at)
        .unwrap()
}

#[test]
fn rows_changing_above_the_view_leave_what_it_shows_still() {
    let mut app = app(State::new(150), list);
    let (ms, rest) = in_the_middle(&mut app);
    let shown = first_whole(app.state(), &rest);

    // A row above grows.
    app.state_mut().grow(shown - 5, 4);
    app.finalize_view();
    assert_eq!(settle(&mut app, ms, ms + 500).text, rest.text);

    // A row is added at the start.
    app.state_mut().ids.insert(0, 300).unwrap();
    app.finalize_view();
    assert_eq!(settle(&mut app, ms + 500, ms + 1000).text, rest.text);

    // The first rows go.
    for _ in 0..5 {
        app.state_mut().ids.remove(0);
    }
    app.finalize_view();
    assert_eq!(settle(&mut app, ms + 1000, ms + 1500).text, rest.text);
}

#[test]
fn a_row_shown_at_the_top_going_leaves_the_rows_below_it_still() {
    let mut app = app(State::new(150), list);
    let (ms, rest) = in_the_middle(&mut app);
    let before = lines(&rest);
    let Line::Row(top) = before[0] else {
        panic!("{before:?}");
    };
    let below = before
        .iter()
        .position(|&line| line != Line::Row(top))
        .unwrap();
    let index = app.state().ids.iter().position(|&id| id == top).unwrap();
    app.state_mut().ids.remove(index);
    app.finalize_view();
    let after = lines(&settle(&mut app, ms, ms + 500));
    assert_eq!(after[below..], before[below..], "{after:?}");
}

#[test]
fn heights_forgotten_move_nothing() {
    fn forgetful(
        state: &State,
    ) -> impl View<
        char,
        State,
        Renderables = ScrollRenderable<LazyRows<impl Render<char> + Clone + use<>, BUILT>>,
    > + use<> {
        scroll(rows_of::<0>(state))
    }
    let mut app = app(State::new(150), forgetful);
    top_at(&mut app, 1000);
    let lifted = drag(&mut app, 1000, 39, -300);
    let lifted = drag(&mut app, lifted + 100, 0, 250);
    drag(&mut app, lifted + 100, 39, -200);
}

#[test]
fn a_new_width_leaves_what_is_shown_still() {
    fn narrowed(state: &State) -> impl View<char, State> + use<> {
        scroll(rows_of::<REMEMBERED>(state).frame().with_width(state.width))
    }
    let mut app = app(State::new(150), narrowed);
    let (ms, rest) = in_the_middle(&mut app);
    app.state_mut().width = 3;
    app.finalize_view();
    assert_eq!(settle(&mut app, ms, ms + 500).text, rest.text);
    app.state_mut().width = 4;
    app.finalize_view();
    assert_eq!(settle(&mut app, ms + 500, ms + 1000).text, rest.text);
}

#[test]
fn rows_animated_take_their_corrections_at_once() {
    fn animated(
        state: &State,
    ) -> impl View<char, State, Renderables = ScrollRenderable<impl Render<char> + Clone + use<>>> + use<>
    {
        scroll(
            rows_of::<REMEMBERED>(state)
                .animated(Animation::linear(Duration::from_millis(400)), state.version),
        )
    }
    let mut app = app(State::new(150), animated);
    top_at(&mut app, 1000);
    let lifted = drag(&mut app, 1000, 39, -300);
    let rest = draw(&mut app, lifted + 16);
    for ms in (lifted + 16..lifted + 600).step_by(16) {
        assert_eq!(draw(&mut app, ms).text, rest.text, "{ms}");
    }
}

// ─── The end ─────────────────────────────────────────────────────────────

#[test]
fn a_chat_opens_at_its_end_and_keeps_it_as_rows_change() {
    let mut app = app(State::new(150), chat);
    end_at(&mut app, 1000);
    assert!(!lines(&draw(&mut app, 1016)).contains(&Line::Blank));

    // The last row grows.
    let last = app.state().ids.len() - 1;
    app.state_mut().grow(last, 3);
    app.finalize_view();
    end_after(&mut app, 1100, 1600);

    // A row in view grows.
    app.state_mut().grow(last - 3, 2);
    app.finalize_view();
    end_after(&mut app, 1600, 2100);

    // A row arrives, gliding in, every frame whole.
    app.state_mut().ids.push(400).unwrap();
    app.finalize_view();
    end_after(&mut app, 2100, 3100);
}

#[test]
fn an_estimate_calling_long_content_short_opens_at_the_end() {
    let mut state = State::new(30);
    state.estimate = 1;
    let mut app = app(state, chat);
    let first = draw(&mut app, 1000);
    at_the_end(app.state(), &first);
    assert!(!lines(&first).contains(&Line::Blank), "{:?}", lines(&first));
    for ms in (1016..2000).step_by(16) {
        assert_eq!(draw(&mut app, ms).text, first.text, "{ms}");
    }
}

#[test]
fn an_estimate_calling_short_content_long_opens_at_the_end() {
    let mut state = State::new(8);
    state.estimate = 10;
    let mut app = app(state, chat);
    let first = draw(&mut app, 1000);
    at_the_end(app.state(), &first);
    let shown = lines(&first);
    let blank = HEIGHT - app.state().total() as usize;
    assert!(
        shown[..blank].iter().all(|&line| line == Line::Blank),
        "{shown:?}"
    );
    assert_eq!(shown[blank], Line::Row(0), "{shown:?}");
    for ms in (1016..2000).step_by(16) {
        assert_eq!(draw(&mut app, ms).text, first.text, "{ms}");
    }
}

#[test]
fn an_emptied_list_and_one_filled_again_show_what_they_hold() {
    let mut app = app(State::new(1), chat);
    end_at(&mut app, 1000);
    app.state_mut().ids.clear();
    app.finalize_view();
    let empty = settle(&mut app, 1000, 1500);
    assert!(lines(&empty).iter().all(|&line| line == Line::Blank));
    app.state_mut().ids.push(7).unwrap();
    app.finalize_view();
    end_after(&mut app, 1500, 2000);
    app.state_mut().ids.clear();
    app.finalize_view();
    let empty = settle(&mut app, 2000, 2500);
    assert!(lines(&empty).iter().all(|&line| line == Line::Blank));
}

#[test]
fn the_end_called_for_from_far_back_is_reached_every_frame_whole() {
    let mut app = app(State::new(150), chat);
    end_at(&mut app, 1000);
    let lifted = drag(&mut app, 1000, 0, 350);
    settle(&mut app, lifted, lifted + 500);
    app.state_mut().generation += 1;
    app.finalize_view();
    end_after(&mut app, lifted + 500, lifted + 3500);
}

#[test]
fn a_fling_across_rows_never_seen_comes_to_rest_at_either_true_end() {
    let mut app = app(State::new(150), chat);
    let first = draw(&mut app, 1000);
    at_the_end(app.state(), &first);
    let lifted = fling(&mut app, 1000, 40);
    let rest = settle(&mut app, lifted, lifted + 3000);
    assert_eq!(complete(app.state(), &rest), 0);
    let lifted = fling(&mut app, lifted + 3000, -40);
    let rest = settle(&mut app, lifted, lifted + 3000);
    at_the_end(app.state(), &rest);
}

#[test]
fn turning_past_the_last_button_shows_the_true_end_before_it_lets_go() {
    let mut state = State::new(60);
    state.plain_from = 50;
    let mut app = app(state, list);
    app.focus(key(49));
    let mut ms = 1000;
    let mut shown = settle(&mut app, ms, ms + 1000);
    ms += 1000;
    // Each turn that scrolls keeps focus where it was; the turn that lets
    // go comes only once the end shows.
    while app.next().is_handled() {
        app.finalize_view();
        assert_eq!(app.focused_key(), Some(key(49)));
        shown = settle(&mut app, ms, ms + 500);
        ms += 500;
        assert!(ms < 60_000, "never let go");
    }
    at_the_end(app.state(), &shown);
}

// ─── Focus ───────────────────────────────────────────────────────────────

#[test]
fn next_and_previous_reach_every_row_whatever_its_height() {
    let mut state = State::new(80);
    state.plain_from = CAPACITY;
    // A row taller than the view.
    state.heights[30] = 50;
    let mut app = app(state, list);
    app.focus(key(0));
    let mut ms = 1000;
    for id in 1..80 {
        assert!(app.next().is_handled(), "{id}");
        app.finalize_view();
        assert_eq!(app.focused_key(), Some(key(id)));
        settle(&mut app, ms, ms + 200);
        ms += 200;
    }
    assert!(!app.next().is_handled());
    for id in (0..79).rev() {
        assert!(app.previous().is_handled(), "{id}");
        app.finalize_view();
        assert_eq!(app.focused_key(), Some(key(id)));
        settle(&mut app, ms, ms + 200);
        ms += 200;
    }
    assert_eq!(top_at(&mut app, ms + 1000), 0);
}

#[test]
fn a_request_for_a_row_far_off_shows_all_of_it() {
    let mut state = State::new(150);
    state.plain_from = CAPACITY;
    let mut app = app(state, list);
    top_at(&mut app, 1000);
    assert!(!built(&app).contains(&120));
    app.focus(key(120));
    assert_eq!(app.focused_key(), Some(key(120)));
    let rest = settle(&mut app, 1000, 2000);
    let shown = lines(&rest);
    let count = shown.iter().filter(|&&line| line == Line::Row(120)).count();
    assert_eq!(count, app.state().height(120) as usize, "{shown:?}");
}

// ─── Lists laid out but not drawn ────────────────────────────────────────

/// A list rejected by a view that fits, ahead of the one it shows.
fn fitted(state: &State) -> impl View<char, State> + use<> {
    scroll(
        ViewThatFits::new(FitAxis::Horizontal, natural_rows::<BUILT, 0>(state, 10)).or(rows_of::<
            REMEMBERED,
        >(
            state
        )),
    )
    .with_initial_pin_to_end(true)
}

/// A hidden list beside the one shown.
fn hidden(state: &State) -> impl View<char, State> + use<> {
    scroll(ZStack::new((
        natural_rows::<BUILT, 0>(state, 2).hidden(),
        rows_of::<REMEMBERED>(state),
    )))
    .with_initial_pin_to_end(true)
}

/// The list laid out more than once, among others in a stack.
fn stacked(state: &State) -> impl View<char, State> + use<> {
    scroll(VStack::new((
        rows_of::<REMEMBERED>(state),
        Rectangle.foreground_color(color(0)).frame().with_height(0),
    )))
    .with_initial_pin_to_end(true)
}

#[test]
fn a_list_a_view_that_fits_rejects_moves_nothing() {
    let mut app = app(State::new(150), fitted);
    end_at(&mut app, 1000);
    drag(&mut app, 1000, 0, 300);
}

#[test]
fn a_hidden_list_moves_nothing() {
    let mut app = app(State::new(150), hidden);
    end_at(&mut app, 1000);
    drag(&mut app, 1000, 0, 300);
}

#[test]
fn a_list_laid_out_in_a_stack_follows_the_finger() {
    let mut app = app(State::new(150), stacked);
    end_at(&mut app, 1000);
    drag(&mut app, 1000, 0, 300);
}

/// A view that builds its content while it is drawn and, once
/// `State::undrawn`, only lays it out: a view that misses the cut.
#[derive(Debug, Clone)]
struct Undrawn<T> {
    child: T,
    undrawn: bool,
}

impl<T: ViewMarker> ViewMarker for Undrawn<T> {
    type Renderables = ();
    type Transition = T::Transition;
}

impl<T: ViewLayout<State>> ViewLayout<State> for Undrawn<T> {
    type Sublayout = T::Sublayout;
    type State = T::State;
    type FocusTree = ();

    fn transition(&self) -> Self::Transition {
        ViewLayout::transition(&self.child)
    }

    fn build_state(&self, captures: &mut State) -> Self::State {
        self.child.build_state(captures)
    }

    fn layout(
        &self,
        offer: &ProposedDimensions,
        env: &impl LayoutEnvironment,
        captures: &mut State,
        state: &mut Self::State,
    ) -> ResolvedLayout<Self::Sublayout> {
        self.child.layout(offer, env, captures, state)
    }

    fn render_tree(
        &self,
        layout: &Self::Sublayout,
        origin: Point,
        env: &impl LayoutEnvironment,
        captures: &mut State,
        state: &mut Self::State,
    ) -> Self::Renderables {
        if !self.undrawn {
            _ = self.child.render_tree(layout, origin, env, captures, state);
        }
    }

    fn handle_event(
        &self,
        _event: &Event,
        _context: &EventContext,
        _render_tree: &mut Self::Renderables,
        _captures: &mut State,
        _state: &mut Self::State,
        _focus: &mut Self::FocusTree,
    ) -> EventResult {
        EventResult::Deferred
    }

    fn handle_touch(
        &self,
        _touch: &Touch,
        _context: &EventContext,
        _render_tree: &mut Self::Renderables,
        _captures: &mut State,
        _state: &mut Self::State,
    ) -> TouchResult<Self::FocusTree> {
        TouchResult::Deferred
    }

    fn seek(
        &self,
        _seek: &Seek,
        _render_tree: &Self::Renderables,
        _state: &Self::State,
        _focus: Option<&Self::FocusTree>,
    ) -> SeekResult<Self::FocusTree> {
        SeekResult::Absent
    }
}

/// Rows of one height, shown, over a short natural list that is built
/// until `State::undrawn` and then only laid out. The natural list's rows
/// are the state's first twenty; the shown rows are the same whatever the
/// state holds.
fn beside_undrawn(state: &State) -> impl View<char, State> + use<> {
    let mut short = State::new(0);
    short.ids = state.ids.iter().copied().take(20).collect();
    short.heights = state.heights;
    short.estimate = state.estimate;
    let count = 200;
    scroll(
        ZStack::new((
            Undrawn {
                child: natural_rows::<BUILT, 0>(&short, 2),
                undrawn: state.undrawn,
            },
            LazyForEach::<BUILT>::new_vertical(count, Extent::Fixed(1), |i| {
                Rectangle
                    .foreground_color(color(i as u32))
                    .frame_sized(2, 1)
            })
            .with_alignment(HorizontalAlignment::Leading),
        ))
        .with_vertical_alignment(VerticalAlignment::Top),
    )
    .with_end_generation(state.generation)
}

/// What happens after the list beside is made undrawn.
#[derive(Clone, Copy)]
#[cfg_attr(debug_assertions, expect(dead_code))]
enum Then {
    /// Nothing, for so long.
    Wait(u64),
    /// The end is called for.
    End,
    /// The content is dragged past its start and let go.
    Spring,
    /// The content is flung.
    Fling,
}

/// What makes the list beside report a move.
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(debug_assertions, expect(dead_code))]
enum Report {
    /// Nothing: the run to compare with.
    Quiet,
    /// A row added above what it shows.
    Added,
    /// Its first row gone.
    Removed,
}

/// The frames a run of `beside_undrawn` shows: dragged down `to`, the list
/// beside made undrawn, `report` done to it, then `then`.
fn undrawn_run(report: Report, to: i32, then: Then) -> heapless::Vec<[[char; 4]; HEIGHT], 256> {
    let mut app = app(State::new(150), beside_undrawn);
    draw(&mut app, 1000);
    touch(&mut app, 1000, 39, Phase::Started);
    for (step, y) in (39 - to..39).rev().enumerate() {
        touch(&mut app, 1010 + 10 * step as u64, y, Phase::Moved);
    }
    let lifted = 1010 + 10 * to as u64 + 300;
    touch(&mut app, lifted, 39 - to, Phase::Ended);
    draw(&mut app, lifted + 500);
    app.state_mut().undrawn = true;
    app.finalize_view();
    draw(&mut app, lifted + 516);
    // The list beside now reports a move it never builds.
    match report {
        Report::Quiet => {}
        Report::Added => app.state_mut().ids.insert(0, 300).unwrap(),
        Report::Removed => _ = app.state_mut().ids.remove(0),
    }
    app.finalize_view();
    let ms = lifted + 532;
    let until = match then {
        Then::Wait(long) => ms + long,
        Then::End => {
            app.state_mut().generation += 1;
            app.finalize_view();
            ms + 2000
        }
        Then::Spring => {
            touch(&mut app, ms, 0, Phase::Started);
            touch(&mut app, ms + 10, 1, Phase::Moved);
            touch(&mut app, ms + 20, 30, Phase::Moved);
            touch(&mut app, ms + 30, 30, Phase::Ended);
            ms + 1500
        }
        Then::Fling => fling(&mut app, ms, -10) + 3000,
    };
    let mut frames = heapless::Vec::new();
    for ms in (ms..until).step_by(16) {
        _ = frames.push(draw(&mut app, ms).text);
    }
    frames
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "was not built")]
fn content_that_reports_a_move_it_does_not_build_is_caught_in_debug_builds() {
    undrawn_run(Report::Added, 10, Then::Wait(100));
}

/// Without debug assertions, the scroll view moves as it would with no
/// report: near a bound, at the end, during a glide, over many builds.
#[cfg(not(debug_assertions))]
mod undrawn {
    use super::*;

    #[track_caller]
    fn alike(report: Report, to: i32, then: Then) {
        let reported = undrawn_run(report, to, then);
        let quiet = undrawn_run(Report::Quiet, to, then);
        assert_eq!(reported.len(), quiet.len());
        for (frame, (a, b)) in reported.iter().zip(&quiet).enumerate() {
            assert_eq!(a, b, "frame {frame}");
        }
    }

    #[test]
    fn near_a_bound() {
        alike(Report::Removed, 2, Then::Wait(500));
        alike(Report::Added, 2, Then::Wait(500));
    }

    #[test]
    fn called_to_the_end() {
        alike(Report::Added, 10, Then::End);
    }

    #[test]
    fn springing_back() {
        alike(Report::Added, 10, Then::Spring);
    }

    #[test]
    fn flinging() {
        alike(Report::Added, 10, Then::Fling);
    }
}

// ─── Builds ──────────────────────────────────────────────────────────────

thread_local! {
    static BUILDS: Cell<u32> = const { Cell::new(0) };
}

/// Counts the builds of its content.
#[derive(Debug, Clone)]
struct Counted<T>(T);

impl<T: ViewMarker> ViewMarker for Counted<T> {
    type Renderables = T::Renderables;
    type Transition = T::Transition;
}

impl<T: ViewLayout<State>> ViewLayout<State> for Counted<T> {
    type Sublayout = T::Sublayout;
    type State = T::State;
    type FocusTree = T::FocusTree;

    fn transition(&self) -> Self::Transition {
        ViewLayout::transition(&self.0)
    }

    fn build_state(&self, captures: &mut State) -> Self::State {
        self.0.build_state(captures)
    }

    fn layout(
        &self,
        offer: &ProposedDimensions,
        env: &impl LayoutEnvironment,
        captures: &mut State,
        state: &mut Self::State,
    ) -> ResolvedLayout<Self::Sublayout> {
        self.0.layout(offer, env, captures, state)
    }

    fn render_tree(
        &self,
        layout: &Self::Sublayout,
        origin: Point,
        env: &impl LayoutEnvironment,
        captures: &mut State,
        state: &mut Self::State,
    ) -> Self::Renderables {
        BUILDS.set(BUILDS.get() + 1);
        self.0.render_tree(layout, origin, env, captures, state)
    }

    fn handle_event(
        &self,
        event: &Event,
        context: &EventContext,
        render_tree: &mut Self::Renderables,
        captures: &mut State,
        state: &mut Self::State,
        focus: &mut Self::FocusTree,
    ) -> EventResult {
        self.0
            .handle_event(event, context, render_tree, captures, state, focus)
    }

    fn handle_touch(
        &self,
        touch: &Touch,
        context: &EventContext,
        render_tree: &mut Self::Renderables,
        captures: &mut State,
        state: &mut Self::State,
    ) -> TouchResult<Self::FocusTree> {
        self.0
            .handle_touch(touch, context, render_tree, captures, state)
    }

    fn seek(
        &self,
        seek: &Seek,
        render_tree: &Self::Renderables,
        state: &Self::State,
        focus: Option<&Self::FocusTree>,
    ) -> SeekResult<Self::FocusTree> {
        self.0.seek(seek, render_tree, state, focus)
    }
}

#[test]
fn a_fling_builds_at_most_twice_a_frame_and_not_at_rest() {
    fn counted(state: &State) -> impl View<char, State> + use<> {
        scroll(Counted(rows_of::<REMEMBERED>(state))).with_initial_pin_to_end(true)
    }
    let mut app = app(State::new(150), counted);
    end_at(&mut app, 1000);
    let lifted = fling(&mut app, 1000, 40);
    for ms in (lifted..lifted + 3000).step_by(16) {
        let before = BUILDS.get();
        shown_at(&mut app, ms);
        let builds = BUILDS.get() - before;
        assert!(builds <= 2, "{ms}: {builds} builds");
    }
    assert!(!app.should_redraw());
    let before = BUILDS.get();
    draw(&mut app, lifted + 3100);
    assert_eq!(BUILDS.get(), before);
}
