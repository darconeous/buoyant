//! A text field edits a buffer in application data, starts editing when
//! typed into, and leaves other keys to the views around it.

use buoyant::{
    app::{App, Harness as _},
    event::Key,
    focus::{Direction, FocusKey, FocusState, Role},
    font::CharacterBufferFont,
    match_view,
    primitives::{Point, Size, geometry::Rectangle},
    render,
    view::{EditBuffer, ViewMarker, prelude::*},
};

static FONT: CharacterBufferFont = CharacterBufferFont;

type Tree = render::TextField<'static, CharacterBufferFont, heapless::String<16>>;

#[derive(Default)]
struct State {
    focus: FocusState,
    drafts: [EditBuffer<heapless::String<16>>; 2],
    shown: usize,
    sent: heapless::Vec<heapless::String<16>, 4>,
    /// What the field said each time it became active or inactive.
    activity: heapless::Vec<bool, 8>,
}

fn field(
    state: &State,
    chat: usize,
) -> impl View<(), State> + ViewMarker<Renderables = Tree> + use<> {
    TextField::new(&state.drafts[chat], &FONT, move |s: &mut State| {
        s.drafts.get_mut(chat)
    })
    .on_submit(move |s: &mut State| {
        _ = s.sent.push(s.drafts[chat].text().try_into().unwrap());
        s.drafts[chat].clear();
    })
    .with_max_lines(3)
    .with_caret_width(1)
}

fn one_field(state: &State) -> impl View<(), State> + ViewMarker<Renderables = Tree> + use<> {
    field(state, 0)
}

fn app_with<V: View<(), State>>(
    state: State,
    width: u32,
    view: fn(&State) -> V,
) -> App<V, State, fn(&State) -> V> {
    App::new(state, Size::new(width, 3), view)
        .with_roles(Role::Button | Role::TextEntry)
        .with_focus_binding(|s| &mut s.focus)
}

fn app<V: View<(), State>>(
    text: &str,
    width: u32,
    view: fn(&State) -> V,
) -> App<V, State, fn(&State) -> V> {
    let mut state = State::default();
    state.drafts[0] = text.into();
    let mut app = app_with(state, width, view);
    assert!(app.focus_forward().is_handled());
    app
}

/// The field's lines, as text, and its caret.
fn shown<V>(app: &App<V, State, fn(&State) -> V>) -> (Vec<String>, Option<Rectangle>)
where
    V: View<(), State> + ViewMarker<Renderables = Tree>,
{
    let tree = app.render_trees().target();
    let lines = tree
        .lines
        .iter()
        .map(|range| tree.text[range.start as usize..range.end as usize].to_owned())
        .collect();
    (lines, tree.caret)
}

fn caret(x: i32, y: i32) -> Rectangle {
    Rectangle::new(Point::new(x, y), Size::new(1, 1))
}

#[test]
fn an_empty_field_has_its_caret_at_the_start() {
    let mut app = app("", 5, one_field);
    assert!(app.select().is_handled());
    app.finalize_view();
    assert_eq!(shown(&app), (vec![String::new()], Some(caret(0, 0))));
}

#[test]
fn an_inactive_field_shows_no_caret_and_lets_backspace_through() {
    let mut app = app("ab", 5, one_field);
    assert!(!app.key_down(Key::Backspace).is_handled());
    app.finalize_view();
    assert_eq!(shown(&app), (vec!["ab".to_owned()], None));
    assert_eq!(app.state().drafts[0].text(), "ab");
}

#[test]
fn typing_starts_editing() {
    let mut app = app("ab", 5, one_field);
    assert!(app.key_down(Key::Character('c')).is_handled());
    app.finalize_view();
    assert_eq!(app.state().drafts[0].text(), "abc");
    assert_eq!(shown(&app).1, Some(caret(3, 0)));

    // Active, Backspace erases and turning moves the cursor.
    assert!(app.key_down(Key::Backspace).is_handled());
    assert!(app.previous().is_handled());
    app.finalize_view();
    assert_eq!(app.state().drafts[0].text(), "ab");
    assert_eq!(shown(&app).1, Some(caret(1, 0)));

    // A press is done.
    assert!(app.select().is_handled());
    app.finalize_view();
    assert_eq!(shown(&app).1, None);
}

