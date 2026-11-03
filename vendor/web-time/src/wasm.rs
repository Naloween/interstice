use core::ops::{Add, AddAssign, Sub, SubAssign};
use std::sync::atomic::{AtomicU64, Ordering};

pub use core::time::Duration;

/// Current time in nanoseconds since the module-defined epoch.
static NOW_NANOS: AtomicU64 = AtomicU64::new(0);

pub(crate) fn set_now_impl(now: Duration) {
    let n = now.as_nanos().min(u64::MAX as u128) as u64;
    // Monotonic: never step backwards.
    NOW_NANOS.fetch_max(n, Ordering::Relaxed);
}

fn now() -> Duration {
    Duration::from_nanos(NOW_NANOS.load(Ordering::Relaxed))
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Instant(Duration);

impl Instant {
    pub fn now() -> Instant {
        Instant(now())
    }
    pub fn duration_since(&self, earlier: Instant) -> Duration {
        self.0.saturating_sub(earlier.0)
    }
    pub fn checked_duration_since(&self, earlier: Instant) -> Option<Duration> {
        self.0.checked_sub(earlier.0)
    }
    pub fn saturating_duration_since(&self, earlier: Instant) -> Duration {
        self.0.saturating_sub(earlier.0)
    }
    pub fn elapsed(&self) -> Duration {
        Instant::now().duration_since(*self)
    }
    pub fn checked_add(&self, d: Duration) -> Option<Instant> {
        self.0.checked_add(d).map(Instant)
    }
    pub fn checked_sub(&self, d: Duration) -> Option<Instant> {
        self.0.checked_sub(d).map(Instant)
    }
}

impl Add<Duration> for Instant {
    type Output = Instant;
    fn add(self, d: Duration) -> Instant {
        Instant(self.0 + d)
    }
}
impl AddAssign<Duration> for Instant {
    fn add_assign(&mut self, d: Duration) {
        self.0 += d;
    }
}
impl Sub<Duration> for Instant {
    type Output = Instant;
    fn sub(self, d: Duration) -> Instant {
        Instant(self.0.saturating_sub(d))
    }
}
impl SubAssign<Duration> for Instant {
    fn sub_assign(&mut self, d: Duration) {
        self.0 = self.0.saturating_sub(d);
    }
}
impl Sub<Instant> for Instant {
    type Output = Duration;
    fn sub(self, other: Instant) -> Duration {
        self.duration_since(other)
    }
}

/// Wall-clock time. The module clock doubles as "time since UNIX epoch" — it is
/// only used for relative timestamps by our dependencies.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SystemTime(Duration);

pub const UNIX_EPOCH: SystemTime = SystemTime(Duration::ZERO);

#[derive(Clone, Debug)]
pub struct SystemTimeError(Duration);

impl SystemTimeError {
    pub fn duration(&self) -> Duration {
        self.0
    }
}
impl core::fmt::Display for SystemTimeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "second time provided was later than self")
    }
}
impl std::error::Error for SystemTimeError {}

impl SystemTime {
    pub const UNIX_EPOCH: SystemTime = UNIX_EPOCH;
    pub fn now() -> SystemTime {
        SystemTime(now())
    }
    pub fn duration_since(&self, earlier: SystemTime) -> Result<Duration, SystemTimeError> {
        self.0.checked_sub(earlier.0).ok_or_else(|| SystemTimeError(earlier.0 - self.0))
    }
    pub fn elapsed(&self) -> Result<Duration, SystemTimeError> {
        SystemTime::now().duration_since(*self)
    }
    pub fn checked_add(&self, d: Duration) -> Option<SystemTime> {
        self.0.checked_add(d).map(SystemTime)
    }
    pub fn checked_sub(&self, d: Duration) -> Option<SystemTime> {
        self.0.checked_sub(d).map(SystemTime)
    }
}
impl Add<Duration> for SystemTime {
    type Output = SystemTime;
    fn add(self, d: Duration) -> SystemTime {
        SystemTime(self.0 + d)
    }
}
impl Sub<Duration> for SystemTime {
    type Output = SystemTime;
    fn sub(self, d: Duration) -> SystemTime {
        SystemTime(self.0.saturating_sub(d))
    }
}
