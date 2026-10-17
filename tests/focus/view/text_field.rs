//! A text field edits a buffer in application data, starts editing when
//! typed into, and leaves other keys to the views around it.

use buoyant::{
    app::{App, Harness as _},
    event::Key,
    focus::{FocusKey, FocusState, Role},
    font::CharacterBufferFont,
    match_view,
    primitives::{Point, Size, geometry::Rectangle},
    render,
    view::{EditBuffer, ViewMarker, prelude::*},
};

static FONT: CharacterBufferFont = CharacterBufferFont;

type Tree = render::TextField<'static, CharacterBufferFont, 16>;

#[derive(Default)]
struct State {
    focus: FocusState,
    drafts: [EditBuffer<16>; 2],
    shown: usize,
    sent: heapless::Vec<heapless::String<16>, 4>,
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
    (lines, tree.caret.clone())
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
fn a_full_field_refuses_input() {
    let mut app = app("abcdefghijklmnop", 20, one_field);
    assert!(app.key_down(Key::Character('q')).is_handled());
    app.finalize_view();
    assert_eq!(app.state().drafts[0].text(), "abcdefghijklmnop");
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

type BarTree = render::TextField<'static, CharacterBufferFont, 16, render::AreaColors<()>>;

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
    draft: EditBuffer<16>,
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
