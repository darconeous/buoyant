//! Building values where they are kept.
//!
//! A render tree can be larger than a small device's stack. Returned by
//! value, every level of a tree holds its children on the stack before it
//! moves them into itself, and moves out of an enum or an `Option` are not
//! elided, so building a tree that way takes the stack many times its size.
//!
//! A [`Place`] is somewhere a value is to be built. A view writes its render
//! tree into the place it is given, and a container gives each child the
//! place of the child's part of the tree, so a tree is built where it is
//! kept and the stack holds only what each level needs to work out its part.
//!
//! A place cannot be left empty: the only way to get the [`Built`] that
//! [`ViewLayout::render_tree_in`](crate::view::ViewLayout::render_tree_in)
//! returns is to fill the place it was given, by [writing](Place::write) a
//! value into it or by building each of its parts in their places.
//!
//! This is the one module allowed `unsafe` code for this, and the code using
//! it can stay safe.

#![allow(unsafe_code)]
// These do nothing but pass a place along: called rather than inlined, each
// would be a frame between a container and its child.
#![allow(clippy::inline_always)]

use core::fmt;
use core::marker::PhantomData;
use core::mem::MaybeUninit;

/// Ties a [`Place`] to the [`Built`] that proves it was filled.
///
/// Invariant, so that no place's proof stands for another's: every place is
/// made with a lifetime of its own (see [`build_in`]).
type Brand<'p> = PhantomData<fn(&'p ()) -> &'p ()>;

/// Somewhere a `T` is to be built.
///
/// A place is filled by [`write`](Self::write), or by building each of its
/// parts in their places (as [`build_each`](Place::build_each) does for a
/// tuple), and either way gives the [`Built`] that proves it was.
#[must_use = "a place must be filled"]
pub struct Place<'p, T> {
    slot: &'p mut MaybeUninit<T>,
    brand: Brand<'p>,
}

/// Proof that the [`Place`] of the same lifetime was filled.
#[must_use = "the proof that a place was filled is returned to whoever gave it"]
pub struct Built<'p> {
    brand: Brand<'p>,
}

impl<T> fmt::Debug for Place<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Place").finish_non_exhaustive()
    }
}

impl fmt::Debug for Built<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Built").finish()
    }
}

impl<'p, T> Place<'p, T> {
    /// A place for the value `slot` will hold.
    ///
    /// # Safety
    ///
    /// `'p` belongs to this place alone: it is a lifetime made for it by a
    /// higher-ranked function (`for<'p>`), so that no other place's [`Built`]
    /// can be given for it.
    #[inline(always)]
    unsafe fn new(slot: &'p mut MaybeUninit<T>) -> Self {
        Self {
            slot,
            brand: PhantomData,
        }
    }

    /// Fills the place with `value`.
    #[inline(always)]
    pub fn write(self, value: T) -> Built<'p> {
        self.slot.write(value);
        Built { brand: PhantomData }
    }

    /// Where the value is to be built.
    #[inline(always)]
    pub(crate) fn as_mut_ptr(&mut self) -> *mut T {
        self.slot.as_mut_ptr()
    }

    /// The proof that this place was filled.
    ///
    /// # Safety
    ///
    /// The place holds an initialized `T`.
    #[inline(always)]
    #[allow(clippy::unused_self)] // Taken, so that the place is used no more.
    pub(crate) unsafe fn assume_built(self) -> Built<'p> {
        Built { brand: PhantomData }
    }

    /// Fills the place with `build`, to be looked at before it is kept: see
    /// [`Filled`].
    #[inline(always)]
    pub fn fill(self, build: impl for<'b> FnOnce(Place<'b, T>) -> Built<'b>) -> Filled<'p, T> {
        build_in(self.slot, build);
        Filled {
            slot: self.slot,
            brand: PhantomData,
        }
    }
}

/// A [`Place`] that was filled, and that can be kept, or emptied to be filled
/// again.
///
/// Dropped, it drops what it holds.
pub struct Filled<'p, T> {
    slot: &'p mut MaybeUninit<T>,
    brand: Brand<'p>,
}

impl<T> fmt::Debug for Filled<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Filled").finish_non_exhaustive()
    }
}

