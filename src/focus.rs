mod role;

pub use role::{Role, RoleSet};

use crate::{
    event::Event,
    layout::LayoutDirection,
    primitives::{Point, geometry::Rectangle},
};

/// The direction to search when acquiring focus
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FocusDirection {
    /// Search forward (towards the end)
    #[default]
    Forward,
    /// Search backward (towards the beginning)
    Backward,
}

/// A direction on screen, for moving focus with arrow keys or a trackball.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// Toward the top.
    Up,
    /// Toward the bottom.
    Down,
    /// Toward the left.
    Left,
    /// Toward the right.
    Right,
}

impl Direction {
    /// The axis this direction moves along.
    #[must_use]
    pub const fn axis(self) -> LayoutDirection {
        match self {
            Self::Up | Self::Down => LayoutDirection::Vertical,
            Self::Left | Self::Right => LayoutDirection::Horizontal,
        }
    }

    /// The order this direction moves through elements laid out along its
    /// axis: down and right go forward.
    #[must_use]
    pub const fn focus_direction(self) -> FocusDirection {
        match self {
            Self::Down | Self::Right => FocusDirection::Forward,
            Self::Up | Self::Left => FocusDirection::Backward,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusAction {
    /// Move focus to the next element
    Next,
    /// Move focus to the previous element
    Previous,
    /// Obtain the selected container
    ///
    /// If the current container does not match the requested role mask,
    /// the direction will be used to obtain the nearest matching element.
    Focus(FocusDirection),
    /// Move focus on screen toward `dir`, from `from`, the frame of the
    /// focused element.
    ///
    /// A stack moving along its axis treats this as [`Next`](Self::Next) or
    /// [`Previous`](Self::Previous), and enters the element it moves to with
    /// [`Enter`](Self::Enter). Across its axis, it leaves the move to its
    /// focused element and defers when that element does. A directional
    /// move never wraps.
    Toward {
        /// The direction focus moves.
        dir: Direction,
        /// The frame of the focused element, in the coordinates of the view
        /// receiving the event.
        from: Rectangle,
    },
    /// Acquire focus while moving toward `dir` from `from`, as a
    /// [`Toward`](Self::Toward) move enters the element it reaches.
    ///
    /// It searches as [`Focus`](Self::Focus) does, forward for down and right
    /// and backward for up and left, except that a stack entered across its
    /// axis starts at the child nearest `from` along that axis.
    Enter {
        /// The direction focus is moving.
        dir: Direction,
        /// The frame of the element focus is moving from, in the coordinates
        /// of the view receiving the event.
        from: Rectangle,
    },
    /// Exit the selected container.
    ///
    /// Typically associated with the user pressing a "back" or "menu" button.
    Blur,
    /// Perform the focused element's primary action. If the currently focused element
    /// does not match the requested role set, this does nothing.
    Select,
    /// Inform the focused tree that it is being terminated and should perform
    /// any necessary cleanup.
    Teardown,
}

impl FocusAction {
    #[must_use]
    pub fn into_event(self, group: FocusGroup) -> Event {
        Event::Focus {
            action: self,
            group,
        }
    }

    /// The order this action searches through elements in, when it moves
    /// or acquires focus.
    #[must_use]
    pub const fn direction(&self) -> Option<FocusDirection> {
        match self {
            Self::Next | Self::Focus(FocusDirection::Forward) => Some(FocusDirection::Forward),
            Self::Previous | Self::Focus(FocusDirection::Backward) => {
                Some(FocusDirection::Backward)
            }
            Self::Toward { dir, .. } | Self::Enter { dir, .. } => Some(dir.focus_direction()),
            Self::Blur | Self::Select | Self::Teardown => None,
        }
    }

    /// Whether this action acquires focus, rather than moving it on from
    /// an element that has it.
    #[must_use]
    pub const fn is_acquire(&self) -> bool {
        matches!(self, Self::Focus(_) | Self::Enter { .. })
    }

    /// Whether this action moves focus on from the focused element.
    #[must_use]
    pub const fn is_move(&self) -> bool {
        matches!(self, Self::Next | Self::Previous | Self::Toward { .. })
    }

    /// The action that acquires focus in an element this action moves to,
    /// searching in `direction`.
    ///
    /// A directional move enters with [`Enter`](Self::Enter), so the element
    /// can start nearest where focus came from; anything else acquires with
    /// [`Focus`](Self::Focus).
    #[must_use]
    pub const fn entering(&self, direction: FocusDirection) -> Self {
        match *self {
            Self::Toward { dir, from } | Self::Enter { dir, from } => Self::Enter { dir, from },
            _ => Self::Focus(direction),
        }
    }

    /// The same action, with a frame it carries moved by `offset`: for a
    /// view that places its content away from where the content lays itself
    /// out, such as a scroll view.
    #[must_use]
    pub fn offset(self, offset: Point) -> Self {
        match self {
            Self::Toward { dir, from } => Self::Toward {
                dir,
                from: Rectangle::new(from.origin + offset, from.size),
            },
            Self::Enter { dir, from } => Self::Enter {
                dir,
                from: Rectangle::new(from.origin + offset, from.size),
            },
            other => other,
        }
    }

    /// The order a stack laid out along `axis` steps through its children
    /// in once its focused child defers this action, or `None` when it
    /// defers too.
    ///
    /// A stack with no axis, whose children are layers, passes `None`: it
    /// leaves a directional move to its focused layer.
    pub(crate) fn stack_step(&self, axis: Option<LayoutDirection>) -> Option<FocusDirection> {
        match self {
            Self::Select => Some(FocusDirection::Forward),
            Self::Toward { dir, .. } if axis != Some(dir.axis()) => None,
            _ => self.direction(),
        }
    }
}

/// The center of `frame` along `axis`.
pub(crate) const fn center_along(frame: &Rectangle, axis: LayoutDirection) -> i32 {
    match axis {
        LayoutDirection::Horizontal => frame.origin.x + frame.size.width as i32 / 2,
        LayoutDirection::Vertical => frame.origin.y + frame.size.height as i32 / 2,
    }
}

/// The indices `0..len` in order of the distance of their centers from
/// `target`, nearest first, for children laid out in order along an axis.
///
/// Laid out in order, the distances fall to the nearest child and rise after
/// it, so the order is found walking outward from it.
pub(crate) fn nearest_first(
    len: usize,
    center: impl Fn(usize) -> i32,
    target: i32,
) -> impl Iterator<Item = usize> {
    let distance = move |index: usize| center(index).abs_diff(target);
    let nearest = (0..len).min_by_key(|&index| distance(index));
    // The children not yet visited on either side of those visited.
    let mut before = nearest;
    let mut after = nearest.map(|nearest| nearest + 1);
    let mut first = nearest;
    core::iter::from_fn(move || {
        if let Some(index) = first.take() {
            return Some(index);
        }
        let below = before.and_then(|index| index.checked_sub(1));
        let above = after.filter(|&index| index < len);
        let take_below = match (below, above) {
            (Some(below), Some(above)) => distance(below) <= distance(above),
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => return None,
        };
        if take_below {
            before = below;
            below
        } else {
            after = above.map(|index| index + 1);
            above
        }
    })
}

/// A stable name for a focusable element, chosen by the application.
///
/// A key identifies what is focused independently of where it sits in the
/// view tree, so focus can follow an element when its position changes.
/// Keys need only be unique among the elements mounted at the same time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FocusKey(pub u32);

/// Focus as the application sees it, kept in application state and bound to
/// an [`App`](crate::app::App) with
/// [`with_focus_binding`](crate::app::App::with_focus_binding).
///
/// Views read [`current`](Self::current) when what they show depends on
/// focus. Anything that changes application state can ask for focus to move
/// with [`request`](Self::request); the app moves it once the view is
/// settled, if the requested element can be focused there.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FocusState {
    current: Option<FocusKey>,
    request: Option<FocusKey>,
}

impl FocusState {
    /// No focus, and nothing requested.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            current: None,
            request: None,
        }
    }

