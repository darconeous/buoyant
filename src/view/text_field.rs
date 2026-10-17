//! A text field that edits text kept in application data.

use core::ops::Range;

use embedded_touch::Touch;

use crate::{
    environment::LayoutEnvironment,
    event::{Event, EventContext, EventResult, Key, TouchResult},
    focus::{FocusAction, Role, Seek, SeekResult, SeekTarget},
    font::{CustomSize, Font, FontMetrics},
    layout::ResolvedLayout,
    primitives::{Point, ProposedDimension, ProposedDimensions, Size, geometry::Rectangle},
    render::{self, ContentShape, IntrinsicShape, text_field::MAX_VISIBLE_LINES},
    transition::Opacity,
    view::{ViewLayout, ViewMarker},
};

/// Text being edited, and the position of the cursor in it.
///
/// It lives in application data, so it outlives the field that edits it:
/// a draft is kept when its field goes away.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditBuffer<const N: usize> {
    text: heapless::String<N>,
    /// A byte index on a character boundary.
    cursor: usize,
}

impl<const N: usize> EditBuffer<N> {
    /// An empty buffer.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            text: heapless::String::new(),
            cursor: 0,
        }
    }

    /// The text.
    #[must_use]
    pub fn text(&self) -> &str {
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
        self.text.is_empty()
    }

    /// Removes all of the text.
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    /// Inserts `c` at the cursor and moves the cursor past it.
    ///
    /// Returns `false`, changing nothing, when the text is full.
    pub fn insert(&mut self, c: char) -> bool {
        if self.text.len() + c.len_utf8() > N {
            return false;
        }
        let cursor = self.cursor();
        let mut tail = heapless::String::<N>::new();
        _ = tail.push_str(&self.text[cursor..]);
        self.text.truncate(cursor);
        _ = self.text.push(c);
        _ = self.text.push_str(&tail);
        self.cursor = cursor + c.len_utf8();
        true
    }

    /// Removes the character before the cursor.
    ///
    /// Returns `false` when the cursor is at the start.
    pub fn erase_back(&mut self) -> bool {
        let cursor = self.cursor();
        let Some(c) = self.text[..cursor].chars().next_back() else {
            return false;
        };
        let start = cursor - c.len_utf8();
        let mut tail = heapless::String::<N>::new();
        _ = tail.push_str(&self.text[cursor..]);
        self.text.truncate(start);
        _ = self.text.push_str(&tail);
        self.cursor = start;
        true
    }

    /// Moves the cursor back one character. Returns `false` at the start.
    pub fn move_back(&mut self) -> bool {
        let cursor = self.cursor();
        match self.text[..cursor].chars().next_back() {
            Some(c) => {
                self.cursor = cursor - c.len_utf8();
                true
            }
            None => false,
        }
    }

    /// Moves the cursor forward one character. Returns `false` at the end.
    pub fn move_forward(&mut self) -> bool {
        let cursor = self.cursor();
        match self.text[cursor..].chars().next() {
            Some(c) => {
                self.cursor = cursor + c.len_utf8();
                true
            }
            None => false,
        }
    }

    /// Moves the cursor to the start of the next word, or the end.
    pub fn move_forward_word(&mut self) -> bool {
        let start = self.cursor();
        let rest = &self.text[start..];
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
        let before = self.text[..start].trim_end();
        let word_start = before.rfind(char::is_whitespace).map_or(0, |i| {
            i + before[i..].chars().next().map_or(1, char::len_utf8)
        });
        self.cursor = word_start;
        word_start < start
    }

    /// The nearest character boundary at or before `index`.
    fn clamped(&self, index: usize) -> usize {
        let mut index = index.min(self.text.len());
        while !self.text.is_char_boundary(index) {
            index -= 1;
        }
        index
    }
}

