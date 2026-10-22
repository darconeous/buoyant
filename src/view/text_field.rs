//! A text field that edits text kept in application data.

use core::ops::Range;

use embedded_touch::{Phase, Touch};

use crate::{
    environment::LayoutEnvironment,
    event::{Event, EventContext, EventResult, Key, TextAreaGeometry, TextAreaQuery, TouchResult},
    focus::{DefaultFocus, FocusAction, Role, Seek, SeekResult, SeekTarget},
    font::{self, CustomSize, Font, FontMetrics},
    layout::ResolvedLayout,
    primitives::{Point, ProposedDimension, ProposedDimensions, Size, geometry::Rectangle},
    render::{self, ContentShape, IntrinsicShape, text_field::MAX_VISIBLE_LINES},
    transition::Opacity,
    view::{ViewLayout, ViewMarker},
};

/// Where an [`EditBuffer`] keeps its text.
///
/// A field's views and both of its render trees each hold a clone of the
/// text they show. A [`heapless::String`] is copied each time; storage that
/// shares its text when cloned, such as a counted reference that copies on
/// write, keeps long text cheap to show.
pub trait EditText: AsRef<str> + Clone {
    /// Replaces the bytes in `range`, which lies on character boundaries,
    /// with `with`.
    ///
    /// Returns `false`, changing nothing, when the result does not fit.
    fn splice(&mut self, range: Range<usize>, with: &str) -> bool;
}

impl<const N: usize> EditText for heapless::String<N> {
    fn splice(&mut self, range: Range<usize>, with: &str) -> bool {
        if self.len() - range.len() + with.len() > N {
            return false;
        }
        let mut tail = Self::new();
        _ = tail.push_str(&self[range.end..]);
        self.truncate(range.start);
        _ = self.push_str(with);
        _ = self.push_str(&tail);
        true
    }
}

/// Text being edited, and the position of the cursor in it.
///
/// It lives in application data, so it outlives the field that edits it:
/// a draft is kept when its field goes away. Its text is kept in `T`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditBuffer<T> {
    text: T,
    /// A byte index on a character boundary.
    cursor: usize,
}

impl<const N: usize> EditBuffer<heapless::String<N>> {
    /// An empty buffer.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            text: heapless::String::new(),
            cursor: 0,
        }
    }
}

impl<T: EditText> EditBuffer<T> {
    /// A buffer holding `text`, with the cursor at its end.
    #[must_use]
    pub fn with_text(text: T) -> Self {
        let cursor = text.as_ref().len();
        Self { text, cursor }
    }

    /// The text.
    #[must_use]
    pub fn text(&self) -> &str {
        self.text.as_ref()
    }

    /// The text as it is stored, to show without copying.
    #[must_use]
    pub fn storage(&self) -> &T {
        &self.text
    }

    /// The cursor, as a byte index into the text.
    #[must_use]
    pub fn cursor(&self) -> usize {
        self.clamped(self.cursor)
    }

    /// Whether there is no text.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text().is_empty()
    }

    /// Removes all of the text.
    pub fn clear(&mut self) {
        let len = self.text().len();
        _ = self.text.splice(0..len, "");
        self.cursor = 0;
    }

    /// Inserts `c` at the cursor and moves the cursor past it.
    ///
    /// Returns `false`, changing nothing, when the text is full.
    pub fn insert(&mut self, c: char) -> bool {
        self.insert_str(c.encode_utf8(&mut [0; 4]))
    }

    /// Inserts `text` at the cursor, whole, and moves the cursor past it.
    ///
    /// Returns `false`, changing nothing, when the text is full.
    pub fn insert_str(&mut self, text: &str) -> bool {
        let cursor = self.cursor();
        if !self.text.splice(cursor..cursor, text) {
            return false;
        }
        self.cursor = cursor + text.len();
        true
    }

    /// Removes the grapheme cluster before the cursor, such as an emoji
    /// with its skin tone, whole.
    ///
    /// Returns `false` when the cursor is at the start.
    pub fn erase_back(&mut self) -> bool {
        let cursor = self.cursor();
        let Some(cluster) = font::clusters(&self.text()[..cursor]).next_back() else {
            return false;
        };
        let start = cursor - cluster.len();
        if !self.text.splice(start..cursor, "") {
            return false;
        }
        self.cursor = start;
        true
    }

    /// Places the cursor at `index`, a byte index into the text, moved back
    /// to a cluster boundary and kept within the text.
    pub fn set_cursor(&mut self, index: usize) {
        self.cursor = self.clamped(index);
    }

    /// Moves the cursor back one cluster. Returns `false` at the start.
    pub fn move_back(&mut self) -> bool {
        let cursor = self.cursor();
        let Some(len) = font::clusters(&self.text()[..cursor])
            .next_back()
            .map(str::len)
        else {
            return false;
        };
        self.cursor = cursor - len;
        true
    }

    /// Moves the cursor forward one cluster. Returns `false` at the end.
    pub fn move_forward(&mut self) -> bool {
        let cursor = self.cursor();
        let Some(len) = font::clusters(&self.text()[cursor..]).next().map(str::len) else {
            return false;
        };
        self.cursor = cursor + len;
        true
    }

    /// Moves the cursor to the start of the next word, or the end.
    pub fn move_forward_word(&mut self) -> bool {
        let start = self.cursor();
        let rest = &self.text()[start..];
        let word_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let next_word = rest[word_end..]
            .find(|c: char| !c.is_whitespace())
            .map_or(rest.len(), |i| word_end + i);
        self.cursor = start + next_word;
        next_word > 0
    }

    /// Moves the cursor to the start of this word or the one before it.
    pub fn move_back_word(&mut self) -> bool {
        let start = self.cursor();
        let before = self.text()[..start].trim_end();
        let word_start = before.rfind(char::is_whitespace).map_or(0, |i| {
            i + before[i..].chars().next().map_or(1, char::len_utf8)
        });
        self.cursor = word_start;
        word_start < start
    }

    /// The nearest cluster boundary at or before `index`.
    fn clamped(&self, index: usize) -> usize {
        let text = self.text();
        if index >= text.len() {
            return text.len();
        }
        font::cluster_indices(text)
            .map(|(start, _)| start)
            .take_while(|&start| start <= index)
            .last()
            .unwrap_or(0)
    }
}