impl<'p, T> Filled<'p, T> {
    /// What the place holds.
    #[inline(always)]
    pub fn get_mut(&mut self) -> &mut T {
        // SAFETY: a filled place holds an initialized `T`.
        unsafe { self.slot.assume_init_mut() }
    }

    /// Keeps what the place holds, and gives the proof that it was filled.
    #[inline(always)]
    pub fn keep(self) -> Built<'p> {
        core::mem::forget(self);
        Built { brand: PhantomData }
    }

    /// Drops what the place holds, and gives the place back to be filled
    /// again.
    #[inline(always)]
    pub fn clear(self) -> Place<'p, T> {
        let this = core::mem::ManuallyDrop::new(self);
        // SAFETY: `this` is not dropped, so its slot is moved out once, and
        // what it holds is dropped once, here.
        unsafe {
            let slot = core::ptr::read(&raw const this.slot);
            slot.assume_init_drop();
            Place::new(slot)
        }
    }
}

impl<T> Drop for Filled<'_, T> {
    fn drop(&mut self) {
        // SAFETY: a filled place holds an initialized `T`.
        unsafe { self.slot.assume_init_drop() }
    }
}

/// Builds a `T` in `slot` with `build`, and returns it.
///
/// Whatever `slot` held is overwritten without being dropped.
#[inline(always)]
pub fn build_in<T>(
    slot: &mut MaybeUninit<T>,
    build: impl for<'b> FnOnce(Place<'b, T>) -> Built<'b>,
) -> &mut T {
    // SAFETY: `build` takes a place of any lifetime, so the one it is given
    // here is the place's own.
    let _built = build(unsafe { Place::new(slot) });
    // SAFETY: the only way `build` can return is by filling the place it was
    // given, which is the slot.
    unsafe { slot.assume_init_mut() }
}

/// Builds a `T` with `build`, and returns it.
///
/// The value is built on the stack. Containers give their children places in
/// themselves instead (see [`Place`]).
#[inline(always)]
pub fn build_on_stack<T>(build: impl for<'b> FnOnce(Place<'b, T>) -> Built<'b>) -> T {
    let mut slot = MaybeUninit::uninit();
    build_in(&mut slot, build);
    // SAFETY: `build_in` initialized the slot.
    unsafe { slot.assume_init() }
}

/// Builds `*ptr` with `build`.
///
/// # Safety
///
/// `ptr` is valid for writes and aligned, and nothing else uses what it
/// points to while `build` runs.
#[inline(always)]
pub(crate) unsafe fn build_at<T>(
    ptr: *mut T,
    build: impl for<'b> FnOnce(Place<'b, T>) -> Built<'b>,
) {
    // SAFETY: the caller provides a valid place; `build` takes a place of any
    // lifetime, so the one it is given here is the place's own.
    let _built = build(unsafe { Place::new(&mut *ptr.cast::<MaybeUninit<T>>()) });
}

/// Fixes the type of a closure that builds the `T` at `_ptr`.
///
/// A closure takes a higher-ranked signature, and the type of its place, only
/// when it is passed straight to a function that asks for them, which
/// [`build!`] cannot do while keeping the closure out of its `unsafe` block.
#[inline(always)]
pub(crate) fn builder<T, F>(_ptr: *mut T, build: F) -> F
where
    F: for<'b> FnOnce(Place<'b, T>) -> Built<'b>,
{
    build
}

/// Type-checks the closure [`build!`] uses to see that every field of the
/// struct built at `ptr` is named.
#[inline(always)]
pub(crate) fn check_fields<T>(_ptr: *mut T, _names: impl FnOnce(&T)) {}

