//! Unix-like Clocksource Framework
//!
//! Provides a generic, rating-based timer abstraction so the rest of the
//! kernel never talks to a concrete timer (HPET, TSC, PIT, ...) directly.
//!
//! A hardware driver implements [`ClockSource`] and calls
//! [`register_source`] once it is running. The framework keeps the
//! highest-rated source as the current one and exposes monotonic
//! `elapsed_*` / `sleep_*` helpers used by syscalls, IPC, ACPI and RTC.
//!
//! ```ignore
//! struct HpetClockSource;
//! impl ClockSource for HpetClockSource { /* ... */ }
//! static HPET_SOURCE: HpetClockSource = HpetClockSource;
//! clock::register_source(&HPET_SOURCE)?;
//! let ns = clock::elapsed_ns();
//! ```

use crate::sync::Mutex;
use core::fmt;

/// Nanoseconds per microsecond.
pub const NSEC_PER_USEC: u64 = 1_000;
/// Nanoseconds per millisecond.
pub const NSEC_PER_MSEC: u64 = 1_000_000;
/// Nanoseconds per second.
pub const NSEC_PER_SEC: u64 = 1_000_000_000;
/// Microseconds per second.
pub const USEC_PER_SEC: u64 = 1_000_000;

/// Clocksource ratings (higher wins), modelled after Linux clocksource ratings.
pub const RATING_PIT: u32 = 110;
pub const RATING_HPET: u32 = 250;
pub const RATING_TSC: u32 = 300;
pub const RATING_ARCH_TIMER: u32 = 300;

/// Maximum number of clocksources that can be registered.
pub const MAX_CLOCKSOURCES: usize = 8;

static REGISTRY: Mutex<ClockRegistry> = Mutex::new(ClockRegistry::new());

/// Explicit error type for clocksource registration/selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockError {
    /// No clocksource is currently registered.
    NoSource,
    /// A source with the same name is already registered.
    AlreadyRegistered,
    /// The registration table is full.
    TableFull,
}

impl fmt::Display for ClockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSource => write!(f, "no clocksource registered"),
            Self::AlreadyRegistered => write!(f, "clocksource already registered"),
            Self::TableFull => write!(f, "clocksource table full"),
        }
    }
}

/// Hardware-agnostic monotonic timer.
///
/// Implementors wrap a free-running counter (HPET main counter, TSC,
/// ARM generic timer, ...) and convert ticks to nanoseconds internally
/// so callers always observe `u64` nanoseconds since boot.
pub trait ClockSource: Send + Sync {
    /// Unique short name (e.g. `"hpet"`, `"tsc"`).
    fn name(&self) -> &'static str;

    /// Selection priority; the highest-rated source becomes current.
    fn rating(&self) -> u32;

    /// Monotonic nanoseconds since boot.
    fn read_ns(&self) -> u64;

    /// Nominal resolution in nanoseconds (default 1 ns).
    fn resolution_ns(&self) -> u64 {
        1
    }

    /// Whether the counter is continuous (never stops). Defaults to true.
    fn is_continuous(&self) -> bool {
        true
    }

    /// Convenience: microseconds since boot derived from [`ClockSource::read_ns`].
    fn read_us(&self) -> u64 {
        self.read_ns() / NSEC_PER_USEC
    }

    /// Convenience: milliseconds since boot derived from [`ClockSource::read_ns`].
    fn read_ms(&self) -> u64 {
        self.read_ns() / NSEC_PER_MSEC
    }
}