impl<T: EditText + Default> From<&str> for EditBuffer<T> {
    /// A buffer holding as many of `text`'s clusters as fit, with the
    /// cursor at its end.
    fn from(text: &str) -> Self {
        let mut buffer = Self::default();
        for cluster in font::clusters(text) {
            if !buffer.insert_str(cluster) {
                break;
            }
        }
        buffer
    }
}

/// Breaks text into lines for editing.
///
/// Every byte belongs to a line: whitespace where a line breaks, and a
/// newline, end the line before the break. Text that ends at a break is
/// followed by an empty line, where a cursor at the end sits.
struct EditLines<'t, M> {
    text: &'t str,
    width: u32,
    metrics: &'t M,
    start: usize,
    done: bool,
}

impl<'t, M: FontMetrics> EditLines<'t, M> {
    const fn new(text: &'t str, width: u32, metrics: &'t M) -> Self {
        Self {
            text,
            width,
            metrics,
            start: 0,
            done: false,
        }
    }
}

impl<M: FontMetrics> Iterator for EditLines<'_, M> {
    type Item = Range<usize>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let start = self.start;
        if start == self.text.len() {
            self.done = true;
            return Some(start..start);
        }
        let mut x = 0;
        let mut seen_word = false;
        // Where the line may break: after the spaces that follow a word.
        let mut break_at = None;
        for (offset, cluster) in font::cluster_indices(&self.text[start..]) {
            let index = start + offset;
            if cluster.ends_with('\n') {
                self.start = index + cluster.len();
                return Some(start..self.start);
            }
            let advance = self.metrics.cluster_advance(cluster);
            if cluster.chars().all(char::is_whitespace) {
                // Spaces never force a break; they hang past the end.
                x += advance;
                if seen_word {
                    break_at = Some(index + cluster.len());
                }
                continue;
            }
            if x + advance > self.width && index > start {
                let end = break_at.unwrap_or(index);
                self.start = end;
                return Some(start..end);
            }
            seen_word = true;
            x += advance;
        }
        self.done = true;
        Some(start..self.text.len())
    }
}

/// The width of `text`, leaving out a newline.
fn advance_of(text: &str, metrics: &impl FontMetrics) -> u32 {
    font::clusters(text)
        .filter(|cluster| !cluster.ends_with('\n'))
        .map(|cluster| metrics.cluster_advance(cluster))
        .sum()
}

/// The width `text` wraps at in an area `width` wide: text longer than
/// `max_lines` leaves `scroll_bar_room` beside it for the scroll bar.
fn wrap_width(
    text: &str,
    width: u32,
    scroll_bar_room: u32,
    max_lines: u16,
    metrics: &impl FontMetrics,
) -> u32 {
    if scroll_bar_room > 0
        && EditLines::new(text, width, metrics)
            .nth(usize::from(max_lines))
            .is_some()
    {
        width.saturating_sub(scroll_bar_room)
    } else {
        width
    }
}

/// The line holding `cursor`, and how far into it the cursor is. A cursor at
/// a break begins the next line.
fn caret_of(text: &str, cursor: usize, wrap: u32, metrics: &impl FontMetrics) -> (usize, u32) {
    let mut last = (0, 0..0);
    for (index, line) in EditLines::new(text, wrap, metrics).enumerate() {
        if cursor < line.end {
            return (index, advance_of(&text[line.start..cursor], metrics));
        }
        last = (index, line);
    }
    let (index, line) = last;
    (
        index,
        advance_of(&text[line.start..cursor.max(line.start)], metrics),
    )
}

/// Where the cursor goes a line up or down from `cursor`, with text wrapped
/// at `wrap`: as near across the line as `goal`, or as the cursor already is
/// without one. Returns the new cursor and the goal kept for the next move,
/// or `None` from the first line up or the last line down.
fn line_move(
    text: &str,
    cursor: usize,
    wrap: u32,
    metrics: &impl FontMetrics,
    down: bool,
    goal: Option<u32>,
) -> Option<(usize, u32)> {
    let (line, x) = caret_of(text, cursor, wrap, metrics);
    let goal = goal.unwrap_or(x);
    let target = if down { line + 1 } else { line.checked_sub(1)? };
    let mut lines = EditLines::new(text, wrap, metrics).skip(target).peekable();
    let range = lines.next()?;
    let last = lines.peek().is_none();
    Some((column_at(text, range, last, goal, metrics), goal))
}

