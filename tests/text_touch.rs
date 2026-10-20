//! Text and images are there to be seen: a touch on them belongs to the views
//! beneath and around them.

use buoyant::{
    app::{App, Harness as _},
    focus::Role,
    font::CharacterBufferFont,
    primitives::{Point, Size},
    view::prelude::*,
};

static FONT: CharacterBufferFont = CharacterBufferFont;

#[derive(Debug, Default, PartialEq, Eq)]
struct Taps(u8);

fn captioned(_: &Taps) -> impl View<(), Taps> + use<> {
    ZStack::new((
        Button::new(|taps: &mut Taps| taps.0 += 1, |_| Rectangle).frame_sized(6, 1),
        Text::new("hi", &FONT),
    ))
}

#[test]
fn a_tap_on_text_over_a_button_presses_the_button() {
    let mut app = App::new(Taps::default(), Size::new(6, 1), captioned).with_roles(Role::Button);
    app.tap(Point::new(2, 0));
    assert_eq!(*app.state(), Taps(1));
}
