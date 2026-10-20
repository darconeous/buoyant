use core::{cell::Cell, time::Duration};

use crate::{
    focus::{self, FocusAction, FocusGroup, FocusKey, RoleSet},
    primitives::Point,
    render::ContentShape,
};

/// An interaction event that can be handled by a view.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// A scroll event with the given offset.
    Scroll(Point),
    /// A request to move focus, often driven by navigational buttons
    /// or an encoder.
    Focus {
        /// The focus action to perform.
        action: FocusAction,
        /// The focus group this event targets.
        group: FocusGroup,
    },
    /// A key was pressed.
    KeyDown(Key),
    /// A key was released.
    KeyUp(Key),
    /// A [`TextField`](crate::view::TextField) asking the text area in its
    /// label how the area lays out text. Views pass it on as they do a
    /// scroll.
    TextArea(TextAreaQuery),
}

/// How a text area lays out text, asked for by the
/// [`TextField`](crate::view::TextField) it belongs to, which edits the text
/// it shows.
///
/// Only Buoyant makes one.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextAreaQuery {
    pub(crate) answer: Cell<Option<TextAreaGeometry>>,
}

/// The answer to a [`TextAreaQuery`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextAreaGeometry {
    /// Where the area is.
    pub(crate) origin: Point,
    /// The width of the area.
    pub(crate) width: u32,
    /// The width a scroll bar takes from the text, when the text needs one.
    pub(crate) scroll_bar_room: u32,
    /// The height of a line.
    pub(crate) line_height: u32,
    /// The first line shown.
    pub(crate) first_line: u16,
}

/// A key press event.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// A character key.
    Character(char),
    /// The up arrow key.
    UpArrow,
    /// The down arrow key.
    DownArrow,
    /// The left arrow key.
    LeftArrow,
    /// The right arrow key.
    RightArrow,
    /// The escape key.
    Escape,
    /// The backspace key.
    Backspace,
    /// The delete key.
    Delete,
    /// The enter or return key.
    ///
    /// Distinct from a typed `'\n'`: what it does is up to the view, such as
    /// sending what a text field holds.
    Enter,
    /// A step to the next word, such as from a quickly spun wheel.
    ///
    /// A text field moves its cursor by a word; elsewhere it can be taken as
    /// a larger step through focus.
    NextWord,
    /// A step to the previous word. See [`Key::NextWord`].
    PreviousWord,
}

impl From<FocusAction> for Event {
    fn from(action: FocusAction) -> Self {
        Self::Focus {
            action,
            group: focus::GROUP_0,
        }
    }
}

impl Event {
    /// Returns a new event with the specified focus group set on Focus events.
    ///
    /// Non-focus events are returned unchanged.
    #[must_use]
    pub fn with_focus_group(mut self, group: FocusGroup) -> Self {
        if let Self::Focus {
            group: ref mut g, ..
        } = self
        {
            *g = group;
        }
        self
    }
}

/// The result of handling a touch event.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum TouchResult<T> {
    /// The subtree handled the touch, and focus should updated
    Focused(T),
    /// The subtree handled the touch, but focus should not be changed
    Handled,
    /// The touch was not handled by the subtree
    #[default]
    Deferred,
}

/// The result of handling an event.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum EventResult {
    /// Focus successfully moved to a new element
    Handled {
        /// The content shape of the focused element
        shape: ContentShape,
        /// The group of the focused element
        group: FocusGroup,
        /// The key of the focused element, when it has one.
        key: Option<FocusKey>,
    },
    /// The event was not handled, or focus not obtained
    #[default]
    Deferred,
}

impl EventResult {
    /// Creates a new `EventResult` indicating the event was handled but focus was not obtained.
    #[must_use]
    pub const fn handled_unfocused() -> Self {
        Self::Handled {
            shape: ContentShape::Empty,
            group: focus::GROUP_0,
            key: None,
        }
    }

    /// Creates a new `EventResult` indicating the event was handled with focus.
    #[must_use]
    pub const fn handled_focused(shape: ContentShape) -> Self {
        Self::Handled {
            shape,
            group: focus::GROUP_0,
            key: None,
        }
    }

    /// Returns a new [`EventResult`] with the specified focus group.
    #[must_use]
    pub const fn with_group(mut self, group: FocusGroup) -> Self {
        let Self::Handled {
            group: event_group, ..
        } = &mut self
        else {
            return Self::Deferred;
        };
        *event_group = group;
        self
    }

    /// Returns a new [`EventResult`] naming `key` as the focused element.
    ///
    /// A key already set by a nearer element is kept, and a result without a
    /// focused shape is left unnamed: nothing is focused there.
    #[must_use]
    pub fn with_key(mut self, key: FocusKey) -> Self {
        if let Self::Handled {
            shape,
            key: event_key,
            ..
        } = &mut self
            && *shape != ContentShape::Empty
            && event_key.is_none()
        {
            *event_key = Some(key);
        }
        self
    }