/// Where the cursor goes on `line` of text wrapped at `wrap`, as near across
/// it as `x`. A line past the last is the last.
fn offset_at(text: &str, wrap: u32, metrics: &impl FontMetrics, line: usize, x: u32) -> usize {
    let mut lines = EditLines::new(text, wrap, metrics).peekable();
    let mut index = 0;
    while let Some(range) = lines.next() {
        let last = lines.peek().is_none();
        if index == line || last {
            return column_at(text, range, last, x, metrics);
        }
        index += 1;
    }
    0
}

/// The cursor in the line at `range` nearest `x` across it. A cursor at a
/// break begins the next line, so only the `last` line holds its end.
fn column_at(
    text: &str,
    range: Range<usize>,
    last: bool,
    x: u32,
    metrics: &impl FontMetrics,
) -> usize {
    let mut best = (range.start, x);
    let mut across = 0;
    for (offset, cluster) in font::cluster_indices(&text[range.clone()]) {
        if cluster.ends_with('\n') {
            break;
        }
        across += metrics.cluster_advance(cluster);
        let index = range.start + offset + cluster.len();
        if index == range.end && !last {
            break;
        }
        let distance = across.abs_diff(x);
        if distance >= best.1 {
            break;
        }
        best = (index, distance);
    }
    best.0
}

/// What a [`TextField`] keeps while it is shown.
#[derive(Debug, Clone)]
pub struct FieldState<S> {
    status: TextFieldStatus,
    /// The label's state.
    label: S,
    /// Where the cursor heads across lines.
    goal: Option<LineGoal>,
    /// The touch that started on the field, until it ends.
    touch: Option<u8>,
}

/// Where the cursor heads across lines: kept while it moves up and down, so
/// it returns to its column after passing through a shorter line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineGoal {
    /// The cursor the last move left, which the goal belongs to.
    cursor: usize,
    /// How far across a line the cursor heads.
    x: u32,
}

/// What a field does when Enter is pressed while it is active.
pub trait OnSubmit<C: ?Sized> {
    /// Handles the submission. Returns `false` to leave Enter unhandled.
    fn on_submit(&self, captures: &mut C) -> bool;
}

impl<C: ?Sized, F: Fn(&mut C)> OnSubmit<C> for F {
    fn on_submit(&self, captures: &mut C) -> bool {
        self(captures);
        true
    }
}

/// A field without a submit action: Enter is offered further up.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoSubmit;

impl<C: ?Sized> OnSubmit<C> for NoSubmit {
    fn on_submit(&self, _captures: &mut C) -> bool {
        false
    }
}

/// What a field does when it becomes active or inactive.
pub trait OnActive<C: ?Sized> {
    /// The field became active, or inactive.
    fn on_active(&self, captures: &mut C, active: bool);
}

impl<C: ?Sized, F: Fn(&mut C, bool)> OnActive<C> for F {
    fn on_active(&self, captures: &mut C, active: bool) {
        self(captures, active);
    }
}

/// A field that tells no one when it becomes active.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoActive;

impl<C: ?Sized> OnActive<C> for NoActive {
    fn on_active(&self, _captures: &mut C, _active: bool) {}
}

/// Whether a [`TextField`] is focused and whether it is being edited, for
/// the field's label to show.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TextFieldStatus {
    focused: bool,
    active: bool,
    /// The field was tapped without focus: editing begins as focus arrives.
    resume: bool,
}

impl TextFieldStatus {
    /// Whether the field is focused.
    #[must_use]
    pub const fn is_focused(&self) -> bool {
        self.focused
    }

    /// Whether the field is being edited, showing a cursor.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.active
    }
}

/// Edits an [`EditBuffer`] kept in application data.
///
/// The field takes focus with the [`TextEntry`](Role::TextEntry) role. When
/// focused it is inactive, with no cursor shown: Backspace is offered
/// further up, and a character, Enter or a press activates it, a character
/// also being inserted. Active, it holds focus: turning moves the cursor,
/// Backspace erases, and Enter submits; submitting, a press, a blur, or
/// Backspace with no text makes it inactive. The arrow keys move the
/// cursor, Up and Down by the lines shown; Up from the first line and Down
/// from the last are left to the views around. A directional move
/// ([`Toward`](FocusAction::Toward)) leaves an inactive field, but never an
/// active one: the cursor alone moves while the field is edited.
///
/// Whether the field is active is its own state, so it lasts while the
/// field is shown and is forgotten when it is not; the text is the
/// application's.
///
/// Like a [`Button`](crate::view::Button)'s, the field's look comes from a
/// label function, given the field's [`TextFieldStatus`] and its
/// [`TextArea`] to place, as with [`with_label`](Self::with_label).
pub struct TextField<'a, B, F: Font, L, S, ViewFn, A = NoActive> {
    buffer: EditBuffer<B>,
    lens: L,
    on_submit: S,
    on_active: A,
    font: &'a F,
    attributes: F::Attributes,
    max_lines: u16,
    caret_width: u32,
    label: ViewFn,
}

/// The label of a field that shows only its text.
pub type PlainLabel<'a, B, F> = fn(TextFieldStatus, TextArea<'a, B, F>) -> TextArea<'a, B, F>;

impl<B: EditText + core::fmt::Debug, F: Font, L, S, ViewFn, A> core::fmt::Debug
    for TextField<'_, B, F, L, S, ViewFn, A>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TextField")
            .field("buffer", &self.buffer)
            .field("max_lines", &self.max_lines)
            .finish_non_exhaustive()
    }
}