    /// No focus yet, with a request for `key`.
    #[must_use]
    pub const fn requesting(key: FocusKey) -> Self {
        Self {
            current: None,
            request: Some(key),
        }
    }

    /// The key of the focused element, as of the last time focus settled.
    #[must_use]
    pub const fn current(&self) -> Option<FocusKey> {
        self.current
    }

    /// Asks for focus to move to `key`, replacing any earlier request.
    pub fn request(&mut self, key: FocusKey) {
        self.request = Some(key);
    }

    /// The request not yet acted on.
    #[must_use]
    pub const fn pending(&self) -> Option<FocusKey> {
        self.request
    }

    pub(crate) fn take_request(&mut self) -> Option<FocusKey> {
        self.request.take()
    }

    pub(crate) fn set_current(&mut self, key: Option<FocusKey>) {
        self.current = key;
    }
}

/// What [`seek`](crate::view::ViewLayout::seek) looks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeekTarget {
    /// The element with this key.
    Key(FocusKey),
    /// The first element that can be focused.
    Any,
    /// The innermost open modal, such as a popover's overlay.
    Modal,
    /// The element that can be focused under this point, in the
    /// coordinates of the view sought in.
    At(Point),
}

/// A lookup made with [`seek`](crate::view::ViewLayout::seek).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seek {
    /// What is looked for.
    pub target: SeekTarget,
    /// The roles that may be focused.
    pub roles: RoleSet,
}

