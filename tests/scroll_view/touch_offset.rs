//! Touches reach the content where they are on it, however far it has
//! scrolled.

use buoyant::{
    app::{App, Harness as _},
    focus::Role,
    primitives::{Point, Size},
    view::{
        prelude::*,
        scroll_view::{ScrollBarVisibility, ScrollDirection},
    },
};
use embedded_touch::{Phase, Tool, Touch};

#[derive(Debug, Default, PartialEq, Eq)]
struct Taps([u8; 3]);

fn list(_: &Taps) -> impl View<(), Taps> + use<> {
    ScrollView::new(VStack::new((
        Button::new(|s: &mut Taps| s.0[0] += 1, |_| Rectangle).frame_sized(4, 8),
        Button::new(|s: &mut Taps| s.0[1] += 1, |_| Rectangle).frame_sized(4, 8),
        Button::new(|s: &mut Taps| s.0[2] += 1, |_| Rectangle).frame_sized(4, 8),
    )))
    .with_direction(ScrollDirection::Vertical)
    .with_bar_visibility(ScrollBarVisibility::Never)
}

fn touch(id: u8, x: i32, y: i32, phase: Phase) -> Touch {
    Touch::new(id, Point::new(x, y).into(), phase, Tool::Finger)
}

#[test]
fn a_second_finger_taps_what_is_under_it_while_the_first_scrolls() {
    let mut app = App::new(Taps::default(), Size::new(4, 8), list).with_roles(Role::Button);
    app.send_touch(&touch(0, 1, 7, Phase::Started));
    app.send_touch(&touch(0, 1, 1, Phase::Moved));
    app.finalize_view();

    // Scrolled by 6, row 4 shows the second button.
    app.send_touch(&touch(1, 1, 4, Phase::Started));
    app.send_touch(&touch(1, 1, 4, Phase::Ended));
    app.send_touch(&touch(0, 1, 1, Phase::Ended));
    assert_eq!(*app.state(), Taps([0, 1, 0]));
}