impl<'a, B: EditText, F: Font, L> TextField<'a, B, F, L, NoSubmit, PlainLabel<'a, B, F>> {
    /// A field showing `buffer`, which edits the buffer `lens` reaches.
    ///
    /// The lens is called each time the field edits, and should reach the
    /// buffer by something that identifies it, such as a conversation's ID,
    /// so an edit never lands in another buffer. When it finds none, the
    /// field changes nothing.
    #[must_use]
    pub fn new<C: ?Sized>(buffer: &EditBuffer<B>, font: &'a F, lens: L) -> Self
    where
        L: Fn(&mut C) -> Option<&mut EditBuffer<B>>,
    {
        Self {
            buffer: buffer.clone(),
            lens,
            on_submit: NoSubmit,
            on_active: NoActive,
            font,
            attributes: F::Attributes::default(),
            max_lines: 1,
            caret_width: 2,
            label: |_, area| area,
        }
    }
}

impl<'a, B: EditText, F: Font, L, S, ViewFn, A> TextField<'a, B, F, L, S, ViewFn, A> {
    /// Runs `on_submit` when Enter is pressed while the field is active.
    #[must_use]
    pub fn on_submit<C: ?Sized, T: Fn(&mut C)>(
        self,
        on_submit: T,
    ) -> TextField<'a, B, F, L, T, ViewFn, A> {
        TextField {
            buffer: self.buffer,
            lens: self.lens,
            on_submit,
            on_active: self.on_active,
            font: self.font,
            attributes: self.attributes,
            max_lines: self.max_lines,
            caret_width: self.caret_width,
            label: self.label,
        }
    }

    /// Runs `on_active` each time the field becomes active or inactive,
    /// with which: so that what goes with editing, such as a keyboard's
    /// state, can begin and end with it.
    #[must_use]
    pub fn on_active<C: ?Sized, T: Fn(&mut C, bool)>(
        self,
        on_active: T,
    ) -> TextField<'a, B, F, L, S, ViewFn, T> {
        TextField {
            buffer: self.buffer,
            lens: self.lens,
            on_submit: self.on_submit,
            on_active,
            font: self.font,
            attributes: self.attributes,
            max_lines: self.max_lines,
            caret_width: self.caret_width,
            label: self.label,
        }
    }

    /// Draws the field with `label`, given its status and its text area.
    #[must_use]
    pub fn with_label<Inner, T>(self, label: T) -> TextField<'a, B, F, L, S, T, A>
    where
        T: Fn(TextFieldStatus, TextArea<'a, B, F>) -> Inner,
    {
        TextField {
            buffer: self.buffer,
            lens: self.lens,
            on_submit: self.on_submit,
            on_active: self.on_active,
            font: self.font,
            attributes: self.attributes,
            max_lines: self.max_lines,
            caret_width: self.caret_width,
            label,
        }
    }

    /// Grows to at most `lines` lines.
    #[must_use]
    pub fn with_max_lines(mut self, lines: u16) -> Self {
        self.max_lines = lines.clamp(1, MAX_VISIBLE_LINES as u16);
        self
    }

    /// Sets the width of the cursor.
    #[must_use]
    pub fn with_caret_width(mut self, width: u32) -> Self {
        self.caret_width = width;
        self
    }

    /// The text area for a field with `status`.
    fn area(&self, status: TextFieldStatus) -> TextArea<'a, B, F> {
        TextArea {
            buffer: self.buffer.clone(),
            font: self.font,
            attributes: self.attributes.clone(),
            max_lines: self.max_lines,
            caret_width: self.caret_width,
            active: status.active,
            style: (),
            scroll_bar_room: 0,
        }
    }
}

impl<B: EditText, F: Font<Attributes: CustomSize>, L, S, ViewFn, A>
    TextField<'_, B, F, L, S, ViewFn, A>
{
    /// Sets the font size.
    #[must_use]
    pub fn with_font_size(self, size: u32) -> Self {
        Self {
            attributes: self.attributes.with_size(size),
            ..self
        }
    }
}

impl<'a, B: EditText, F: Font, L, S, ViewFn, A, Inner> ViewMarker
    for TextField<'a, B, F, L, S, ViewFn, A>
