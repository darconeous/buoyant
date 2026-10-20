//! Content dragged and let go while moving coasts on, slowing, to rest.

use core::time::Duration;

use buoyant::{
    app::{App, Harness as _},
    focus::Role,
    primitives::{Point, Size},
    render_target::{FixedTextBuffer, RenderTarget as _},
    view::{
        prelude::*,
        scroll_view::{ScrollBarVisibility, ScrollDirection},
    },
};
use embedded_touch::{Phase, Tool, Touch};

const ROWS: usize = 200;

/// Each row's number, which its color is drawn from.
static NUMBERS: [u16; ROWS] = {
    let mut numbers = [0; ROWS];
    let mut i = 0;
    while i < ROWS {
        numbers[i] = i as u16;
        i += 1;
    }
    numbers
};

/// Rows a pixel high, each of its own color, in a view 40 rows high.
/// The list's state, of which it has none.
#[derive(Default)]
struct Rows;

fn list(_: &Rows) -> impl View<char, Rows> + use<> {
    ScrollView::new(ForEach::<ROWS>::new_vertical(&NUMBERS, |&row| {
        Rectangle
            .foreground_color(char::from_u32(0x100 + u32::from(row)).unwrap())
            .frame_sized(4, 1)
    }))
    .with_direction(ScrollDirection::Vertical)
    .with_bar_visibility(ScrollBarVisibility::Never)
}

/// The list, its touches starting a second in.
macro_rules! app {
    () => {{
        let mut app = App::new(Rows, Size::new(4, 40), list);
        app.set_time(Duration::from_secs(1));
        app
    }};
}

/// Sends a touch at `y`, `ms` after the app's start.
fn touch<V: View<char, Rows>, F: Fn(&Rows) -> V>(
    app: &mut App<V, Rows, F>,
    ms: u64,
    y: i32,
    phase: Phase,
) {
    app.set_time(Duration::from_millis(ms));
    app.send_touch(&Touch::new(0, Point::new(1, y).into(), phase, Tool::Finger));
    app.finalize_view();
}

/// The row drawn at the top `ms` after the app's start, and whether the
/// content is still moving.
fn top<V: View<char, Rows>, F: Fn(&Rows) -> V>(app: &mut App<V, Rows, F>, ms: u64) -> (u32, bool) {
    app.set_time(Duration::from_millis(ms));
    let mut buffer = FixedTextBuffer::<4, 40>::default();
    buffer.clear_animation_status();
    app.render_animated(&mut buffer, &' ');
    let moving = buffer.clear_animation_status();
    (buffer.text[0][1] as u32 - 0x100, moving)
}

/// A drag up through `ys`, a sample every 10 ms from 1000 ms, lifting at the
/// last.
fn drag<V: View<char, Rows>, F: Fn(&Rows) -> V>(app: &mut App<V, Rows, F>, ys: &[i32]) -> u64 {
    let (last, moves) = ys.split_last().unwrap();
    let mut ms = 1000;
    for (i, &y) in moves.iter().enumerate() {
        let phase = if i == 0 { Phase::Started } else { Phase::Moved };
        touch(app, ms, y, phase);
        ms += 10;
    }
    touch(app, ms, *last, Phase::Ended);
    ms
}

#[test]
fn content_let_go_while_moving_coasts_on_to_rest() {
    let mut app = app!();
    // Up 2 rows every 10 ms: 200 rows a second. The first 2 rows go to
    // telling a drag from a tap.
    let lifted = drag(&mut app, &[9, 7, 5, 3, 1]);
    assert_eq!(top(&mut app, lifted), (8, true));

    // Slowing by e every 325 ms, it coasts 65 rows in all.
    let (at, moving) = top(&mut app, lifted + 325);
    assert!(moving && (48..=50).contains(&at), "{at}");
    let mut last = at;
    let mut rest = None;
    for ms in (lifted + 330..lifted + 4000).step_by(10) {
        let (at, moving) = top(&mut app, ms);
        assert!(at >= last && at <= 73, "{ms}: {at}");
        last = at;
        if !moving && rest.is_none() {
            rest = Some(ms - lifted);
        }
    }
    assert_eq!(last, 73);
    assert!(rest.is_some_and(|ms| ms < 3000), "{rest:?}");
}

#[test]
fn content_held_still_before_it_is_let_go_stays() {
    let mut app = app!();
    touch(&mut app, 1000, 9, Phase::Started);
    touch(&mut app, 1010, 7, Phase::Moved);
    touch(&mut app, 1020, 5, Phase::Moved);
    touch(&mut app, 1030, 3, Phase::Moved);
    touch(&mut app, 1200, 3, Phase::Ended);
    assert_eq!(top(&mut app, 1200), (6, false));
    assert_eq!(top(&mut app, 2000), (6, false));
}

#[test]
fn flung_content_stops_at_its_end() {
    let mut app = app!();
    // 2000 rows a second would coast 650 rows; there are 160 to go. It
    // springs back from a little past the end.
    let lifted = drag(&mut app, &[39, 19, -1, -21]);
    let mut furthest = 0;
    for ms in (lifted..lifted + 2000).step_by(5) {
        furthest = furthest.max(top(&mut app, ms).0);
    }
    assert!((161..=172).contains(&furthest), "{furthest}");
    assert_eq!(top(&mut app, lifted + 2000), (160, false));
}

#[test]
fn a_touch_on_coasting_content_stops_it() {
    let mut app = app!();
    let lifted = drag(&mut app, &[9, 7, 5, 3, 1]);
    let (caught, moving) = top(&mut app, lifted + 100);
    assert!(moving && caught > 6, "{caught}");
    touch(&mut app, lifted + 100, 5, Phase::Started);
    touch(&mut app, lifted + 110, 5, Phase::Ended);
    assert_eq!(top(&mut app, lifted + 110), (caught, false));
    assert_eq!(top(&mut app, lifted + 2000), (caught, false));
}

/// How many times the button was pressed.
#[derive(Debug, Default, PartialEq, Eq)]
struct Taps(u8);

/// One button the whole length of the list.
fn long_button(_: &Taps) -> impl View<char, Taps> + use<> {
    ScrollView::new(
        Button::new(
            |taps: &mut Taps| taps.0 += 1,
            |_| Rectangle.foreground_color('b'),
        )
        .frame_sized(4, 200),
    )
    .with_direction(ScrollDirection::Vertical)
    .with_bar_visibility(ScrollBarVisibility::Never)
}

#[test]
fn the_touch_that_stops_coasting_content_presses_nothing() {
    let mut app = App::new(Taps::default(), Size::new(4, 40), long_button).with_roles(Role::Button);
    let send = |app: &mut App<_, Taps, _>, ms: u64, y: i32, phase: Phase| {
        app.set_time(Duration::from_millis(ms));
        app.send_touch(&Touch::new(0, Point::new(1, y).into(), phase, Tool::Finger));
        app.finalize_view();
    };
    for (i, y) in [39, 37, 35, 33].into_iter().enumerate() {
        let phase = if i == 0 { Phase::Started } else { Phase::Moved };
        send(&mut app, 1000 + 10 * i as u64, y, phase);
    }
    send(&mut app, 1040, 31, Phase::Ended);
    send(&mut app, 1100, 20, Phase::Started);
    send(&mut app, 1110, 20, Phase::Ended);
    assert_eq!(*app.state(), Taps(0));

    // At rest, a tap presses it.
    send(&mut app, 5000, 20, Phase::Started);
    send(&mut app, 5010, 20, Phase::Ended);
    assert_eq!(*app.state(), Taps(1));
}
