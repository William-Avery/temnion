// SPDX-License-Identifier: AGPL-3.0-only
//! Model-agnostic identities and explicit clock domains.
//!
//! These Rust types are not a disk format, C ABI, or wire protocol. TSF and TNP
//! will have independently versioned, explicitly encoded representations.

use std::error::Error;
use std::fmt;

/// Stable logical shard identity, independent of its executing thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ShardId(pub u32);

/// A producer identity within one database.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceId(pub u32);

/// A producer incarnation. Callers must not reuse it when restarting a sequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceEpoch(pub u64);

/// Identifies one clock's origin and tick unit within a database.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClockId(pub u32);

/// Generation-safe entity identity. Slot reuse never preserves the generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntityId {
    pub shard: ShardId,
    pub slot: u32,
    pub generation: u32,
}

/// Source-local event order; lexicographic ID order is not global causal order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventId {
    pub source: SourceId,
    pub epoch: SourceEpoch,
    pub sequence: u64,
}

/// A tick value is only meaningful within its declared clock domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Timestamp {
    pub clock: ClockId,
    pub ticks: u64,
}

impl Timestamp {
    pub const fn new(clock: ClockId, ticks: u64) -> Self {
        Self { clock, ticks }
    }
}

/// Independent temporal meanings, not three aliases of a wall-clock timestamp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventTimes {
    pub valid: Timestamp,
    pub observed: Option<Timestamp>,
    pub known: Timestamp,
}

/// Which temporal meaning a history query filters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeAxis {
    Valid,
    Observed,
    Known,
}

impl EventTimes {
    pub fn on(self, axis: TimeAxis) -> Option<Timestamp> {
        match axis {
            TimeAxis::Valid => Some(self.valid),
            TimeAxis::Observed => self.observed,
            TimeAxis::Known => Some(self.known),
        }
    }
}

/// An inclusive-start, exclusive-end interval within exactly one clock domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeRange {
    clock: ClockId,
    start: u64,
    end: u64,
}

impl TimeRange {
    pub fn new(clock: ClockId, start: u64, end: u64) -> Result<Self, InvalidTimeRange> {
        if start > end {
            return Err(InvalidTimeRange { start, end });
        }
        Ok(Self { clock, start, end })
    }

    pub const fn clock(self) -> ClockId {
        self.clock
    }

    pub const fn start(self) -> u64 {
        self.start
    }

    pub const fn end(self) -> u64 {
        self.end
    }

    /// A timestamp on another clock is outside this explicitly scoped range.
    pub fn contains(self, timestamp: Timestamp) -> bool {
        timestamp.clock == self.clock && timestamp.ticks >= self.start && timestamp.ticks < self.end
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidTimeRange {
    pub start: u64,
    pub end: u64,
}

impl fmt::Display for InvalidTimeRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "time range start {} exceeds end {}",
            self.start, self.end
        )
    }
}

impl Error for InvalidTimeRange {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_are_half_open_and_clock_scoped() {
        let range = TimeRange::new(ClockId(1), 10, 20).unwrap();
        assert!(!range.contains(Timestamp::new(ClockId(1), 9)));
        assert!(range.contains(Timestamp::new(ClockId(1), 10)));
        assert!(range.contains(Timestamp::new(ClockId(1), 19)));
        assert!(!range.contains(Timestamp::new(ClockId(1), 20)));
        assert!(!range.contains(Timestamp::new(ClockId(2), 15)));
    }

    #[test]
    fn empty_ranges_are_valid_but_reversed_ranges_are_not() {
        let empty = TimeRange::new(ClockId(0), 4, 4).unwrap();
        assert!(!empty.contains(Timestamp::new(ClockId(0), 4)));
        assert_eq!(
            TimeRange::new(ClockId(0), 5, 4),
            Err(InvalidTimeRange { start: 5, end: 4 })
        );
    }

    #[test]
    fn times_preserve_missing_observations_and_independent_domains() {
        let times = EventTimes {
            valid: Timestamp::new(ClockId(1), 50),
            observed: None,
            known: Timestamp::new(ClockId(2), 7),
        };
        assert_eq!(times.on(TimeAxis::Valid), Some(times.valid));
        assert_eq!(times.on(TimeAxis::Observed), None);
        assert_eq!(times.on(TimeAxis::Known), Some(times.known));
    }
}