where
    ViewFn: Fn(TextFieldStatus, TextArea<'a, B, F>) -> Inner,
    Inner: ViewMarker<Renderables: IntrinsicShape>,
{
    type Renderables = Inner::Renderables;
    type Transition = Opacity;
}

impl<'a, C: ?Sized, B: EditText, F: Font, L, S, ViewFn, A, Inner> ViewLayout<C>
    for TextField<'a, B, F, L, S, ViewFn, A>
where
    L: Fn(&mut C) -> Option<&mut EditBuffer<B>>,
    S: OnSubmit<C>,
    A: OnActive<C>,
    ViewFn: Fn(TextFieldStatus, TextArea<'a, B, F>) -> Inner,
    Inner: ViewLayout<C, Renderables: IntrinsicShape>,
{
    type State = FieldState<Inner::State>;
    type Sublayout = ResolvedLayout<Inner::Sublayout>;
    type FocusTree = ();

    fn transition(&self) -> Self::Transition {
        Opacity
    }

    fn build_state(&self, captures: &mut C) -> Self::State {
        let status = TextFieldStatus::default();
        let label = (self.label)(status, self.area(status)).build_state(captures);
        FieldState {
            status,
            label,
            goal: None,
            touch: None,
        }
    }

    fn layout(
        &self,
        offer: &ProposedDimensions,
        env: &impl LayoutEnvironment,
        captures: &mut C,
        state: &mut Self::State,
    ) -> ResolvedLayout<Self::Sublayout> {
        let inner = (self.label)(state.status, self.area(state.status)).layout(
            offer,
            env,
            captures,
            &mut state.label,
        );
        ResolvedLayout {
            resolved_size: inner.resolved_size,
            sublayouts: inner,
        }
    }

    fn render_tree(
        &self,
        layout: &Self::Sublayout,
        origin: Point,
        env: &impl LayoutEnvironment,
        captures: &mut C,
        state: &mut Self::State,
    ) -> Self::Renderables {
        (self.label)(state.status, self.area(state.status)).render_tree(
            &layout.sublayouts,
            origin,
            env,
            captures,
            &mut state.label,
        )
    }

    fn handle_event(
        &self,
        event: &Event,
        context: &EventContext,
        render_tree: &mut Self::Renderables,
        captures: &mut C,
        state: &mut Self::State,
        _focus: &mut Self::FocusTree,
    ) -> EventResult {
        let was_active = state.status.active;
        let result = match event {
            Event::Focus { action, .. } => {
                if !context.roles.contains(Role::TextEntry) {
                    return EventResult::Deferred;
                }
                state.goal = None;
                self.handle_focus(
                    *action,
                    context,
                    captures,
                    &mut state.status,
                    render_tree.content_shape(),
                )
            }
            Event::KeyDown(key @ (Key::UpArrow | Key::DownArrow))
                if state.status.focused && state.status.active =>
            {
                self.move_line(
                    *key == Key::DownArrow,
                    context,
                    render_tree,
                    captures,
                    state,
                )
            }
            Event::KeyDown(key) if state.status.focused => {
                state.goal = None;
                self.handle_key(*key, context, captures, &mut state.status)
            }
            _ => EventResult::Deferred,
        };
        if state.status.active != was_active {
            self.on_active.on_active(captures, state.status.active);
        }
        result
    }

    fn seek(
        &self,
        seek: &Seek,
        render_tree: &Self::Renderables,
        _state: &Self::State,
        _focus: Option<&Self::FocusTree>,
    ) -> SeekResult<Self::FocusTree> {
        if !seek.roles.contains(Role::TextEntry) {
            return SeekResult::Absent;
        }
        match seek.target {
            SeekTarget::Any => SeekResult::found(()),
            SeekTarget::At(point) if render_tree.content_shape().contains(point) => {
                SeekResult::found(())
            }
            _ => SeekResult::Absent,
        }
    }

    fn handle_touch(
        &self,
        touch: &Touch,
        context: &EventContext,
        render_tree: &mut Self::Renderables,
        captures: &mut C,
        state: &mut Self::State,
    ) -> TouchResult<Self::FocusTree> {
        if !context.roles.contains(Role::TextEntry) || state.touch.is_some_and(|id| id != touch.id)
        {
            return TouchResult::Deferred;
        }
        let point = touch.location.into();
        let area = render_tree.content_shape();
        match touch.phase {
            Phase::Started if context.touch_starts_on(&area, point) => {
                state.touch = Some(touch.id);
                TouchResult::Handled
            }
            Phase::Moved if state.touch.is_some() => TouchResult::Handled,
            Phase::Ended if state.touch.take().is_some() => {
                if context.touch_is_over(&area, point) {
                    self.tap(point, context, render_tree, captures, state)
                } else {
                    TouchResult::Handled
                }
            }
            Phase::Cancelled if state.touch.take().is_some() => TouchResult::Handled,
            _ => TouchResult::Deferred,
        }
    }
}

impl<'a, B: EditText, F: Font, L, S, ViewFn, A> TextField<'a, B, F, L, S, ViewFn, A> {
    fn handle_focus<C: ?Sized>(
        &self,
        action: FocusAction,
        context: &EventContext,
        captures: &mut C,
        status: &mut TextFieldStatus,
        shape: ContentShape,
    ) -> EventResult
    where
        L: Fn(&mut C) -> Option<&mut EditBuffer<B>>,
    {
        let before = *status;
        let result = match action {
            FocusAction::Focus(_) | FocusAction::Enter { .. } => {
                status.focused = true;
                status.active |= core::mem::take(&mut status.resume);
                EventResult::handled_focused(shape)
            }
            FocusAction::Next | FocusAction::Previous if status.active => {
                if let Some(buffer) = (self.lens)(captures) {
                    if action == FocusAction::Next {
                        buffer.move_forward();
                    } else {
                        buffer.move_back();
                    }
                    context.request_view_rebuild();
                }
                EventResult::handled_focused(shape)
            }
            FocusAction::Select => {
                status.active = !status.active;
                EventResult::handled_focused(shape)
            }
            FocusAction::Blur if status.active => {
                status.active = false;
                EventResult::handled_focused(shape)
            }
            FocusAction::Teardown => {
                *status = TextFieldStatus::default();
                EventResult::handled_unfocused()
            }
            // Edited, the field keeps focus: the arrow key that came first
            // moved the cursor, or found it at the end.
            FocusAction::Toward { .. } if status.active => EventResult::handled_focused(shape),
            FocusAction::Next
            | FocusAction::Previous
            | FocusAction::Toward { .. }
            | FocusAction::Blur => {
                status.focused = false;
                EventResult::Deferred
            }
        };
        if *status != before {
            context.request_view_rebuild();
        }
        result
    }

    /// Moves the cursor a line up or down, through the lines the text area
    /// in the label shows, keeping to the column the cursor started from
    /// across moves. From the first line up or the last line down, the key
    /// is not handled, so focus can move on.
    fn move_line<C: ?Sized, Inner>(
        &self,
        down: bool,
        context: &EventContext,
        render_tree: &mut Inner::Renderables,
        captures: &mut C,
        state: &mut FieldState<Inner::State>,
    ) -> EventResult
    where
        L: Fn(&mut C) -> Option<&mut EditBuffer<B>>,
        ViewFn: Fn(TextFieldStatus, TextArea<'a, B, F>) -> Inner,
        Inner: ViewLayout<C>,
    {
        let area = self.area_geometry(context, render_tree, captures, state);
        let (Some(area), Some(buffer)) = (area, (self.lens)(captures)) else {
            return EventResult::Deferred;
        };
        let metrics = self.font.metrics(&self.attributes);
        let text = buffer.text();
        let wrap = wrap_width(
            text,
            area.width,
            area.scroll_bar_room,
            self.max_lines,
            &metrics,
        );
        let goal = state
            .goal
            .filter(|goal| goal.cursor == buffer.cursor())
            .map(|goal| goal.x);
        let Some((cursor, x)) = line_move(text, buffer.cursor(), wrap, &metrics, down, goal) else {
            return EventResult::Deferred;
        };
        buffer.set_cursor(cursor);
        state.goal = Some(LineGoal {
            cursor: buffer.cursor(),
            x,
        });
        context.request_view_rebuild();
        EventResult::handled_unfocused()
    }

    /// A tap on the field at `point`: the cursor goes where the text was
    /// tapped, and the field is edited, taking focus if it has none.
    fn tap<C: ?Sized, Inner>(
        &self,
        point: Point,
        context: &EventContext,
        render_tree: &mut Inner::Renderables,
        captures: &mut C,
        state: &mut FieldState<Inner::State>,
    ) -> TouchResult<()>
    where
        L: Fn(&mut C) -> Option<&mut EditBuffer<B>>,
        A: OnActive<C>,
        ViewFn: Fn(TextFieldStatus, TextArea<'a, B, F>) -> Inner,
        Inner: ViewLayout<C>,
    {
        let area = self.area_geometry(context, render_tree, captures, state);
        if let (Some(area), Some(buffer)) = (area, (self.lens)(captures)) {
            let metrics = self.font.metrics(&self.attributes);
            let text = buffer.text();
            let wrap = wrap_width(
                text,
                area.width,
                area.scroll_bar_room,
                self.max_lines,
                &metrics,
            );
            let within = point - area.origin;
            let line = usize::from(area.first_line)
                + (within.y.max(0) as u32 / area.line_height.max(1)) as usize;
            let cursor = offset_at(text, wrap, &metrics, line, within.x.max(0) as u32);
            buffer.set_cursor(cursor);
        }
        state.goal = None;
        context.request_view_rebuild();
        if state.status.focused {
            if !state.status.active {
                state.status.active = true;
                self.on_active.on_active(captures, true);
            }
            TouchResult::Handled
        } else {
            // Focus moves here, and editing begins as it arrives.
            state.status.resume = true;
            TouchResult::Focused(())
        }
    }

    /// How the text area in the label lays out text, as it answers a
    /// [`TextAreaQuery`].
    fn area_geometry<C: ?Sized, Inner>(
        &self,
        context: &EventContext,
        render_tree: &mut Inner::Renderables,
        captures: &mut C,
        state: &mut FieldState<Inner::State>,
    ) -> Option<TextAreaGeometry>
    where
        ViewFn: Fn(TextFieldStatus, TextArea<'a, B, F>) -> Inner,
        Inner: ViewLayout<C>,
    {
        let query = Event::TextArea(TextAreaQuery::default());
        _ = (self.label)(state.status, self.area(state.status)).handle_event(
            &query,
            context,
            render_tree,
            captures,
            &mut state.label,
            &mut DefaultFocus::default_first(),
        );
        let Event::TextArea(query) = query else {
            unreachable!();
        };
        query.answer.get()
    }

    fn handle_key<C: ?Sized>(
        &self,
        key: Key,
        context: &EventContext,
        captures: &mut C,
        status: &mut TextFieldStatus,
    ) -> EventResult
    where
        L: Fn(&mut C) -> Option<&mut EditBuffer<B>>,
        S: OnSubmit<C>,
    {
        if !status.active {
            // Typing starts editing; anything else is for the views around.
            match key {
                Key::Character(_) | Key::Enter => status.active = true,
                _ => return EventResult::Deferred,
            }
            context.request_view_rebuild();
            if key == Key::Enter {
                return EventResult::handled_unfocused();
            }
        }
        if key == Key::Enter {
            // Submitting ends the edit.
            return if self.on_submit.on_submit(captures) {
                status.active = false;
                context.request_view_rebuild();
                EventResult::handled_unfocused()
            } else {
                EventResult::Deferred
            };
        }
        let Some(buffer) = (self.lens)(captures) else {
            return EventResult::Deferred;
        };
        if key == Key::Backspace && buffer.is_empty() {
            // Nothing to erase ends the edit.
            status.active = false;
            context.request_view_rebuild();
            return EventResult::handled_unfocused();
        }
        match key {
            Key::Character(c) => _ = buffer.insert(c),
            Key::Backspace => _ = buffer.erase_back(),
            Key::LeftArrow => _ = buffer.move_back(),
            Key::RightArrow => _ = buffer.move_forward(),
            Key::PreviousWord => _ = buffer.move_back_word(),
            Key::NextWord => _ = buffer.move_forward_word(),
            _ => return EventResult::Deferred,
        }
        context.request_view_rebuild();
        EventResult::handled_unfocused()
    }
}

/// A field's text, broken into lines, with the cursor while it is edited.
///
/// It is as wide as offered, or as its text when not offered a width, and
/// grows to the field's line limit, then scrolls within itself to keep the
/// cursor in view.
pub struct TextArea<'a, B, F: Font, K = ()> {
    buffer: EditBuffer<B>,
    font: &'a F,
    attributes: F::Attributes,
    max_lines: u16,
    caret_width: u32,
    active: bool,
    style: K,
    /// How much width the scroll bar takes when the text needs one.
    scroll_bar_room: u32,
}

/// The space between a text field's lines and its scroll bar.
const SCROLL_BAR_GAP: u32 = 7;

impl<'a, B: EditText, F: Font> TextArea<'a, B, F> {
    /// Draws the caret, and a scroll bar for text longer than the area, in
    /// `colors`.
    ///
    /// The bar takes room along the trailing edge only while the text needs
    /// it.
    #[must_use]
    pub fn colors<C>(
        self,
        colors: render::AreaColors<C>,
    ) -> TextArea<'a, B, F, render::AreaColors<C>> {
        TextArea {
            buffer: self.buffer,
            font: self.font,
            attributes: self.attributes,
            max_lines: self.max_lines,
            caret_width: self.caret_width,
            active: self.active,
            scroll_bar_room: if colors.scroll_bar.is_some() {
                render::text_field::SCROLL_BAR_WIDTH + SCROLL_BAR_GAP
            } else {
                0
            },
            style: colors,
        }
    }
}

impl<B: EditText + core::fmt::Debug, F: Font, K> core::fmt::Debug for TextArea<'_, B, F, K> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TextArea")
            .field("buffer", &self.buffer)
            .field("active", &self.active)
            .finish_non_exhaustive()
    }
}

/// The lines a text area shows and where its cursor is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextAreaLayout {
    size: Size,
    lines: heapless::Vec<Range<u16>, MAX_VISIBLE_LINES>,
    line_height: u32,
    caret: Option<Rectangle>,
    scrolled: Option<render::Scrolled>,
}