impl Seek {
    /// A lookup for `target` among elements with the given roles.
    #[must_use]
    pub const fn new(target: SeekTarget, roles: RoleSet) -> Self {
        Self { target, roles }
    }

    /// The same lookup for a different target.
    #[must_use]
    pub const fn with_target(self, target: SeekTarget) -> Self {
        Self {
            target,
            roles: self.roles,
        }
    }
}

/// Where a [`Seek`] leads in a view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SeekResult<T> {
    /// The target can be focused along `tree`, with focus events in `group`.
    Found {
        /// The path to the target.
        tree: T,
        /// The group focus events must be sent in to reach it.
        group: FocusGroup,
    },
    /// Focus is already inside the target.
    Current,
    /// An open modal keeps focus from the target.
    Blocked,
    /// The target is not here.
    Absent,
}

impl<T> SeekResult<T> {
    /// Found along `tree`, in the default group.
    #[must_use]
    pub fn found(tree: T) -> Self {
        Self::Found {
            tree,
            group: FocusGroup::default(),
        }
    }

    /// Wraps a found path in the caller's focus tree.
    #[must_use]
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> SeekResult<U> {
        match self {
            Self::Found { tree, group } => SeekResult::Found {
                tree: f(tree),
                group,
            },
            Self::Current => SeekResult::Current,
            Self::Blocked => SeekResult::Blocked,
            Self::Absent => SeekResult::Absent,
        }
    }

    /// Sets the group of a found path.
    #[must_use]
    pub fn with_group(self, group: FocusGroup) -> Self {
        match self {
            Self::Found { tree, .. } => Self::Found { tree, group },
            other => other,
        }
    }

    /// Combines the results of two siblings, `self` the earlier.
    ///
    /// A modal holds wherever it sits, so `Blocked` outranks everything;
    /// focus already inside the target stays there; otherwise the earlier
    /// path is taken.
    #[must_use]
    pub fn or(self, later: Self) -> Self {
        match (self, later) {
            (Self::Blocked, _) | (_, Self::Blocked) => Self::Blocked,
            (Self::Current, _) | (_, Self::Current) => Self::Current,
            (found @ Self::Found { .. }, _) | (Self::Absent, found) => found,
        }
    }

    /// Whether the target was found.
    #[must_use]
    pub const fn is_found(&self) -> bool {
        matches!(self, Self::Found { .. })
    }
}

/// A trait for focus tree types that can be initialized to either the first or last element.
///
/// This is roughly equivalent to `Default` but supports bidirectional navigation
pub trait DefaultFocus {
    /// Returns a focus tree initialized to the first element.
    fn default_first() -> Self;

    /// Returns a focus tree initialized to the last element.
    fn default_last() -> Self;
}

impl DefaultFocus for () {
    fn default_first() -> Self {}
    fn default_last() -> Self {}
}

/// A group identifying a set of related elements.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FocusGroup(u8);

/// A set of focus groups.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FocusGroupSet(u8);