/// Fills the place of a struct, a field at a time, and gives its [`Built`].
///
/// `field: value` writes a value into a field, and `field <- |place| ...`
/// builds a field in its own place. Every field must be named once, or this
/// does not compile.
///
/// ```ignore
/// build!(place => Opacity {
///     opacity: self.opacity,
///     subtree <- |place| self.inner.render_tree_in(layout, origin, env, captures, state, place),
/// })
/// ```
///
/// Should a field's value or builder panic, the fields filled before it leak.
macro_rules! build {
    ($place:expr => $($ty:ident)::+ { $($fields:tt)* }) => {
        $crate::place::build!(@fields [$place] [$($ty)::+] [] [] $($fields)*)
    };
    (@fields $place:tt $ty:tt [$($names:tt)*] [$($steps:tt)*]
        $field:tt : $value:expr $(, $($rest:tt)*)?) => {
        $crate::place::build!(@fields $place $ty [$($names)* $field]
            [$($steps)* (write $field $value)] $($($rest)*)?)
    };
    (@fields $place:tt $ty:tt [$($names:tt)*] [$($steps:tt)*]
        $field:tt <- $build:expr $(, $($rest:tt)*)?) => {
        $crate::place::build!(@fields $place $ty [$($names)* $field]
            [$($steps)* (build $field $build)] $($($rest)*)?)
    };
    (@fields $place:tt $ty:tt [$($names:tt)*] [$($steps:tt)*]
        $field:ident $(, $($rest:tt)*)?) => {
        $crate::place::build!(@fields $place $ty [$($names)* $field]
            [$($steps)* (write $field $field)] $($($rest)*)?)
    };
    (@fields [$place:expr] [$($ty:tt)*] [$($names:tt)*] [$(($how:ident $field:tt $value:expr))*]) => {{
        let mut place = $place;
        let ptr = place.as_mut_ptr();
        $($crate::place::build! { @step ptr $how $field $value })*
        $crate::place::check_fields(ptr, |value| {
            let $($ty)* { $($names: _),* } = value;
        });
        #[allow(unsafe_code)]
        // SAFETY: every field was filled, as the check above makes certain.
        let built = unsafe { place.assume_built() };
        built
    }};
    (@step $ptr:ident write $field:tt $value:expr) => {
        let value = $value;
        #[allow(unsafe_code)]
        // SAFETY: a field of a place is a place.
        unsafe {
            (&raw mut (*$ptr).$field).write(value);
        };
    };
    (@step $ptr:ident build $field:tt $build:expr) => {
        #[allow(unsafe_code)]
        // SAFETY: a field of a place is a place.
        let field = unsafe { &raw mut (*$ptr).$field };
        let build = $crate::place::builder(field, $build);
        #[allow(unsafe_code)]
        // SAFETY: no other part of the place is used while the field is built.
        unsafe {
            $crate::place::build_at(field, build);
        };
    };
}
pub(crate) use build;

/// Builds the variant of a `#[repr(u8)]` enum whose one field is a `P`.
///
/// # Safety
///
/// `E` is `#[repr(u8)]`, and `tag` is the discriminant of a variant whose
/// only field is a `P`.
#[inline(always)]
pub(crate) unsafe fn build_variant<'p, E, P>(
    mut place: Place<'p, E>,
    tag: u8,
    build: impl for<'b> FnOnce(Place<'b, P>) -> Built<'b>,
) -> Built<'p> {
    /// A variant of one field of a `#[repr(u8)]` enum (RFC 2195).
    #[repr(C)]
    struct Variant<P> {
        tag: u8,
        payload: P,
    }
    let ptr = place.as_mut_ptr().cast::<Variant<P>>();
    // SAFETY: RFC 2195 lays a variant of a `#[repr(u8)]` enum out as a
    // `#[repr(C)]` struct of the tag and the variant's fields, so the tag and
    // the payload are places, and with both filled so is the enum.
    unsafe {
        build_at(&raw mut (*ptr).payload, build);
        (&raw mut (*ptr).tag).write(tag);
        place.assume_built()
    }
}

macro_rules! impl_build_each {
    ($($t:ident $n:tt $b:lifetime),+) => {
        impl<'p, $($t),+> Place<'p, ($($t,)+)> {
            /// Fills the place of a tuple by building each element in its
            /// own place.
            #[inline(always)]
            pub fn build_each(
                mut self,
                build: impl for<$($b),+> FnOnce($(Place<$b, $t>),+) -> ($(Built<$b>,)+),
            ) -> Built<'p> {
                let ptr = self.as_mut_ptr();
                // SAFETY: the elements of a tuple are distinct places within
                // it, and `build` takes places of any lifetimes, so each one
                // it is given is the place's own.
                let _built = unsafe {
                    build($(Place::new(&mut *(&raw mut (*ptr).$n).cast::<MaybeUninit<$t>>())),+)
                };
                // SAFETY: `build` returned, so every element was filled.
                unsafe { self.assume_built() }
            }
        }
    };
}

