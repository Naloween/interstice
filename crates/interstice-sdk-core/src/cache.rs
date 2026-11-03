//! Instance caches: module memory that is *derived from tables*.
//!
//! Interstice state lives in tables. Module memory is not state: a module
//! runs in a pool of instances, any of which may serve the next call, and an
//! instance is thrown away (replaced by a fresh one) when it traps. What
//! module memory *can* hold is a cache — data that is expensive to rebuild
//! (a parsed document, a layout tree, a decoded image) but that can always be
//! rebuilt from table contents.
//!
//! [`InstanceCache`] is that pattern made explicit:
//!
//! * it is per instance — every instance builds its own copy on demand;
//! * it is dropped automatically when the host signals that this module's
//!   effects were rolled back after the instance saw them (a rejected commit
//!   bumps [`interstice_abi::RawReducerContext::state_epoch`]);
//! * the caller supplies a `valid` check against the tables, so a cache that
//!   another instance made stale (it applied newer table changes) is caught
//!   and either caught up or rebuilt.
//!
//! ```ignore
//! instance_cache!(static DOCS: HashMap<u64, ParsedDoc>;);
//!
//! DOCS.with(|cache| cache.with(HashMap::new, |docs| { /* use / update */ }));
//! ```

use std::cell::{Cell, RefCell};

thread_local! {
    static EPOCH: Cell<u64> = const { Cell::new(0) };
}

/// Called by the context conversions at the start of every reducer / query.
#[doc(hidden)]
pub fn __observe_state_epoch(epoch: u64) {
    EPOCH.with(|e| e.set(epoch));
}

/// The module's current state epoch (see the module docs).
pub fn state_epoch() -> u64 {
    EPOCH.with(|e| e.get())
}

/// A per-instance cache of table-derived data. Declare with [`instance_cache!`].
pub struct InstanceCache<T> {
    slot: RefCell<Option<(u64, T)>>,
}

impl<T> InstanceCache<T> {
    pub const fn new() -> Self {
        Self {
            slot: RefCell::new(None),
        }
    }

    /// Run `f` on the cached value, building it with `build` if it is
    /// missing or from an older state epoch.
    pub fn with<R>(&self, build: impl FnOnce() -> T, f: impl FnOnce(&mut T) -> R) -> R {
        self.with_valid(|_| true, build, f)
    }

    /// Like [`with`](Self::with), but also rebuilds when `valid` rejects the
    /// cached value (e.g. it no longer matches the tables it was derived from).
    pub fn with_valid<R>(
        &self,
        valid: impl FnOnce(&T) -> bool,
        build: impl FnOnce() -> T,
        f: impl FnOnce(&mut T) -> R,
    ) -> R {
        let epoch = state_epoch();
        let mut slot = self.slot.borrow_mut();
        let keep = matches!(&*slot, Some((e, v)) if *e == epoch && valid(v));
        if !keep {
            *slot = Some((epoch, build()));
        }
        f(&mut slot.as_mut().unwrap().1)
    }

    /// Take the value out (e.g. to call something that may re-enter the
    /// cache); put it back with [`put`](Self::put). `None` if absent or stale.
    pub fn take(&self) -> Option<T> {
        let epoch = state_epoch();
        match self.slot.borrow_mut().take() {
            Some((e, v)) if e == epoch => Some(v),
            _ => None,
        }
    }

    pub fn put(&self, value: T) {
        *self.slot.borrow_mut() = Some((state_epoch(), value));
    }

    /// Drop the cached value.
    pub fn invalidate(&self) {
        self.slot.borrow_mut().take();
    }
}

impl<T> Default for InstanceCache<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Declare instance caches:
///
/// ```ignore
/// instance_cache! {
///     static DOCS: HashMap<u64, ParsedDoc>;
///     pub static FONTS: FontSet;
/// }
/// ```
#[macro_export]
macro_rules! instance_cache {
    ($($(#[$meta:meta])* $vis:vis static $name:ident : $ty:ty;)+) => {
        ::std::thread_local! {
            $($(#[$meta])* $vis static $name: $crate::cache::InstanceCache<$ty> =
                const { $crate::cache::InstanceCache::new() };)+
        }
    };
}
