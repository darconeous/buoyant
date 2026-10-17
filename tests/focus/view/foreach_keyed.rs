//! Keyed `ForEach`: focus and each item's state follow the item.

use buoyant::{
    app::{App, Harness as _},
    focus::{FocusKey, Role},
    primitives::Size,
    view::{
        prelude::*,
        rotary::{RotaryEvent, RotaryState},
    },
};

#[derive(Default)]
struct State {
    /// Item IDs, in display order.
    items: heapless::Vec<u32, 8>,
    /// Rotary events, with the ID of the item that saw each.
    events: heapless::Vec<(u32, &'static str), 16>,
}

/// The slots `ForEach` iterates; each view reads its item from a copy of
/// the IDs, so it borrows nothing from the state.
static SLOTS: [usize; 8] = [0, 1, 2, 3, 4, 5, 6, 7];

fn with_items(ids: &[u32]) -> State {
    State {
        items: ids.iter().copied().collect(),
        ..State::default()
    }
}

fn buttons(state: &State) -> impl View<(), State> + use<> {
    let ids = state.items.clone();
    let keys = ids.clone();
    ForEach::<8>::new_vertical(&SLOTS[..ids.len()], move |_| {
        Button::new(|_: &mut State| {}, |_| Rectangle).frame_sized(20, 20)
    })
    .keyed(move |&slot| FocusKey(keys[slot]))
}

fn rotaries(state: &State) -> impl View<(), State> + use<> {
    let ids = state.items.clone();
    let keys = ids.clone();
    ForEach::<8>::new_vertical(&SLOTS[..ids.len()], move |&slot| {
        let id = ids[slot];
        Rotary::new(
            move |s: &mut State, event: RotaryEvent| {
                let name = match event {
                    RotaryEvent::Next => "next",
                    RotaryEvent::Previous => "previous",
                    RotaryEvent::Select => "select",
                    RotaryEvent::Exit => "exit",
                };
                _ = s.events.push((id, name));
            },
            |_: RotaryState| Rectangle.frame_sized(20, 20),
        )
    })
    .keyed(move |&slot| FocusKey(keys[slot]))
}

#[test]
fn a_growing_list_takes_focus_on_its_new_items() {
    let mut app =
        App::new(with_items(&[10]), Size::new(100, 200), buttons).with_roles(Role::Button);
    assert_eq!(app.focus_forward().key(), Some(FocusKey(10)));

    app.state_mut().items = [10, 20, 30].into_iter().collect();
    app.finalize_view();
    assert_eq!(app.focused_key(), Some(FocusKey(10)));
    assert_eq!(app.next().key(), Some(FocusKey(20)));
    assert_eq!(app.next().key(), Some(FocusKey(30)));
}

#[test]
fn focus_follows_an_item_that_moves() {
    let mut app =
        App::new(with_items(&[10, 20, 30]), Size::new(100, 200), buttons).with_roles(Role::Button);
    assert!(app.focus_forward().is_handled());
    assert_eq!(app.next().key(), Some(FocusKey(20)));

    app.state_mut().items = [20, 10, 30].into_iter().collect();
    app.finalize_view();
    assert_eq!(app.focused_key(), Some(FocusKey(20)));
    assert_eq!(app.next().key(), Some(FocusKey(10)));
}

#[test]
fn a_captive_control_stays_captive_when_its_item_moves() {
    let mut app =
        App::new(with_items(&[10, 20, 30]), Size::new(100, 200), rotaries).with_roles(Role::Button);
    assert!(app.focus_forward().is_handled());
    assert!(app.next().is_handled());
    assert_eq!(app.focused_key(), Some(FocusKey(20)));
    // Capture the second item's rotary.
    assert!(app.select().is_handled());
    app.finalize_view();

    app.state_mut().items = [30, 10, 20].into_iter().collect();
    app.finalize_view();
    assert_eq!(app.focused_key(), Some(FocusKey(20)));

    // Still captive: a turn reaches the same item instead of moving focus.
    assert!(app.next().is_handled());
    assert_eq!(app.focused_key(), Some(FocusKey(20)));
    assert_eq!(app.state().events.as_slice(), &[(20, "next")]);
}

#[test]
fn removing_the_focused_last_item_focuses_the_one_before() {
    let mut app =
        App::new(with_items(&[10, 20, 30]), Size::new(100, 200), buttons).with_roles(Role::Button);
    assert!(app.focus_forward().is_handled());
    assert!(app.next().is_handled());
    assert_eq!(app.next().key(), Some(FocusKey(30)));

    app.state_mut().items = [10, 20].into_iter().collect();
    app.finalize_view();
    assert_eq!(app.focused_key(), Some(FocusKey(20)));
}

#[test]
fn removing_a_focused_item_focuses_the_one_in_its_place() {
    let mut app =
        App::new(with_items(&[10, 20, 30]), Size::new(100, 200), buttons).with_roles(Role::Button);
    assert!(app.focus_forward().is_handled());
    assert_eq!(app.next().key(), Some(FocusKey(20)));

    app.state_mut().items = [10, 30].into_iter().collect();
    app.finalize_view();
    assert_eq!(app.focused_key(), Some(FocusKey(30)));
}

#[test]
fn focus_returns_when_an_emptied_list_fills_again() {
    let mut app =
        App::new(with_items(&[10, 20]), Size::new(100, 200), buttons).with_roles(Role::Button);
    assert!(app.focus_forward().is_handled());
    assert_eq!(app.next().key(), Some(FocusKey(20)));

    app.state_mut().items = heapless::Vec::new();
    app.finalize_view();
    assert_eq!(app.focused_key(), None);

    app.state_mut().items = [40, 50].into_iter().collect();
    app.finalize_view();
    assert_eq!(app.focused_key(), Some(FocusKey(40)));
}