impl<const N: usize> From<&str> for EditBuffer<N> {
    /// A buffer holding as much of `text` as fits, with the cursor at its end.
    fn from(text: &str) -> Self {
        let mut buffer = Self::new();
        for c in text.chars() {
            if !buffer.insert(c) {
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
        for (offset, c) in self.text[start..].char_indices() {
            let index = start + offset;
            if c == '\n' {
                self.start = index + 1;
                return Some(start..self.start);
            }
            let advance = self.metrics.advance(c);
            if c.is_whitespace() {
                // Spaces never force a break; they hang past the end.
                x += advance;
                if seen_word {
                    break_at = Some(index + c.len_utf8());
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
    text.chars()
        .filter(|&c| c != '\n')
        .map(|c| metrics.advance(c))
        .sum()
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

/// Whether a [`TextField`] is focused and whether it is being edited, for
/// the field's label to show.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TextFieldStatus {
    focused: bool,
    active: bool,
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
/// Backspace with no text makes it inactive.
///
/// Whether the field is active is its own state, so it lasts while the
/// field is shown and is forgotten when it is not; the text is the
/// application's.
///
/// Like a [`Button`](crate::view::Button)'s, the field's look comes from a
/// label function, given the field's [`TextFieldStatus`] and its
/// [`TextArea`] to place, as with [`with_label`](Self::with_label).
pub struct TextField<'a, const N: usize, F: Font, L, S, ViewFn> {
    buffer: EditBuffer<N>,
    lens: L,
    on_submit: S,
    font: &'a F,
    attributes: F::Attributes,
    max_lines: u16,
    caret_width: u32,
    label: ViewFn,
}

/// The label of a field that shows only its text.
pub type PlainLabel<'a, const N: usize, F> =
    fn(TextFieldStatus, TextArea<'a, N, F>) -> TextArea<'a, N, F>;

impl<const N: usize, F: Font, L, S, ViewFn> core::fmt::Debug for TextField<'_, N, F, L, S, ViewFn> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TextField")
            .field("buffer", &self.buffer)
            .field("max_lines", &self.max_lines)
            .finish_non_exhaustive()
    }
}

impl<'a, const N: usize, F: Font, L> TextField<'a, N, F, L, NoSubmit, PlainLabel<'a, N, F>> {
    /// A field showing `buffer`, which edits the buffer `lens` reaches.
    ///
    /// The lens is called each time the field edits, and should reach the
    /// buffer by something that identifies it, such as a conversation's ID,
    /// so an edit never lands in another buffer. When it finds none, the
    /// field changes nothing.
    #[must_use]
    pub fn new<C: ?Sized>(buffer: &EditBuffer<N>, font: &'a F, lens: L) -> Self
    where
        L: Fn(&mut C) -> Option<&mut EditBuffer<N>>,
    {
        Self {
            buffer: buffer.clone(),
            lens,
            on_submit: NoSubmit,
            font,
            attributes: F::Attributes::default(),
            max_lines: 1,
            caret_width: 2,
            label: |_, area| area,
        }
    }
}

impl<'a, const N: usize, F: Font, L, S, ViewFn> TextField<'a, N, F, L, S, ViewFn> {
    /// Runs `on_submit` when Enter is pressed while the field is active.
    #[must_use]
    pub fn on_submit<C: ?Sized, T: Fn(&mut C)>(
        self,
        on_submit: T,
    ) -> TextField<'a, N, F, L, T, ViewFn> {
        TextField {
            buffer: self.buffer,
            lens: self.lens,
            on_submit,
            font: self.font,
            attributes: self.attributes,
            max_lines: self.max_lines,
            caret_width: self.caret_width,
            label: self.label,
        }
    }

    /// Draws the field with `label`, given its status and its text area.
    #[must_use]
    pub fn with_label<Inner, T>(self, label: T) -> TextField<'a, N, F, L, S, T>
    where
        T: Fn(TextFieldStatus, TextArea<'a, N, F>) -> Inner,
    {
        TextField {
            buffer: self.buffer,
            lens: self.lens,
            on_submit: self.on_submit,
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
    fn area(&self, status: TextFieldStatus) -> TextArea<'a, N, F> {
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

impl<const N: usize, F: Font<Attributes: CustomSize>, L, S, ViewFn>
    TextField<'_, N, F, L, S, ViewFn>
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

impl<'a, const N: usize, F: Font, L, S, ViewFn, Inner> ViewMarker
    for TextField<'a, N, F, L, S, ViewFn>
where
    ViewFn: Fn(TextFieldStatus, TextArea<'a, N, F>) -> Inner,
    Inner: ViewMarker<Renderables: IntrinsicShape>,
{
    type Renderables = Inner::Renderables;
    type Transition = Opacity;
}

impl<'a, C: ?Sized, const N: usize, F: Font, L, S, ViewFn, Inner> ViewLayout<C>
    for TextField<'a, N, F, L, S, ViewFn>
where
    L: Fn(&mut C) -> Option<&mut EditBuffer<N>>,
    S: OnSubmit<C>,
    ViewFn: Fn(TextFieldStatus, TextArea<'a, N, F>) -> Inner,
    Inner: ViewLayout<C, Renderables: IntrinsicShape>,
{
    type State = (TextFieldStatus, Inner::State);
    type Sublayout = ResolvedLayout<Inner::Sublayout>;
    type FocusTree = ();

    fn transition(&self) -> Self::Transition {
        Opacity
    }

    fn build_state(&self, captures: &mut C) -> Self::State {
        let status = TextFieldStatus::default();
        let inner = (self.label)(status, self.area(status)).build_state(captures);
        (status, inner)
    }

    fn layout(
        &self,
        offer: &ProposedDimensions,
        env: &impl LayoutEnvironment,
        captures: &mut C,
        state: &mut Self::State,
    ) -> ResolvedLayout<Self::Sublayout> {
        let inner =
            (self.label)(state.0, self.area(state.0)).layout(offer, env, captures, &mut state.1);
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
        (self.label)(state.0, self.area(state.0)).render_tree(
            &layout.sublayouts,
            origin,
            env,
            captures,
            &mut state.1,
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
        let status = &mut state.0;
        match event {
            Event::Focus { action, .. } => {
                if !context.roles.contains(Role::TextEntry) {
                    return EventResult::Deferred;
                }
                self.handle_focus(
                    *action,
                    context,
                    captures,
                    status,
                    render_tree.content_shape(),
                )
            }
            Event::KeyDown(key) if status.focused => {
                self.handle_key(*key, context, captures, status)
            }
            _ => EventResult::Deferred,
        }
    }

    fn seek(
        &self,
        seek: &Seek,
        _render_tree: &Self::Renderables,
        _state: &Self::State,
        _focus: Option<&Self::FocusTree>,
    ) -> SeekResult<Self::FocusTree> {
        match seek.target {
            SeekTarget::Any if seek.roles.contains(Role::TextEntry) => SeekResult::found(()),
            _ => SeekResult::Absent,
        }
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

impl<const N: usize, F: Font, L, S, ViewFn> TextField<'_, N, F, L, S, ViewFn> {
    fn handle_focus<C: ?Sized>(
        &self,
        action: FocusAction,
        context: &EventContext,
        captures: &mut C,
        status: &mut TextFieldStatus,
        shape: ContentShape,
    ) -> EventResult
    where
        L: Fn(&mut C) -> Option<&mut EditBuffer<N>>,
    {
        let before = *status;
        let result = match action {
            FocusAction::Focus(_) => {
                status.focused = true;
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
            FocusAction::Next | FocusAction::Previous | FocusAction::Blur => {
                status.focused = false;
                EventResult::Deferred
            }
        };
        if *status != before {
            context.request_view_rebuild();
        }
        result
    }

    fn handle_key<C: ?Sized>(
        &self,
        key: Key,
        context: &EventContext,
        captures: &mut C,
        status: &mut TextFieldStatus,
    ) -> EventResult
    where
        L: Fn(&mut C) -> Option<&mut EditBuffer<N>>,
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
pub struct TextArea<'a, const N: usize, F: Font, K = ()> {
    buffer: EditBuffer<N>,
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

impl<'a, const N: usize, F: Font> TextArea<'a, N, F> {
    /// Draws the caret, and a scroll bar for text longer than the area, in
    /// `colors`.
    ///
    /// The bar takes room along the trailing edge only while the text needs
    /// it.
    #[must_use]
    pub fn colors<C>(
        self,
        colors: render::AreaColors<C>,
    ) -> TextArea<'a, N, F, render::AreaColors<C>> {
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

impl<const N: usize, F: Font, K> core::fmt::Debug for TextArea<'_, N, F, K> {
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

impl<'a, const N: usize, F: Font, K: Clone> ViewMarker for TextArea<'a, N, F, K> {
    type Renderables = render::TextField<'a, F, N, K>;
    type Transition = Opacity;
}

impl<C: ?Sized, const N: usize, F: Font, K: Clone> ViewLayout<C> for TextArea<'_, N, F, K> {
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
        let wrap = if self.scroll_bar_room > 0
            && EditLines::new(text, width, &metrics)
                .nth(usize::from(self.max_lines))
                .is_some()
        {
            width.saturating_sub(self.scroll_bar_room)
        } else {
            width
        };

        // The line holding the cursor, where a cursor at a break begins the
        // next line, and how far into it the cursor is.
        let cursor = self.buffer.cursor();
        let mut line_count = 0;
        let mut caret = None;
        let mut last = 0..0;
        for (index, line) in EditLines::new(text, wrap, &metrics).enumerate() {
            line_count = index + 1;
            if caret.is_none() && cursor < line.end {
                caret = Some((index, advance_of(&text[line.start..cursor], &metrics)));
            }
            last = line;
        }
        let (caret_line, caret_x) = caret.unwrap_or_else(|| {
            (
                line_count - 1,
                advance_of(&text[last.start..cursor.max(last.start)], &metrics),
            )
        });

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
            caret: layout.caret.clone(),
            scrolled: layout.scrolled,
            style: self.style.clone(),
        }
    }

    fn handle_event(
        &self,
        _event: &Event,
        _context: &EventContext,
        _render_tree: &mut Self::Renderables,
        _captures: &mut C,
        _state: &mut Self::State,
        _focus: &mut Self::FocusTree,
    ) -> EventResult {
        EventResult::Deferred
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
        let mut buffer = EditBuffer::<4>::from("abc");
        assert!(!buffer.insert('é'));
        assert!(buffer.insert('d'));
        assert!(!buffer.insert('e'));
        assert_eq!(buffer.text(), "abcd");
        assert_eq!(buffer.cursor(), 4);
    }

    #[test]
    fn editing_keeps_to_character_boundaries() {
        let mut buffer = EditBuffer::<16>::from("aéb");
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

    #[test]
    fn words_are_stepped_over_by_their_starts() {
        let mut buffer = EditBuffer::<32>::from("one  two three");
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
