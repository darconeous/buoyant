//! `on_key` handles the key presses its focused subtree leaves unhandled,
//! and can send one on to where it moves focus.

use buoyant::{
    app::{App, Harness as _},
    event::{Key, KeyResponse},
    focus::{FocusKey, FocusState, Role},
    primitives::Size,
    view::prelude::*,
};

#[derive(Default)]
struct State {
    focus: FocusState,
    /// Keys the screen's handler saw.
    screen: heapless::Vec<Key, 8>,
    /// Keys the field's handler saw.
    field: heapless::Vec<Key, 8>,
    /// Where the screen sends characters.
    target: u32,
}

const LIST: FocusKey = FocusKey(0);
const FIELD: FocusKey = FocusKey(1);

fn bind(state: &mut State) -> &mut FocusState {
    &mut state.focus
}

fn view(_: &State) -> impl View<(), State> + use<> {
    VStack::new((
        Button::new(|_: &mut State| {}, |_| Rectangle)
            .frame_sized(50, 20)
            .focus_key(LIST),
        Button::new(|_: &mut State| {}, |_| Circle)
            .frame_sized(20, 20)
            .focus_key(FIELD)
            .on_key(|key, s: &mut State| {
                _ = s.field.push(key);
                match key {
                    Key::Character(_) => KeyResponse::Handled,
                    _ => KeyResponse::Ignored,
                }
            }),
    ))
    .on_key(|key, s: &mut State| {
        _ = s.screen.push(key);
        match key {
            Key::Character(_) => {
                s.focus.request(FocusKey(s.target));
                KeyResponse::Redirect
            }
            Key::Enter | Key::Backspace => KeyResponse::Handled,
            _ => KeyResponse::Ignored,
        }
    })
}

fn app<V: View<(), State>>(target: u32, view: fn(&State) -> V) -> App<V, State, fn(&State) -> V> {
    let state = State {
        target,
        ..State::default()
    };
    let mut app = App::new(state, Size::new(100, 100), view)
        .with_roles(Role::Button)
        .with_focus_binding(bind);
    app.focus(LIST);
    app
}

#[test]
fn a_press_and_release_act_once() {
    let mut app = app(FIELD.0, view);
    for key in [Key::Enter, Key::Backspace] {
        assert!(app.key_down(key).is_handled());
        assert!(!app.key_up(key).is_handled());
        app.finalize_view();
    }
    assert_eq!(app.state().screen, [Key::Enter, Key::Backspace]);
}

#[test]
fn the_focused_element_sees_a_key_first() {
    let mut app = app(FIELD.0, view);
    app.focus(FIELD);
    assert!(app.key_down(Key::Character('a')).is_handled());
    assert_eq!(app.state().field, [Key::Character('a')]);
    assert!(app.state().screen.is_empty());

    // What the field ignores goes on to the screen.
    assert!(app.key_down(Key::Enter).is_handled());
    assert_eq!(app.state().screen, [Key::Enter]);
}

#[test]
fn a_redirected_key_reaches_its_target_once() {
    let mut app = app(FIELD.0, view);
    assert!(app.key_down(Key::Character('x')).is_handled());
    assert_eq!(app.focused_key(), Some(FIELD));
    assert_eq!(app.state().field, [Key::Character('x')]);
    assert_eq!(app.state().screen, [Key::Character('x')]);
}

#[test]
fn a_redirected_key_is_dropped_when_its_target_cannot_be_focused() {
    let mut app = app(9, view);
    assert!(app.key_down(Key::Character('x')).is_handled());
    assert_eq!(app.focused_key(), Some(LIST));
    assert!(app.state().field.is_empty());
    assert_eq!(app.state().screen, [Key::Character('x')]);
}

#[test]
fn a_key_redirected_to_where_focus_already_is_does_not_loop() {
    // Focus stays on the list, and the redirected key comes back to the
    // screen's handler, which redirects it again: it is dropped.
    let mut app = app(LIST.0, view);
    assert!(app.key_down(Key::Character('x')).is_handled());
    assert_eq!(app.focused_key(), Some(LIST));
    assert_eq!(
        app.state().screen,
        [Key::Character('x'), Key::Character('x')]
    );
}