#[test]
fn spaces_at_a_break_stay_on_the_line_before_it() {
    let mut app = app("ab  cd", 3, one_field);
    assert!(app.select().is_handled());
    app.finalize_view();
    assert_eq!(shown(&app).0, ["ab  ", "cd"]);

    // At the break, the caret begins the next line; in the hanging spaces
    // it stays at the end of the line before, within the field.
    for _ in 0..2 {
        assert!(app.previous().is_handled());
    }
    app.finalize_view();
    assert_eq!(shown(&app).1, Some(caret(0, 1)));
    assert!(app.previous().is_handled());
    app.finalize_view();
    assert_eq!(shown(&app).1, Some(caret(2, 0)));
}

#[test]
fn the_cursor_moves_across_a_break() {
    let mut app = app("abc def", 4, one_field);
    assert!(app.select().is_handled());
    app.finalize_view();
    assert_eq!(shown(&app).0, ["abc ", "def"]);
    for _ in 0..4 {
        assert!(app.previous().is_handled());
    }
    app.finalize_view();
    assert_eq!(shown(&app).1, Some(caret(3, 0)));
    assert!(app.next().is_handled());
    app.finalize_view();
    assert_eq!(shown(&app).1, Some(caret(0, 1)));
}

#[test]
fn backspace_with_nothing_to_erase_ends_the_edit() {
    let mut app = app("a", 5, one_field);
    assert!(app.select().is_handled());
    assert!(app.key_down(Key::Backspace).is_handled());
    app.finalize_view();
    assert_eq!(shown(&app).1, Some(caret(0, 0)));

    // Empty, Backspace ends the edit; the field keeps focus, and the next
    // Backspace is offered further up.
    assert!(app.key_down(Key::Backspace).is_handled());
    app.finalize_view();
    assert_eq!(shown(&app).1, None);
    assert_ne!(app.focus_shape(), &buoyant::render::ContentShape::Empty);
    assert!(!app.key_down(Key::Backspace).is_handled());
}

#[test]
fn a_full_field_refuses_input() {
    let mut app = app("abcdefghijklmnop", 20, one_field);
    assert!(app.key_down(Key::Character('q')).is_handled());
    app.finalize_view();
    assert_eq!(app.state().drafts[0].text(), "abcdefghijklmnop");
}

#[test]
fn text_goes_in_whole_or_not_at_all() {
    let mut app = app("abcdefghij", 20, one_field);
    // Its thumb would fit in the six bytes left, but not with its tone.
    assert!(app.key_down(Key::Text("\u{1F44D}\u{1F3FD}")).is_handled());
    app.finalize_view();
    assert_eq!(app.state().drafts[0].text(), "abcdefghij");
    assert!(app.key_down(Key::Text("\u{2764}\u{FE0F}")).is_handled());
    app.finalize_view();
    assert_eq!(app.state().drafts[0].text(), "abcdefghij\u{2764}\u{FE0F}");
}

#[test]
fn enter_starts_editing_then_submits() {
    let mut app = app("hi", 5, one_field);
    assert!(app.key_down(Key::Enter).is_handled());
    app.finalize_view();
    assert!(app.state().sent.is_empty());
    assert!(app.key_down(Key::Enter).is_handled());
    app.finalize_view();
    assert_eq!(app.state().sent, ["hi"]);
    assert!(app.state().drafts[0].is_empty());

    // Submitting ends the edit: turning moves focus on, and Backspace is
    // offered further up.
    assert_eq!(shown(&app).1, None);
    assert!(!app.key_down(Key::Backspace).is_handled());
}

