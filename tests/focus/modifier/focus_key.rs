//! Focus keys: naming focusable elements, and what App reports as focused.

use buoyant::{
    app::{App, Harness as _},
    event::{Event, Key},
    focus::{FocusKey, GROUP_1, Role},
    primitives::Size,
    render::ContentShape,
    view::{map_event::Mapping, prelude::*},
};

#[derive(Clone, Default)]
struct State {
    show_buttons: bool,
}

const A: FocusKey = FocusKey(1);
const B: FocusKey = FocusKey(2);
const INNER: FocusKey = FocusKey(3);

fn keyed_and_unkeyed(_: &State) -> impl View<(), State> + use<> {
    VStack::new((
        Button::new(|_: &mut State| {}, |_| Circle)
            .frame_sized(50, 50)
            .focus_key(A),
        Button::new(|_: &mut State| {}, |_| Rectangle).frame_sized(50, 50),
        Button::new(|_: &mut State| {}, |_| RoundedRectangle::new(5))
            .frame_sized(50, 50)
            .focus_key(B),
    ))
}

fn nested_keys(_: &State) -> impl View<(), State> + use<> {
    VStack::new((Button::new(|_: &mut State| {}, |_| Circle)
        .focus_key(INNER)
        .frame_sized(50, 50),))
    .focus_key(A)
}

/// A key that is handled without focusing anything.
fn handles_keys_unfocused(_: &State) -> impl View<(), State> + use<> {
    VStack::new((Button::new(|_: &mut State| {}, |_| Circle)
        .frame_sized(50, 50)
        .focus_key(A),))
    .map_event(|event, _: &mut State| match event {
        Event::KeyDown(_) => Mapping::Handled,
        _ => Mapping::Passthrough,
    })
}

fn buttons_when_shown(state: &State) -> impl View<(), State> + use<> {
    state.show_buttons.then(|| {
        Button::new(|_: &mut State| {}, |_| Circle)
            .frame_sized(50, 50)
            .focus_key(A)
    })
}

#[test]
fn reports_the_key_of_the_focused_element() {
    let mut app =
        App::new(State::default(), Size::new(100, 200), keyed_and_unkeyed).with_roles(Role::Button);

    assert_eq!(app.focused_key(), None);
    assert_eq!(app.focus_forward().key(), Some(A));
    assert_eq!(app.focused_key(), Some(A));

    // An element without a key reports none.
    assert!(app.next().is_handled());
    assert_eq!(app.focused_key(), None);

    assert_eq!(app.next().key(), Some(B));
    assert_eq!(app.focused_key(), Some(B));
}

#[test]
fn the_nearest_key_wins() {
    let mut app =
        App::new(State::default(), Size::new(100, 200), nested_keys).with_roles(Role::Button);
    assert_eq!(app.focus_forward().key(), Some(INNER));
}

#[test]
fn a_key_handled_without_a_shape_leaves_focus_alone() {
    let mut app = App::new(
        State::default(),
        Size::new(100, 200),
        handles_keys_unfocused,
    )
    .with_roles(Role::Button);
    assert!(app.focus_forward().is_handled());
    assert_eq!(app.focused_key(), Some(A));
    assert!(matches!(app.focus_shape(), ContentShape::Circle(_)));

    assert!(app.key_down(Key::Character('x')).is_handled());
    assert_eq!(app.focused_key(), Some(A));
    assert!(matches!(app.focus_shape(), ContentShape::Circle(_)));
}

#[test]
fn failing_to_acquire_focus_clears_it() {
    let mut app = App::new(
        State { show_buttons: true },
        Size::new(100, 200),
        buttons_when_shown,
    )
    .with_roles(Role::Button);
    assert!(app.focus_forward().is_handled());
    assert_eq!(app.focused_key(), Some(A));

    app.state_mut().show_buttons = false;
    app.finalize_view();
    assert_eq!(app.focused_key(), None);
    assert_eq!(*app.focus_shape(), ContentShape::Empty);
}

fn in_group_one(_: &State) -> impl View<(), State> + use<> {
    VStack::new((Button::new(|_: &mut State| {}, |_| Circle)
        .frame_sized(50, 50)
        .focus_key(A),))
    .exclusive_focus(GROUP_1)
}

#[test]
fn a_rebuild_keeps_focus_in_its_group() {
    let mut app =
        App::new(State::default(), Size::new(100, 200), in_group_one).with_roles(Role::Button);
    assert_eq!(app.focus_forward_group(GROUP_1).key(), Some(A));
    assert_eq!(app.focus_group(), GROUP_1);

    app.force_rebuild();
    assert_eq!(app.focused_key(), Some(A));
    assert_eq!(app.focus_group(), GROUP_1);
}