/// Internal registry holding every registered source plus the current pick.
struct ClockRegistry {
    sources: [Option<&'static dyn ClockSource>; MAX_CLOCKSOURCES],
    count: usize,
    current: Option<&'static dyn ClockSource>,
}

impl ClockRegistry {
    const fn new() -> Self {
        Self {
            sources: [None; MAX_CLOCKSOURCES],
            count: 0,
            current: None,
        }
    }
}

/// Register a static clocksource and promote it if it outranks the current one.
///
/// Registration is idempotent by name: registering the same name twice
/// returns `Err(ClockError::AlreadyRegistered)`.
pub fn register_source(source: &'static dyn ClockSource) -> Result<(), ClockError> {
    let mut switched_name: Option<&'static str> = None;
    let mut switched_rating: u32 = 0;

    {
        let mut registry = REGISTRY.lock();

        for existing in registry.sources.iter().take(registry.count).flatten() {
            if existing.name() == source.name() {
                return Err(ClockError::AlreadyRegistered);
            }
        }

        if registry.count >= MAX_CLOCKSOURCES {
            return Err(ClockError::TableFull);
        }

        let idx = registry.count;
        if let Some(slot) = registry.sources.get_mut(idx) {
            *slot = Some(source);
        } else {
            return Err(ClockError::TableFull);
        }
        registry.count += 1;

        let should_switch = match registry.current {
            None => true,
            Some(current) => source.rating() > current.rating(),
        };
        if should_switch {
            registry.current = Some(source);
            switched_name = Some(source.name());
            switched_rating = source.rating();
        }
    }

    if let Some(name) = switched_name {
        log::info!(
            "[clock] clocksource '{}' selected (rating {})",
            name,
            switched_rating
        );
    } else {
        log::info!(
            "[clock] clocksource '{}' registered (rating {}, not selected)",
            source.name(),
            source.rating()
        );
    }

    Ok(())
}

/// Run `f` against the current source, or return `fallback` when none exists.
///
/// This is the single generic access point; all `elapsed_*` helpers build on it
/// so per-caller locking logic is not duplicated.
pub fn with_source<R>(f: impl FnOnce(&'static dyn ClockSource) -> R, fallback: R) -> R {
    let current = REGISTRY.lock().current;
    match current {
        Some(source) => f(source),
        None => fallback,
    }
}

/// Copy out the current source reference, if any.
pub fn current() -> Option<&'static dyn ClockSource> {
    REGISTRY.lock().current
}

/// Returns `true` once at least one clocksource is registered.
pub fn is_ready() -> bool {
    REGISTRY.lock().current.is_some()
}

/// Number of registered clocksources.
pub fn source_count() -> usize {
    REGISTRY.lock().count
}

/// Name of the current clocksource, or `"none"`.
pub fn current_name() -> &'static str {
    with_source(|source| source.name(), "none")
}

/// Rating of the current clocksource, or 0 when none exists.
pub fn current_rating() -> u32 {
    with_source(|source| source.rating(), 0)
}

/// Resolution of the current clocksource in nanoseconds, or 0 when none exists.
pub fn resolution_ns() -> u64 {
    with_source(|source| source.resolution_ns(), 0)
}

/// Monotonic nanoseconds since boot (0 when no source is registered).
#[inline]
pub fn elapsed_ns() -> u64 {
    with_source(|source| source.read_ns(), 0)
}

/// Alias for [`elapsed_ns`] using POSIX `CLOCK_MONOTONIC` vocabulary.
#[inline]
pub fn monotonic_ns() -> u64 {
    elapsed_ns()
}

/// Monotonic microseconds since boot.
#[inline]
pub fn elapsed_us() -> u64 {
    with_source(|source| source.read_us(), 0)
}

/// Monotonic milliseconds since boot.
#[inline]
pub fn elapsed_ms() -> u64 {
    with_source(|source| source.read_ms(), 0)
}

/// Whole seconds since boot.
#[inline]
pub fn uptime_secs() -> u64 {
    with_source(|source| source.read_ns() / NSEC_PER_SEC, 0)
}

/// Busy-wait for `ns` nanoseconds using the current clocksource.
pub fn sleep_ns(ns: u64) {
    let Some(source) = current() else {
        return;
    };
    if ns == 0 {
        return;
    }
    let start = source.read_ns();
    while source.read_ns().wrapping_sub(start) < ns {
        core::hint::spin_loop();
    }
}

/// Busy-wait for `us` microseconds using the current clocksource.
pub fn sleep_us(us: u64) {
    sleep_ns(us.saturating_mul(NSEC_PER_USEC));
}

/// Busy-wait for `ms` milliseconds using the current clocksource.
pub fn sleep_ms(ms: u64) {
    sleep_ns(ms.saturating_mul(NSEC_PER_MSEC));
}

/// Unix-style microsecond delay alias.
pub fn udelay(us: u64) {
    sleep_us(us);
}

/// Unix-style millisecond delay alias.
pub fn mdelay(ms: u64) {
    sleep_ms(ms);
}

/// Unix-style nanosecond delay alias.
pub fn ndelay(ns: u64) {
    sleep_ns(ns);
}
