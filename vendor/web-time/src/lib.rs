//! Interstice replacement for `web-time`.
//!
//! Native targets re-export `std::time`. On `wasm32` there is no clock that
//! does not go through JavaScript, so time is a module-local counter that the
//! embedding module advances with [`set_now`] (e.g. once per frame tick).
//! Until it is first set, time stands still at zero, which is harmless for the
//! users we care about (animation/scroll timestamps, double-click detection).

#[cfg(not(target_arch = "wasm32"))]
pub use std::time::{Duration, Instant, SystemTime, SystemTimeError, UNIX_EPOCH};

#[cfg(target_arch = "wasm32")]
mod wasm;
#[cfg(target_arch = "wasm32")]
pub use wasm::*;

/// Set the current time (since an arbitrary epoch). No-op on native targets.
#[allow(unused_variables)]
pub fn set_now(now: core::time::Duration) {
    #[cfg(target_arch = "wasm32")]
    wasm::set_now_impl(now);
}
