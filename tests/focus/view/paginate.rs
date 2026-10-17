use buoyant::{
    app::{App, Harness as _},
    event::Event,
    focus::{self, FocusAction, Role},
    match_view,
    primitives::{Point, Size, geometry::Circle},
    render::ContentShape,
    view::{
        paginate::PageEvent,
        prelude::*,
        rotary::{RotaryEvent, RotaryState},
    },
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Page {
    #[default]
    One,
    Two,
}

#[derive(Debug, Clone, Default)]
struct State {
    page: Page,
    one_count: u32,
    two_count: u32,
}

impl State {
    fn switch_page(&mut self, _event: PageEvent) {
        self.page = match self.page {
            Page::One => Page::Two,
            Page::Two => Page::One,
        }
    }
}

fn view(state: &State) -> impl View<(), State> + use<> {
    Paginate::new(
        focus::GROUP_1,
        State::switch_page,
        match_view!(state.page, {
            Page::One => Button::new(|s: &mut State| s.one_count += 1, |_| Circle),
            Page::Two => Button::new(|s: &mut State| s.two_count += 1, |_| Rectangle),
        })
        .bound_focus(focus::BoundaryBehavior::Wrap),
    )
}

#[test]
fn only_matching_group_paginates() {
    let state = State::default();
    let mut harness = App::new(state, Size::new(100, 100), view).with_roles(Role::Button);

    assert_eq!(
        *harness.focus_forward().shape().unwrap(),
        ContentShape::Circle(Circle::new(Point::new(0, 0), 100))
    );
    harness.finalize_view();
    assert_eq!(
        *harness.next().shape().unwrap(),
        ContentShape::Circle(Circle::new(Point::new(0, 0), 100))
    );
    harness.finalize_view();
    assert_eq!(
        *harness.next_group(focus::GROUP_0).shape().unwrap(),
        ContentShape::Circle(Circle::new(Point::new(0, 0), 100))
    );
    harness.finalize_view();
    assert_eq!(
        *harness.next_group(focus::GROUP_2).shape().unwrap(),
        ContentShape::Circle(Circle::new(Point::new(0, 0), 100))
    );
    harness.finalize_view();
    assert_eq!(harness.state().page, Page::One);
}

#[test]
fn tap_first_moves_focus() {
    let state = State::default();
    let mut harness = App::new(state, Size::new(100, 100), view).with_roles(Role::Button);

    assert_eq!(
        *harness.focus_forward().shape().unwrap(),
        ContentShape::Circle(Circle::new(Point::new(0, 0), 100))
    );

    harness.tap(Point::new(50, 50));
    harness.focus_forward();
    harness.finalize_view();
    assert_eq!(harness.state().page, Page::One);
    assert_eq!(harness.state().one_count, 1);
    assert_eq!(harness.state().two_count, 0);

    harness.next_group(focus::GROUP_1);
    harness.finalize_view();

    harness.tap(Point::new(50, 50));
    harness.finalize_view();
    assert_eq!(harness.state().page, Page::Two);
    assert_eq!(harness.state().one_count, 1);
    assert_eq!(harness.state().two_count, 1);
}

#[derive(Debug, Clone, Default)]
struct DialState {
    turns: u32,
    exits: u32,
}

fn dial_page(_: &DialState) -> impl View<(), DialState> + use<> {
    Paginate::new(
        focus::GROUP_1,
        |_: &mut DialState, _| {},
        Rotary::new(
            |s: &mut DialState, event: RotaryEvent| match event {
                RotaryEvent::Next => s.turns += 1,
                RotaryEvent::Exit => s.exits += 1,
                _ => {}
            },
            |_: RotaryState| Circle,
        ),
    )
}

#[test]
fn teardown_in_the_pagination_group_reaches_the_page() {
    let mut harness =
        App::new(DialState::default(), Size::new(100, 100), dial_page).with_roles(Role::Button);
    assert!(harness.focus_forward_group(focus::GROUP_1).is_handled());
    assert!(harness.select_group(focus::GROUP_1).is_handled());
    harness.finalize_view();

    // The captive dial is released exactly once.
    _ = harness.send(Event::Focus {
        action: FocusAction::Teardown,
        group: focus::GROUP_1,
    });
    assert_eq!(harness.state().exits, 1);

    // Turning in the pagination group turns the page, not the dial.
    harness.finalize_view();
    assert!(harness.next_group(focus::GROUP_1).is_handled());
    assert_eq!(harness.state().turns, 0);
}