impl_build_each!(T0 0 'b0);
impl_build_each!(T0 0 'b0, T1 1 'b1);
impl_build_each!(T0 0 'b0, T1 1 'b1, T2 2 'b2);
impl_build_each!(T0 0 'b0, T1 1 'b1, T2 2 'b2, T3 3 'b3);
impl_build_each!(T0 0 'b0, T1 1 'b1, T2 2 'b2, T3 3 'b3, T4 4 'b4);
impl_build_each!(T0 0 'b0, T1 1 'b1, T2 2 'b2, T3 3 'b3, T4 4 'b4, T5 5 'b5);
impl_build_each!(T0 0 'b0, T1 1 'b1, T2 2 'b2, T3 3 'b3, T4 4 'b4, T5 5 'b5, T6 6 'b6);
impl_build_each!(T0 0 'b0, T1 1 'b1, T2 2 'b2, T3 3 'b3, T4 4 'b4, T5 5 'b5, T6 6 'b6, T7 7 'b7);
impl_build_each!(
    T0 0 'b0, T1 1 'b1, T2 2 'b2, T3 3 'b3, T4 4 'b4, T5 5 'b5, T6 6 'b6, T7 7 'b7, T8 8 'b8
);
impl_build_each!(
    T0 0 'b0, T1 1 'b1, T2 2 'b2, T3 3 'b3, T4 4 'b4, T5 5 'b5, T6 6 'b6, T7 7 'b7, T8 8 'b8,
    T9 9 'b9
);

/// Builds a value at the end of `vec`, and returns it, or `None` without
/// calling `build` when `vec` is full.
#[inline(always)]
pub(crate) fn push_in<T, const N: usize>(
    vec: &mut heapless::Vec<T, N>,
    build: impl for<'b> FnOnce(Place<'b, T>) -> Built<'b>,
) -> Option<&mut T> {
    let len = vec.len();
    let slot = vec.spare_capacity_mut().first_mut()?;
    build_in(slot, build);
    // SAFETY: the element after the last was just initialized.
    unsafe { vec.set_len(len + 1) };
    vec.last_mut()
}

/// Makes the place `to` a clone of `from` should it unwind while this is
/// held.
///
/// Something built in place was dropped first, to be built again where it
/// was. Should building it panic, and the panic be caught, what owns it stays
/// whole, holding a copy of `from` instead.
#[derive(Debug)]
pub(crate) struct CloneOnUnwind<T: Clone> {
    from: *const T,
    to: *mut T,
}

impl<T: Clone> CloneOnUnwind<T> {
    /// # Safety
    ///
    /// `from` holds a `T`, and `to` is a place for one, apart from it. Both
    /// stay valid, and nothing else uses `from`, for as long as this is held.
    #[inline(always)]
    pub(crate) unsafe fn new(from: *const T, to: *mut T) -> Self {
        Self { from, to }
    }

    /// Disarms the guard, once what it guards is whole again.
    #[inline(always)]
    pub(crate) fn disarm(self) {
        core::mem::forget(self);
    }
}

impl<T: Clone> Drop for CloneOnUnwind<T> {
    fn drop(&mut self) {
        // SAFETY: as `new` was promised. What was built of `to` before the
        // panic leaks.
        unsafe { self.to.write((*self.from).clone()) }
    }
}

/// Aborts the program should it unwind while this is held.
///
/// Something built in place may have been dropped first, to be built again
/// where it was. Were building it to panic and the panic be caught, what owns
/// it would drop it a second time; aborting instead keeps that from happening.
#[derive(Debug)]
pub(crate) struct AbortOnUnwind;

impl AbortOnUnwind {
    /// Disarms the guard, once whatever it guards is whole again.
    #[inline(always)]
    pub(crate) fn disarm(self) {
        core::mem::forget(self);
    }
}

impl Drop for AbortOnUnwind {
    fn drop(&mut self) {
        // A panic while unwinding aborts.
        panic!("a view panicked while being rebuilt in place");
    }
}