#[test]
fn a_long_draft_scrolls_to_keep_the_cursor_in_view() {
    let mut app = app("aa bb cc dd", 3, one_field);
    assert!(app.select().is_handled());
    app.finalize_view();
    assert_eq!(shown(&app).0, ["bb ", "cc ", "dd"]);
    assert_eq!(shown(&app).1, Some(caret(2, 2)));

    // Back to the start, the first lines come into view.
    for _ in 0..11 {
        assert!(app.previous().is_handled());
    }
    app.finalize_view();
    assert_eq!(shown(&app).0, ["aa ", "bb ", "cc "]);
    assert_eq!(shown(&app).1, Some(caret(0, 0)));
}

type BarTree =
    render::TextField<'static, CharacterBufferFont, heapless::String<16>, render::AreaColors<()>>;

/// A one-line field with a scroll bar.
fn with_bar(state: &State) -> impl View<(), State> + ViewMarker<Renderables = BarTree> + use<> {
    TextField::new(&state.drafts[0], &FONT, |s: &mut State| s.drafts.get_mut(0))
        .with_max_lines(1)
        .with_caret_width(1)
        .with_label(|_, area| {
            area.colors(render::AreaColors {
                caret: None,
                scroll_bar: Some(((), ())),
            })
        })
}

#[test]
fn text_longer_than_the_field_makes_room_for_the_scroll_bar() {
    // At its full width of 14, this takes two lines; the bar's room of 10
    // leaves 4 for the text beside it.
    let mut state = State::default();
    state.drafts[0] = "aaa bbb ccc ddd".into();
    let mut app = App::new(state, Size::new(14, 3), with_bar)
        .with_roles(Role::Button | Role::TextEntry)
        .with_focus_binding(|s| &mut s.focus);
    app.finalize_view();
    let tree = app.render_trees().target();
    assert_eq!(tree.size.width, 14);
    let shown: Vec<&str> = tree
        .lines
        .iter()
        .map(|range| &tree.text[range.start as usize..range.end as usize])
        .collect();
    assert_eq!(shown, ["ddd"]);
    assert_eq!(
        tree.scrolled,
        Some(render::Scrolled {
            first: 3,
            shown: 1,
            total: 4,
        })
    );

    // Text that fits shows no bar and keeps the whole width.
    app.state_mut().drafts[0] = "aaa bbb".into();
    app.finalize_view();
    let tree = app.render_trees().target();
    assert_eq!(tree.scrolled, None);
    assert_eq!(tree.lines.len(), 1);
}

const FIELD: FocusKey = FocusKey(1);

fn chats(state: &State) -> impl View<(), State> + use<> {
    match_view!(state.shown, {
        0 => field(state, 0).focus_key(FIELD),
        _ => field(state, 1).focus_key(FIELD),
    })
}

#[test]
fn each_field_edits_only_its_own_draft() {
    let mut app = app_with(State::default(), 10, chats);
    app.focus(FIELD);
    assert!(app.key_down(Key::Character('a')).is_handled());
    app.finalize_view();

    // Another chat's field is shown in its place, and starts inactive.
    app.state_mut().shown = 1;
    app.finalize_view();
    assert!(!app.key_down(Key::Backspace).is_handled());
    assert!(app.key_down(Key::Character('b')).is_handled());
    app.finalize_view();
    assert_eq!(app.state().drafts[0].text(), "a");
    assert_eq!(app.state().drafts[1].text(), "b");

    // Back to the first: inactive again, its draft kept.
    app.state_mut().shown = 0;
    app.finalize_view();
    assert!(!app.key_down(Key::Backspace).is_handled());
    assert_eq!(app.state().drafts[0].text(), "a");
}

fn field_and_button(state: &State) -> impl View<(), State> + use<> {
    VStack::new((
        Button::new(|_: &mut State| {}, |_| Circle)
            .frame_sized(1, 1)
            .focus_key(FocusKey(0)),
        field(state, 0).focus_key(FIELD),
    ))
}

#[test]
fn a_field_takes_focus_only_with_the_text_entry_role() {
    let mut app = App::new(State::default(), Size::new(5, 4), field_and_button)
        .with_roles(Role::Button)
        .with_focus_binding(|s| &mut s.focus);
    app.focus(FocusKey(0));
    app.focus(FIELD);
    assert_eq!(app.focused_key(), Some(FocusKey(0)));
    assert!(!app.next().is_handled());
}