impl<'a, B: EditText, F: Font, K: Clone> ViewMarker for TextArea<'a, B, F, K> {
    type Renderables = render::TextField<'a, F, B, K>;
    type Transition = Opacity;
}

impl<C: ?Sized, B: EditText, F: Font, K: Clone> ViewLayout<C> for TextArea<'_, B, F, K> {
    /// The first line shown when the text is longer than the area.
    type State = u16;
    type Sublayout = TextAreaLayout;
    type FocusTree = ();

    fn transition(&self) -> Self::Transition {
        Opacity
    }

    fn build_state(&self, _captures: &mut C) -> Self::State {
        0
    }

    fn layout(
        &self,
        offer: &ProposedDimensions,
        _env: &impl LayoutEnvironment,
        _captures: &mut C,
        first_line: &mut Self::State,
    ) -> ResolvedLayout<Self::Sublayout> {
        let metrics = self.font.metrics(&self.attributes);
        let line_height = metrics.vertical_metrics().line_height();
        let text = self.buffer.text();
        let width = match offer.width {
            ProposedDimension::Exact(width) => width,
            _ => {
                EditLines::new(text, u32::MAX, &metrics)
                    .map(|line| advance_of(&text[line], &metrics))
                    .max()
                    .unwrap_or(0)
                    + self.caret_width
            }
        };
        // Text longer than the area leaves room for the scroll bar beside it.
        let wrap = wrap_width(text, width, self.scroll_bar_room, self.max_lines, &metrics);

        let line_count = EditLines::new(text, wrap, &metrics).count();
        let (caret_line, caret_x) = caret_of(text, self.buffer.cursor(), wrap, &metrics);

        // Keep the cursor's line in view.
        let visible = line_count.clamp(1, usize::from(self.max_lines));
        let mut first = usize::from(*first_line).min(line_count - visible);
        if caret_line < first {
            first = caret_line;
        } else if caret_line >= first + visible {
            first = caret_line + 1 - visible;
        }
        *first_line = first as u16;

        let lines = EditLines::new(text, wrap, &metrics)
            .skip(first)
            .take(visible)
            .map(|line| line.start as u16..line.end as u16)
            .collect();
        let caret = self.active.then(|| {
            Rectangle::new(
                Point::new(
                    caret_x.min(wrap.saturating_sub(self.caret_width)) as i32,
                    ((caret_line - first) as u32 * line_height) as i32,
                ),
                Size::new(self.caret_width, line_height),
            )
        });
        let size = Size::new(width, visible as u32 * line_height);
        ResolvedLayout {
            sublayouts: TextAreaLayout {
                size,
                lines,
                line_height,
                caret,
                scrolled: (line_count > visible).then_some(render::Scrolled {
                    first: first as u16,
                    shown: visible as u16,
                    total: line_count as u16,
                }),
            },
            resolved_size: size.into(),
        }
    }

    fn render_tree(
        &self,
        layout: &Self::Sublayout,
        origin: Point,
        _env: &impl LayoutEnvironment,
        _captures: &mut C,
        _state: &mut Self::State,
    ) -> Self::Renderables {
        render::TextField {
            origin,
            size: layout.size,
            font: self.font,
            attributes: self.attributes.clone(),
            text: self.buffer.text.clone(),
            lines: layout.lines.clone(),
            line_height: layout.line_height,
            caret: layout.caret,
            scrolled: layout.scrolled,
            style: self.style.clone(),
        }
    }

    fn handle_event(
        &self,
        event: &Event,
        _context: &EventContext,
        render_tree: &mut Self::Renderables,
        _captures: &mut C,
        _state: &mut Self::State,
        _focus: &mut Self::FocusTree,
    ) -> EventResult {
        match event {
            Event::TextArea(query) => {
                query.answer.set(Some(TextAreaGeometry {
                    origin: render_tree.origin,
                    width: render_tree.size.width,
                    scroll_bar_room: self.scroll_bar_room,
                    line_height: render_tree.line_height,
                    first_line: render_tree.scrolled.map_or(0, |scrolled| scrolled.first),
                }));
                EventResult::handled_unfocused()
            }
            _ => EventResult::Deferred,
        }
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

    fn handle_touch(
        &self,
        _touch: &Touch,
        _context: &EventContext,
        _render_tree: &mut Self::Renderables,
        _captures: &mut C,
        _state: &mut Self::State,
    ) -> TouchResult<Self::FocusTree> {
        TouchResult::Deferred
    }
}