impl FocusGroup {
    /// Creates a focus group with the specified group index (0-7)
    #[must_use]
    pub const fn new(i: u8) -> Option<Self> {
        if i < 8 { Some(Self(0b1 << i)) } else { None }
    }

    /// Creates a focus group with the specified group index (0-7)
    #[must_use]
    pub const fn new_unchecked(i: u8) -> Self {
        debug_assert!(i < 8, "Focus group index must be between 0 and 7");
        Self(0b1 << i)
    }

    /// Returns the underlying group index (0-7) for this focus group
    #[must_use]
    pub const fn index(self) -> u8 {
        self.0.trailing_zeros() as u8
    }

    /// Returns a focus group set containing only this focus group
    #[must_use]
    pub const fn set(self) -> FocusGroupSet {
        FocusGroupSet(self.0)
    }
}

impl Default for FocusGroup {
    fn default() -> Self {
        Self::new_unchecked(0)
    }
}

impl FocusGroupSet {
    /// Creates a focus group set matching no focus groups.
    #[must_use]
    pub const fn new_none() -> Self {
        Self(0b0000_0000)
    }

    /// Creates a focus group set matching any focus group.
    #[must_use]
    pub const fn new_any() -> Self {
        Self(0b1111_1111)
    }

    /// Returns true if this set contains the specified focus group
    #[must_use]
    pub const fn contains(self, group: FocusGroup) -> bool {
        (self.0 & group.0) != 0
    }
}

impl core::ops::BitOr for FocusGroup {
    type Output = FocusGroupSet;

    fn bitor(self, rhs: Self) -> Self::Output {
        FocusGroupSet(self.0 | rhs.0)
    }
}

impl From<FocusGroup> for FocusGroupSet {
    fn from(group: FocusGroup) -> Self {
        group.set()
    }
}

impl core::ops::BitOr<FocusGroup> for FocusGroupSet {
    type Output = Self;

    fn bitor(self, rhs: FocusGroup) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl core::ops::BitOr for FocusGroupSet {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl core::ops::BitAnd for FocusGroupSet {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.0)
    }
}

pub static GROUP_0: FocusGroup = FocusGroup(0b1 << 0);
pub static GROUP_1: FocusGroup = FocusGroup(0b1 << 1);
pub static GROUP_2: FocusGroup = FocusGroup(0b1 << 2);
pub static GROUP_3: FocusGroup = FocusGroup(0b1 << 3);
pub static GROUP_4: FocusGroup = FocusGroup(0b1 << 4);
pub static GROUP_5: FocusGroup = FocusGroup(0b1 << 5);
pub static GROUP_6: FocusGroup = FocusGroup(0b1 << 6);
pub static GROUP_7: FocusGroup = FocusGroup(0b1 << 7);

/// Behavior when focus reaches the boundary of the bounded region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BoundaryBehavior {
    /// Wrap focus around to the other side when reaching the end
    #[default]
    Wrap,
    /// Stop movement at the boundaries (focus stays on the current element)
    Stop,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_indices() {
        for i in 0..8 {
            let group = FocusGroup::new(i).unwrap();
            let unchecked_group = FocusGroup::new_unchecked(i);
            assert_eq!(group, unchecked_group);
            assert_eq!(group.index(), i);
        }
    }

    #[test]
    fn children_are_tried_nearest_first() {
        let centers = [0, 10, 20, 30, 40];
        let order: heapless::Vec<usize, 5> =
            nearest_first(centers.len(), |index| centers[index], 22).collect();
        assert_eq!(order, [2, 3, 1, 4, 0]);
        let order: heapless::Vec<usize, 5> =
            nearest_first(centers.len(), |index| centers[index], -5).collect();
        assert_eq!(order, [0, 1, 2, 3, 4]);
        assert_eq!(nearest_first(0, |_| 0, 0).count(), 0);
    }

    #[test]
    fn group_contains_all_groups() {
        assert_eq!(FocusGroupSet::new_any(), FocusGroupSet::new_any());

        for i in 0..8 {
            let group = FocusGroup::new(i).unwrap();
            assert!(FocusGroupSet::new_any().contains(group));
        }
    }
}
