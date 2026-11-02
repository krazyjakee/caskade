//! Hybrid logical clocks (#228): wall time in milliseconds plus a counter.
//!
//! A device's clock never goes backwards: each new reading is at least one
//! past the highest it has made or seen, whatever the system clock says, and
//! the highest is kept on disk (the version vector, `sync_seen`), so a
//! restart or a clock set back carries on from there. Seeing another
//! device's clock moves this one past it, so an edit made after receiving
//! another device's change always wins over it, even when this device's
//! system clock is behind.
//!
//! Changes are ordered by [`Stamp`]: the clock, then the device id, so two
//! changes never tie and every device picks the same winner.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

/// A hybrid logical clock reading.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct Hlc {
    /// Milliseconds since the Unix epoch, or past it when ticks outran the
    /// wall clock.
    pub wall: i64,
    pub counter: i64,
}

impl Hlc {
    pub const fn new(wall: i64, counter: i64) -> Self {
        Self { wall, counter }
    }
}

/// A change's position in the total order every device agrees on.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Stamp {
    pub clock: Hlc,
    /// The device that made the change (its endpoint id).
    pub origin: String,
}

impl Ord for Stamp {
    fn cmp(&self, other: &Self) -> Ordering {
        self.clock
            .cmp(&other.clock)
            .then_with(|| self.origin.cmp(&other.origin))
    }
}

impl PartialOrd for Stamp {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// This device's clock.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Clock {
    last: Hlc,
}

impl Clock {
    /// A clock that carries on from `last`, the highest reading made or seen.
    pub fn resume(last: Hlc) -> Self {
        Self { last }
    }

    pub fn last(&self) -> Hlc {
        self.last
    }

    /// A new reading at wall time `now_ms`, after every earlier one.
    pub fn tick(&mut self, now_ms: i64) -> Hlc {
        self.last = if now_ms > self.last.wall {
            Hlc::new(now_ms, 0)
        } else {
            Hlc::new(self.last.wall, self.last.counter + 1)
        };
        self.last
    }

    /// Takes in another device's reading: later ticks come after it.
    pub fn observe(&mut self, seen: Hlc) {
        if seen > self.last {
            self.last = seen;
        }
    }
}

/// The wall clock, in milliseconds since the Unix epoch.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_follow_the_wall_clock_and_never_repeat() {
        let mut clock = Clock::default();
        let a = clock.tick(1000);
        let b = clock.tick(1000);
        let c = clock.tick(1005);
        assert_eq!(a, Hlc::new(1000, 0));
        assert_eq!(b, Hlc::new(1000, 1));
        assert_eq!(c, Hlc::new(1005, 0));
        assert!(a < b && b < c);
    }

    #[test]
    fn a_clock_set_back_keeps_counting_forward() {
        let mut clock = Clock::default();
        let before = clock.tick(10_000);
        // The system clock jumps an hour back, and the app restarts.
        let mut restarted = Clock::resume(clock.last());
        let after = restarted.tick(10_000 - 3_600_000);
        assert!(after > before, "{after:?} after {before:?}");
        assert_eq!(after, Hlc::new(10_000, 1));
    }

    #[test]
    fn an_edit_after_seeing_another_device_wins_even_with_a_slow_clock() {
        // The other device's clock is a day ahead.
        let theirs = Hlc::new(86_400_000 + 5_000, 3);
        let mut mine = Clock::default();
        mine.observe(theirs);
        let edit = mine.tick(5_000);
        assert!(edit > theirs);
    }

    #[test]
    fn stamps_order_by_clock_then_device() {
        let a = Stamp {
            clock: Hlc::new(5, 0),
            origin: "b".into(),
        };
        let b = Stamp {
            clock: Hlc::new(5, 0),
            origin: "c".into(),
        };
        let c = Stamp {
            clock: Hlc::new(5, 1),
            origin: "a".into(),
        };
        assert!(a < b && b < c);
    }
}