    /// The key of the focused element, if the event was handled and the
    /// element has one.
    #[must_use]
    pub const fn key(&self) -> Option<FocusKey> {
        match self {
            Self::Handled { key, .. } => *key,
            Self::Deferred => None,
        }
    }

    /// The group of the focused element, if the event was handled.
    #[must_use]
    pub const fn group(&self) -> Option<FocusGroup> {
        match self {
            Self::Handled { group, .. } => Some(*group),
            Self::Deferred => None,
        }
    }

    /// Returns true if the event was handled (not deferred).
    #[must_use]
    pub const fn is_handled(&self) -> bool {
        matches!(self, Self::Handled { .. })
    }

    /// Returns the content shape if this result has one.
    #[must_use]
    pub fn shape(&self) -> Option<&ContentShape> {
        match self {
            Self::Handled { shape, .. } => Some(shape),
            Self::Deferred => None,
        }
    }

    /// Returns the result offset by the given amount if it has a shape.
    #[must_use]
    pub fn with_offset(mut self, offset: Point) -> Self {
        match &mut self {
            Self::Handled { shape, .. } => {
                shape.offset(offset);
            }
            Self::Deferred => (),
        }
        self
    }
}

/// Context provided to views when handling events.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventContext {
    /// The time since the application started.
    pub app_time: Duration,
    /// Whether a redraw has been requested.
    pub redraw_requested: Cell<bool>,
    /// Whether a view rebuild has been requested.
    pub view_rebuild_requested: Cell<bool>,
    /// Whether the key being handled should be delivered again once focus
    /// has moved, as [`KeyResponse::Redirect`] asks.
    pub redirect_requested: Cell<bool>,
    /// Which roles are currently active.
    pub roles: RoleSet,
}

impl EventContext {
    /// Creates a new `EventContext` with the given application time.
    #[must_use]
    pub const fn new(app_time: Duration) -> Self {
        Self {
            app_time,
            redraw_requested: Cell::new(false),
            view_rebuild_requested: Cell::new(false),
            redirect_requested: Cell::new(false),
            roles: RoleSet::any(),
        }
    }

    #[must_use]
    pub fn with_roles(mut self, roles: impl Into<RoleSet>) -> Self {
        self.roles = roles.into();
        self
    }

    /// Indicates to the render loop that the view needs to be rebuilt.
    pub fn request_view_rebuild(&self) {
        self.view_rebuild_requested.set(true);
    }

    /// This flag indicates the view should be redrawn even if no animations were reported as
    /// active.
    ///
    /// This should be set when a view directly modifies the render tree state
    /// without requesting a view recompute, e.g. scrollview dragging.
    pub fn request_redraw(&self) {
        self.redraw_requested.set(true);
    }

    /// Asks for the key being handled to be delivered again to the element
    /// focus moves to, as [`KeyResponse::Redirect`] does.
    pub fn request_redirect(&self) {
        self.redirect_requested.set(true);
    }
}

/// What an [`on_key`](crate::view::ViewModifier::on_key) handler did with a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyResponse {
    /// The key is not for this handler; it is offered further up.
    Ignored,
    /// The key was handled.
    Handled,
    /// The handler requested focus elsewhere, and the key belongs there.
    ///
    /// Once focus has settled, if it is on the requested element, the same
    /// key is delivered to it, once. Otherwise the key is dropped.
    Redirect,
}

#[cfg(feature = "embedded-graphics-simulator")]
pub mod simulator {
    use crate::{event::Key, primitives::Point};

    use super::Event;
    use embedded_graphics_simulator::{SimulatorEvent, sdl2::Keycode};
    use embedded_touch::{Phase, PointerButton, Tool, Touch, TouchPoint};

    /// An input event produced by the simulator.
    #[derive(Debug, Clone, PartialEq)]
    pub enum InputEvent {
        /// A non-touch event (scroll, focus, key, etc.)
        Event(Event),
        /// A touch event, routed through `App::send_touch`.
        Touch(Touch),
    }

    /// Tracks mouse state and converts simulator events to touch events.
    #[derive(Debug, Default)]
    pub struct MouseTracker {
        touch: Option<Touch>,
    }

    impl MouseTracker {
        /// Creates a new mouse tracker.
        #[must_use]
        pub fn new() -> Self {
            Self { touch: None }
        }