#[cfg(test)]
#[allow(clippy::single_range_in_vec_init)]
mod tests {
    use super::*;
    use crate::font::{CharacterBufferFont, Font};

    fn lines(text: &str, width: u32) -> heapless::Vec<Range<usize>, 16> {
        let metrics = CharacterBufferFont.metrics(&());
        EditLines::new(text, width, &metrics).collect()
    }

    #[test]
    fn empty_text_is_one_empty_line() {
        assert_eq!(lines("", 5), [0..0]);
    }

    #[test]
    fn spaces_at_a_break_end_the_line_before_it() {
        assert_eq!(lines("ab  cd", 3), [0..4, 4..6]);
        // Trailing spaces hang past the edge without a break.
        assert_eq!(lines("abc   ", 3), [0..6]);
    }

    #[test]
    fn text_ending_at_a_break_is_followed_by_an_empty_line() {
        assert_eq!(lines("ab\n", 5), [0..3, 3..3]);
        assert_eq!(lines("ab\ncd", 5), [0..3, 3..5]);
    }

    #[test]
    fn a_word_longer_than_a_line_is_broken_between_characters() {
        assert_eq!(lines("abcdefg", 3), [0..3, 3..6, 6..7]);
    }

    #[test]
    fn lines_break_on_character_boundaries() {
        assert_eq!(lines("héllo wörld", 5), [0..7, 7..13]);
    }