#[derive(Default)]
struct Looks {
    draft: EditBuffer<heapless::String<16>>,
}

/// A field inset further while it is edited, so its status shows in its size.
fn labeled(state: &Looks) -> impl View<(), Looks> + use<> {
    TextField::new(&state.draft, &FONT, |s: &mut Looks| Some(&mut s.draft)).with_label(
        |status, area| {
            let inset = if status.is_active() { 2 } else { 1 };
            area.padding(Edges::All, inset)
        },
    )
}

#[test]
fn the_label_follows_the_field_status() {
    let mut app =
        App::new(Looks::default(), Size::new(10, 10), labeled).with_roles(Role::TextEntry);
    let shape = |app: &App<_, _, _>| match app.focus_shape() {
        buoyant::render::ContentShape::Rectangle(rect) => rect.size,
        other => panic!("unexpected focus shape {other:?}"),
    };
    assert!(app.focus_forward().is_handled());
    assert_eq!(shape(&app), Size::new(8, 1));
    assert!(app.select().is_handled());
    app.finalize_view();
    assert_eq!(shape(&app), Size::new(6, 1));
}

/// Text shared by counted reference, copied only when edited.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Shared(std::rc::Rc<str>);

impl AsRef<str> for Shared {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl buoyant::view::EditText for Shared {
    fn splice(&mut self, range: core::ops::Range<usize>, with: &str) -> bool {
        let mut text = String::from(&*self.0);
        text.replace_range(range, with);
        self.0 = text.into();
        true
    }
}

#[derive(Default)]
struct SharedDraft {
    draft: EditBuffer<Shared>,
}

fn shared_field(
    state: &SharedDraft,
) -> impl View<(), SharedDraft>
+ ViewMarker<Renderables = render::TextField<'static, CharacterBufferFont, Shared>>
+ use<> {
    TextField::new(&state.draft, &FONT, |s: &mut SharedDraft| {
        Some(&mut s.draft)
    })
}

#[test]
fn the_field_shows_shared_text_without_copying_it() {
    let mut app = App::new(SharedDraft::default(), Size::new(10, 1), shared_field)
        .with_roles(Role::TextEntry);
    assert!(app.focus_forward().is_handled());
    for c in "hello".chars() {
        assert!(app.key_down(Key::Character(c)).is_handled());
    }
    app.finalize_view();
    let shown = &app.render_trees().target().text;
    assert_eq!(shown.as_ref(), "hello");
    assert!(std::rc::Rc::ptr_eq(
        &shown.0,
        &app.state().draft.storage().0
    ));
}

fn reporting(state: &State) -> impl View<(), State> + use<> {
    TextField::new(&state.drafts[0], &FONT, |s: &mut State| s.drafts.get_mut(0))
        .on_submit(|s: &mut State| s.drafts[0].clear())
        .on_active(|s: &mut State, active| _ = s.activity.push(active))
}

#[test]
fn the_field_says_when_it_becomes_active_and_inactive() {
    let mut app = app("", 5, reporting);
    assert!(app.state().activity.is_empty());

    // Typing starts the edit, once; submitting ends it.
    assert!(app.key_down(Key::Character('a')).is_handled());
    assert!(app.key_down(Key::Character('b')).is_handled());
    assert_eq!(app.state().activity, [true]);
    assert!(app.key_down(Key::Enter).is_handled());
    assert_eq!(app.state().activity, [true, false]);

    // A press starts it and ends it.
    assert!(app.select().is_handled());
    assert!(app.select().is_handled());
    assert_eq!(app.state().activity, [true, false, true, false]);
}

fn between_buttons(state: &State) -> impl View<(), State> + use<> {
    VStack::new((
        Button::new(|_: &mut State| {}, |_| Rectangle)
            .frame_sized(6, 1)
            .focus_key(FocusKey(1)),
        TextField::new(&state.drafts[0], &FONT, |s: &mut State| s.drafts.get_mut(0))
            .with_max_lines(3)
            .with_caret_width(1)
            .on_active(|s: &mut State, active| _ = s.activity.push(active))
            .focus_key(FocusKey(2)),
        Button::new(|_: &mut State| {}, |_| Rectangle)
            .frame_sized(6, 1)
            .focus_key(FocusKey(3)),
    ))
}

#[test]
fn up_and_down_move_the_cursor_by_line_and_leave_from_the_ends() {
    let mut state = State::default();
    // Lines "abcde ", "ab " and "abcde", the cursor before the first 'e'.
    state.drafts[0] = "abcde ab abcde".into();
    state.drafts[0].set_cursor(4);
    let mut app = App::new(state, Size::new(6, 5), between_buttons as fn(&State) -> _)
        .with_roles(Role::Button | Role::TextEntry);
    assert_eq!(app.focus_forward().key(), Some(FocusKey(1)));
    assert_eq!(app.move_toward(Direction::Down).key(), Some(FocusKey(2)));
    assert!(app.select().is_handled());
    let cursor = |app: &App<_, State, _>| app.state().drafts[0].cursor();

    // Down through the short line, the cursor keeps to its column.
    assert!(app.key_down(Key::DownArrow).is_handled());
    assert_eq!(cursor(&app), 8);
    assert!(app.key_down(Key::DownArrow).is_handled());
    assert_eq!(cursor(&app), 13);

    // From the last line, Down is left to the views around, but the edited
    // field keeps focus.
    assert!(!app.key_down(Key::DownArrow).is_handled());
    assert_eq!(app.move_toward(Direction::Down).key(), Some(FocusKey(2)));
    assert_eq!(app.state().activity, [true]);
    assert_eq!(cursor(&app), 13);

    // Left and Right move by character, and set the column anew.
    assert!(app.key_down(Key::UpArrow).is_handled());
    assert_eq!(cursor(&app), 8);
    assert!(app.key_down(Key::LeftArrow).is_handled());
    assert_eq!(cursor(&app), 7);
    assert!(app.key_down(Key::UpArrow).is_handled());
    assert_eq!(cursor(&app), 1);

    // From the first line too. Once the edit ends, a move leaves the field.
    assert!(!app.key_down(Key::UpArrow).is_handled());
    assert_eq!(app.move_toward(Direction::Up).key(), Some(FocusKey(2)));
    assert!(app.select().is_handled());
    assert_eq!(app.state().activity, [true, false]);
    assert_eq!(app.move_toward(Direction::Up).key(), Some(FocusKey(1)));
}

#[test]
fn a_directional_move_never_leaves_an_edited_field() {
    let mut state = State::default();
    state.drafts[0] = "ab".into();
    let mut app = App::new(state, Size::new(6, 5), between_buttons as fn(&State) -> _)
        .with_roles(Role::Button | Role::TextEntry);
    assert_eq!(app.focus_forward().key(), Some(FocusKey(1)));
    assert_eq!(app.move_toward(Direction::Down).key(), Some(FocusKey(2)));
    assert!(app.select().is_handled());
    for dir in [
        Direction::Up,
        Direction::Down,
        Direction::Left,
        Direction::Right,
    ] {
        assert_eq!(app.move_toward(dir).key(), Some(FocusKey(2)), "{dir:?}");
    }
    assert_eq!(app.state().activity, [true]);
    assert_eq!(app.state().drafts[0].cursor(), 2);
    assert!(app.key_down(Key::Character('c')).is_handled());
    assert_eq!(app.state().drafts[0].text(), "abc");
}

/// A field whose text area sits beside a mark, over a background.
fn marked(state: &Looks) -> impl View<(), Looks> + use<> {
    TextField::new(&state.draft, &FONT, |s: &mut Looks| Some(&mut s.draft))
        .with_max_lines(3)
        .with_caret_width(1)
        .with_label(|_, area| {
            HStack::new((Rectangle.frame_sized(1, 1), area))
                .background(Alignment::Center, Rectangle)
        })
}

#[test]
fn lines_are_those_of_the_text_area_within_the_label() {
    // Beside the mark, the area is 3 wide: lines "a ", "bc " and "de".
    // The whole width would make them "a bc " and "de".
    let mut looks = Looks {
        draft: "a bc de".into(),
    };
    looks.draft.set_cursor(0);
    let mut app = App::new(looks, Size::new(4, 3), marked).with_roles(Role::TextEntry);
    assert!(app.focus_forward().is_handled());
    assert!(app.select().is_handled());
    assert!(app.key_down(Key::DownArrow).is_handled());
    assert_eq!(app.state().draft.cursor(), 2);
    assert!(app.key_down(Key::DownArrow).is_handled());
    assert_eq!(app.state().draft.cursor(), 5);
    assert!(!app.key_down(Key::DownArrow).is_handled());
}

#[test]
fn a_directional_move_that_leads_nowhere_keeps_the_edit() {
    let mut app = app("ab", 5, one_field);
    assert!(app.select().is_handled());
    assert!(!app.key_down(Key::DownArrow).is_handled());
    assert!(app.move_toward(Direction::Down).is_handled());
    app.finalize_view();
    assert_eq!(shown(&app).1, Some(caret(2, 0)));
    assert!(app.key_down(Key::Character('c')).is_handled());
    assert_eq!(app.state().drafts[0].text(), "abc");
}

#[test]
fn a_tap_places_the_cursor_and_starts_editing() {
    let mut state = State::default();
    // Lines "abcde ", "ab " and "abcde", on rows 1 to 3.
    state.drafts[0] = "abcde ab abcde".into();
    let mut app = App::new(state, Size::new(6, 5), between_buttons as fn(&State) -> _)
        .with_roles(Role::Button | Role::TextEntry);
    assert_eq!(app.focus_forward().key(), Some(FocusKey(1)));
    let cursor = |app: &App<_, State, _>| app.state().drafts[0].cursor();

    // From the button, a tap after the 'a' of "ab" focuses the field and
    // edits there.
    app.tap(Point::new(1, 2));
    assert_eq!(app.focused_key(), Some(FocusKey(2)));
    assert_eq!(app.state().activity, [true]);
    assert_eq!(cursor(&app), 7);
    assert!(app.key_down(Key::Character('x')).is_handled());
    assert_eq!(app.state().drafts[0].text(), "abcde axb abcde");

    // While editing, a tap only moves the cursor. Past the end of a line, it
    // goes to the end.
    app.tap(Point::new(5, 3));
    assert_eq!(cursor(&app), 15);
    assert_eq!(app.state().activity, [true]);

    // Focused but not editing, a tap starts the edit again.
    assert!(app.select().is_handled());
    assert_eq!(app.state().activity, [true, false]);
    app.tap(Point::new(0, 1));
    assert_eq!(cursor(&app), 0);
    assert_eq!(app.state().activity, [true, false, true]);
}

#[test]
fn a_touch_that_leaves_the_field_does_not_tap_it() {
    let mut app = app("ab", 5, reporting);
    app.drag(Point::new(1, 0), Point::new(8, 0));
    assert!(app.state().activity.is_empty());
    app.drag(Point::new(1, 0), Point::new(2, 0));
    assert_eq!(app.state().activity, [true]);
}

#[test]
fn a_tap_on_a_scrolled_field_counts_the_lines_above() {
    // Lines "ab ", "cd ", "ef " and "gh": with the cursor at the end, the
    // three shown are the last three.
    let mut app = app("ab cd ef gh", 3, one_field);
    assert!(app.select().is_handled());
    app.finalize_view();
    assert_eq!(shown(&app).0, ["cd ", "ef ", "gh"]);
    app.tap(Point::new(1, 0));
    assert_eq!(app.state().drafts[0].cursor(), 4);
}

#[test]
fn a_tap_is_left_to_the_views_around_without_the_text_entry_role() {
    let mut app = App::new(State::default(), Size::new(5, 4), reporting).with_roles(Role::Button);
    app.tap(Point::new(1, 0));
    assert!(app.state().activity.is_empty());
}