        /// Processes a simulator event and returns the corresponding input event.
        pub fn process_event(&mut self, event: SimulatorEvent) -> Option<InputEvent> {
            match event {
                SimulatorEvent::MouseButtonDown { point, mouse_btn } => {
                    let touch = Touch {
                        id: 0,
                        location: TouchPoint::new(point.x, point.y),
                        phase: Phase::Started,
                        tool: Tool::Pointer {
                            button: map_button(mouse_btn),
                        },
                    };
                    self.touch = Some(touch.clone());
                    Some(InputEvent::Touch(touch))
                }
                SimulatorEvent::MouseButtonUp { point, mouse_btn } => {
                    let touch = Touch {
                        id: 0,
                        location: TouchPoint::new(point.x, point.y),
                        phase: Phase::Ended,
                        tool: Tool::Pointer {
                            button: map_button(mouse_btn),
                        },
                    };

                    self.touch = None;
                    Some(InputEvent::Touch(touch))
                }
                SimulatorEvent::MouseMove { point } => {
                    if let Some(touch) = &mut self.touch {
                        touch.location = TouchPoint::new(point.x, point.y);
                        touch.phase = Phase::Moved;
                        Some(InputEvent::Touch(touch.clone()))
                    } else {
                        let touch = Touch {
                            id: 0,
                            location: TouchPoint::new(point.x, point.y),
                            phase: Phase::Hovering(None),
                            tool: Tool::Pointer {
                                button: PointerButton::None,
                            },
                        };

                        Some(InputEvent::Touch(touch))
                    }
                }
                SimulatorEvent::MouseWheel {
                    scroll_delta,
                    direction,
                } => {
                    if direction == embedded_graphics_simulator::sdl2::MouseWheelDirection::Flipped
                    {
                        Some(InputEvent::Event(Event::Scroll(Point::new(
                            scroll_delta.x * 4,
                            scroll_delta.y * 4,
                        ))))
                    } else {
                        Some(InputEvent::Event(Event::Scroll(Point::new(
                            -scroll_delta.x * 4,
                            -scroll_delta.y * 4,
                        ))))
                    }
                }
                SimulatorEvent::Quit => None,
                SimulatorEvent::KeyDown {
                    keycode,
                    keymod: _,
                    repeat: _,
                } => keycode
                    .try_into()
                    .ok()
                    .map(|k| InputEvent::Event(Event::KeyDown(k))),
                SimulatorEvent::KeyUp {
                    keycode,
                    keymod: _,
                    repeat: _,
                } => keycode
                    .try_into()
                    .ok()
                    .map(|k| InputEvent::Event(Event::KeyUp(k))),
            }
        }
    }