    #[test]
    fn a_full_buffer_refuses_input() {
        let mut buffer = EditBuffer::<heapless::String<4>>::from("abc");
        assert!(!buffer.insert('é'));
        assert!(buffer.insert('d'));
        assert!(!buffer.insert('e'));
        assert_eq!(buffer.text(), "abcd");
        assert_eq!(buffer.cursor(), 4);
    }

    #[test]
    fn editing_keeps_to_character_boundaries() {
        let mut buffer = EditBuffer::<heapless::String<16>>::from("aéb");
        assert!(buffer.move_back());
        assert!(buffer.move_back());
        assert_eq!(buffer.cursor(), 1);
        assert!(buffer.insert('ü'));
        assert_eq!(buffer.text(), "aüéb");
        assert!(buffer.move_forward());
        assert!(buffer.erase_back());
        assert_eq!(buffer.text(), "aüb");
        assert_eq!(buffer.cursor(), 3);
    }

    #[cfg(feature = "unicode-segmentation")]
    #[test]
    fn editing_keeps_emoji_whole() {
        // A thumbs up with a skin tone, and a flag: two code points each.
        let mut buffer = EditBuffer::<heapless::String<32>>::from("a👍🏽🇫🇷");
        assert!(buffer.move_back());
        assert_eq!(buffer.cursor(), "a👍🏽".len());
        assert!(buffer.erase_back());
        assert_eq!(buffer.text(), "a🇫🇷");
        assert_eq!(buffer.cursor(), 1);
        buffer.set_cursor(3);
        assert_eq!(buffer.cursor(), 1);
        assert!(buffer.move_forward());
        assert_eq!(buffer.cursor(), buffer.text().len());
    }

    #[cfg(feature = "unicode-segmentation")]
    #[test]
    fn a_cluster_that_does_not_fit_is_left_out() {
        let buffer = EditBuffer::<heapless::String<8>>::from("ab👍🏽");
        assert_eq!(buffer.text(), "ab");
    }

    #[test]
    fn words_are_stepped_over_by_their_starts() {
        let mut buffer = EditBuffer::<heapless::String<32>>::from("one  two three");
        assert!(buffer.move_back_word());
        assert_eq!(buffer.cursor(), 9);
        assert!(buffer.move_back_word());
        assert_eq!(buffer.cursor(), 5);
        assert!(buffer.move_back_word());
        assert_eq!(buffer.cursor(), 0);
        assert!(!buffer.move_back_word());
        assert!(buffer.move_forward_word());
        assert_eq!(buffer.cursor(), 5);
        assert!(buffer.move_forward_word());
        assert!(buffer.move_forward_word());
        assert_eq!(buffer.cursor(), 14);
        assert!(!buffer.move_forward_word());
    }
}