    fn map_button(mouse_btn: embedded_graphics_simulator::sdl2::MouseButton) -> PointerButton {
        match mouse_btn {
            embedded_graphics_simulator::sdl2::MouseButton::Left => PointerButton::Primary,
            embedded_graphics_simulator::sdl2::MouseButton::Right => PointerButton::Secondary,
            embedded_graphics_simulator::sdl2::MouseButton::Middle => PointerButton::Tertiary,
            _ => PointerButton::None,
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct UnknownKeycode;

    impl TryFrom<embedded_graphics_simulator::sdl2::Keycode> for Key {
        type Error = UnknownKeycode;

        #[allow(clippy::too_many_lines)]
        fn try_from(
            value: embedded_graphics_simulator::sdl2::Keycode,
        ) -> Result<Self, Self::Error> {
            // FIXME: this is incomplete, sometimes wrong
            match value {
                // Arrow keys
                Keycode::RIGHT => Ok(Self::RightArrow),
                Keycode::LEFT => Ok(Self::LeftArrow),
                Keycode::DOWN => Ok(Self::DownArrow),
                Keycode::UP => Ok(Self::UpArrow),

                // Special keys
                Keycode::ESCAPE => Ok(Self::Escape),
                Keycode::DELETE => Ok(Self::Delete),
                Keycode::BACKSPACE | Keycode::KP_BACKSPACE => Ok(Self::Backspace),

                // Whitespace and control
                Keycode::TAB | Keycode::KP_TAB => Ok(Self::Character('\t')),
                Keycode::RETURN | Keycode::KP_ENTER => Ok(Self::Enter),
                Keycode::SPACE | Keycode::KP_SPACE => Ok(Self::Character(' ')),

                // Punctuation (combined regular and keypad)
                Keycode::EXCLAIM | Keycode::KP_EXCLAM => Ok(Self::Character('!')),
                Keycode::QUOTEDBL => Ok(Self::Character('"')),
                Keycode::HASH | Keycode::KP_HASH => Ok(Self::Character('#')),
                Keycode::DOLLAR => Ok(Self::Character('$')),
                Keycode::PERCENT | Keycode::KP_PERCENT => Ok(Self::Character('%')),
                Keycode::AMPERSAND | Keycode::KP_AMPERSAND => Ok(Self::Character('&')),
                Keycode::QUOTE => Ok(Self::Character('\'')),
                Keycode::LEFTPAREN | Keycode::KP_LEFTPAREN => Ok(Self::Character('(')),
                Keycode::RIGHTPAREN | Keycode::KP_RIGHTPAREN => Ok(Self::Character(')')),
                Keycode::ASTERISK | Keycode::KP_MULTIPLY => Ok(Self::Character('*')),
                Keycode::PLUS | Keycode::KP_PLUS => Ok(Self::Character('+')),
                Keycode::COMMA | Keycode::KP_COMMA => Ok(Self::Character(',')),
                Keycode::MINUS | Keycode::KP_MINUS => Ok(Self::Character('-')),
                Keycode::PERIOD | Keycode::KP_PERIOD => Ok(Self::Character('.')),
                Keycode::SLASH | Keycode::KP_DIVIDE => Ok(Self::Character('/')),
                Keycode::COLON | Keycode::KP_COLON => Ok(Self::Character(':')),
                Keycode::SEMICOLON => Ok(Self::Character(';')),
                Keycode::LESS | Keycode::KP_LESS => Ok(Self::Character('<')),
                Keycode::EQUALS | Keycode::KP_EQUALS => Ok(Self::Character('=')),
                Keycode::GREATER | Keycode::KP_GREATER => Ok(Self::Character('>')),
                Keycode::QUESTION => Ok(Self::Character('?')),
                Keycode::AT | Keycode::KP_AT => Ok(Self::Character('@')),
                Keycode::LEFTBRACKET => Ok(Self::Character('[')),
                Keycode::BACKSLASH => Ok(Self::Character('\\')),
                Keycode::RIGHTBRACKET => Ok(Self::Character(']')),
                Keycode::CARET | Keycode::KP_XOR => Ok(Self::Character('^')),
                Keycode::UNDERSCORE => Ok(Self::Character('_')),
                Keycode::BACKQUOTE => Ok(Self::Character('`')),
                Keycode::KP_LEFTBRACE => Ok(Self::Character('{')),
                Keycode::KP_RIGHTBRACE => Ok(Self::Character('}')),
                Keycode::KP_VERTICALBAR => Ok(Self::Character('|')),

                // Numbers (combined regular and keypad)
                Keycode::NUM_0 | Keycode::KP_0 => Ok(Self::Character('0')),
                Keycode::NUM_1 | Keycode::KP_1 => Ok(Self::Character('1')),
                Keycode::NUM_2 | Keycode::KP_2 => Ok(Self::Character('2')),
                Keycode::NUM_3 | Keycode::KP_3 => Ok(Self::Character('3')),
                Keycode::NUM_4 | Keycode::KP_4 => Ok(Self::Character('4')),
                Keycode::NUM_5 | Keycode::KP_5 => Ok(Self::Character('5')),
                Keycode::NUM_6 | Keycode::KP_6 => Ok(Self::Character('6')),
                Keycode::NUM_7 | Keycode::KP_7 => Ok(Self::Character('7')),
                Keycode::NUM_8 | Keycode::KP_8 => Ok(Self::Character('8')),
                Keycode::NUM_9 | Keycode::KP_9 => Ok(Self::Character('9')),

                // Letters (combined regular and keypad)
                Keycode::A | Keycode::KP_A => Ok(Self::Character('a')),
                Keycode::B | Keycode::KP_B => Ok(Self::Character('b')),
                Keycode::C | Keycode::KP_C => Ok(Self::Character('c')),
                Keycode::D | Keycode::KP_D => Ok(Self::Character('d')),
                Keycode::E | Keycode::KP_E => Ok(Self::Character('e')),
                Keycode::F | Keycode::KP_F => Ok(Self::Character('f')),
                Keycode::G => Ok(Self::Character('g')),
                Keycode::H => Ok(Self::Character('h')),
                Keycode::I => Ok(Self::Character('i')),
                Keycode::J => Ok(Self::Character('j')),
                Keycode::K => Ok(Self::Character('k')),
                Keycode::L => Ok(Self::Character('l')),
                Keycode::M => Ok(Self::Character('m')),
                Keycode::N => Ok(Self::Character('n')),
                Keycode::O => Ok(Self::Character('o')),
                Keycode::P => Ok(Self::Character('p')),
                Keycode::Q => Ok(Self::Character('q')),
                Keycode::R => Ok(Self::Character('r')),
                Keycode::S => Ok(Self::Character('s')),
                Keycode::T => Ok(Self::Character('t')),
                Keycode::U => Ok(Self::Character('u')),
                Keycode::V => Ok(Self::Character('v')),
                Keycode::W => Ok(Self::Character('w')),
                Keycode::X => Ok(Self::Character('x')),
                Keycode::Y => Ok(Self::Character('y')),
                Keycode::Z => Ok(Self::Character('z')),

                _ => Err(UnknownKeycode),
            }
        }
    }
}
