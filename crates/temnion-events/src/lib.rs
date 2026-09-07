// SPDX-License-Identifier: AGPL-3.0-only
//! Bounded, source-local, append-only event history.
//!
//! This foundation is **volatile**: append success does not mean durable
//! persistence. Payloads can be typed deltas; no whole-state or JSON encoding is
//! imposed. Values are never exposed mutably after append.

use std::error::Error;
use std::fmt;

use temnion_core::{
    ClockId, EntityId, EventId, EventTimes, SourceEpoch, SourceId, TimeAxis, TimeRange, Timestamp,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventInput<T> {
    pub entity: EntityId,
    pub times: EventTimes,
    pub change: T,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event<T> {
    pub id: EventId,
    pub entity: EntityId,
    pub times: EventTimes,
    pub change: T,
}

/// Acknowledges in-memory admission only, never a WAL/fsync guarantee.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VolatileReceipt {
    pub first: EventId,
    pub last: EventId,
    pub count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventError {
    AllocationFailed,
    EmptyBatch,
    CapacityExceeded { available: usize, requested: usize },
    KnownClockMismatch { expected: ClockId, actual: ClockId },
    KnownTimeRegression { previous: u64, actual: u64 },
    InvalidBudget,
    InvalidCursor,
    UnknownEvent(EventId),
}

impl fmt::Display for EventError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AllocationFailed => write!(f, "could not reserve event storage"),
            Self::EmptyBatch => write!(f, "an event batch must not be empty"),
            Self::CapacityExceeded {
                available,
                requested,
            } => write!(
                f,
                "event capacity exceeded: {requested} requested, {available} available"
            ),
            Self::KnownClockMismatch { expected, actual } => {
                write!(f, "known-time clock {actual:?} differs from {expected:?}")
            }
            Self::KnownTimeRegression { previous, actual } => {
                write!(f, "known time regressed from {previous} to {actual}")
            }
            Self::InvalidBudget => write!(f, "result and scan budgets must both be positive"),
            Self::InvalidCursor => {
                write!(f, "cursor does not match this source, snapshot, or query")
            }
            Self::UnknownEvent(id) => write!(f, "unknown event {id:?}"),
        }
    }
}

impl Error for EventError {}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HistoryFilter {
    pub entity: Option<EntityId>,
    pub time: Option<(TimeAxis, TimeRange)>,
    /// Inclusive cutoff in the log's known-time clock.
    pub known_as_of: Option<Timestamp>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueryBudget {
    pub max_results: usize,
    pub max_scanned: usize,
}

/// An in-process cursor pinned to the original append-only prefix and filter.
///
/// This is not a serialized or authenticated TNP cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistoryCursor {
    source: SourceId,
    epoch: SourceEpoch,
    snapshot_len: usize,
    next_offset: usize,
    filter: HistoryFilter,
}

#[derive(Debug)]
pub struct HistoryPage<'a, T> {
    pub events: Vec<&'a Event<T>>,
    pub scanned: usize,
    pub continuation: Option<HistoryCursor>,
}

#[derive(Debug)]
pub struct EventLog<T> {
    source: SourceId,
    epoch: SourceEpoch,
    capacity: usize,
    events: Vec<Event<T>>,
}

impl<T> EventLog<T> {
    pub fn new(source: SourceId, epoch: SourceEpoch, capacity: usize) -> Result<Self, EventError> {
        let mut events = Vec::new();
        events
            .try_reserve_exact(capacity)
            .map_err(|_| EventError::AllocationFailed)?;
        Ok(Self {
            source,
            epoch,
            capacity,
            events,
        })
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn remaining_capacity(&self) -> usize {
        self.capacity - self.events.len()
    }

    fn id_at(&self, index: usize) -> EventId {
        EventId {
            source: self.source,
            epoch: self.epoch,
            sequence: index as u64,
        }
    }

    fn check_capacity(&self, count: usize) -> Result<(), EventError> {
        if count > self.remaining_capacity() {
            return Err(EventError::CapacityExceeded {
                available: self.remaining_capacity(),
                requested: count,
            });
        }
        Ok(())
    }

    fn check_known_time(previous: Option<Timestamp>, actual: Timestamp) -> Result<(), EventError> {
        if let Some(previous) = previous {
            if previous.clock != actual.clock {
                return Err(EventError::KnownClockMismatch {
                    expected: previous.clock,
                    actual: actual.clock,
                });
            }
            if actual.ticks < previous.ticks {
                return Err(EventError::KnownTimeRegression {
                    previous: previous.ticks,
                    actual: actual.ticks,
                });
            }
        }
        Ok(())
    }

    fn push_validated(&mut self, input: EventInput<T>) -> EventId {
        let id = self.id_at(self.events.len());
        self.events.push(Event {
            id,
            entity: input.entity,
            times: input.times,
            change: input.change,
        });
        id
    }

    /// Appends without allocating per event after the initial reservation.
    pub fn append(&mut self, input: EventInput<T>) -> Result<VolatileReceipt, EventError> {
        self.check_capacity(1)?;
        Self::check_known_time(self.events.last().map(|e| e.times.known), input.times.known)?;
        let id = self.push_validated(input);
        Ok(VolatileReceipt {
            first: id,
            last: id,
            count: 1,
        })
    }

    /// All-or-nothing admission: every capacity/time check precedes mutation.
    ///
    /// Valid/observation times may arrive out of order. Known time must be
    /// nondecreasing within the source and epoch; equal ticks use sequence order.
    pub fn append_batch(
        &mut self,
        batch: Vec<EventInput<T>>,
    ) -> Result<VolatileReceipt, EventError> {
        if batch.is_empty() {
            return Err(EventError::EmptyBatch);
        }
        self.check_capacity(batch.len())?;
        let mut previous = self.events.last().map(|e| e.times.known);
        for input in &batch {
            Self::check_known_time(previous, input.times.known)?;
            previous = Some(input.times.known);
        }
        let first = self.id_at(self.events.len());
        let count = batch.len();
        for input in batch {
            self.push_validated(input);
        }
        Ok(VolatileReceipt {
            first,
            last: self.id_at(self.events.len() - 1),
            count,
        })
    }

    pub fn get(&self, id: EventId) -> Result<&Event<T>, EventError> {
        if id.source != self.source || id.epoch != self.epoch {
            return Err(EventError::UnknownEvent(id));
        }
        let index = usize::try_from(id.sequence).map_err(|_| EventError::UnknownEvent(id))?;
        self.events.get(index).ok_or(EventError::UnknownEvent(id))
    }

    /// Queries in source sequence order, with bounded scanning as well as output.
    ///
    /// New appends between pages are excluded from the cursor's snapshot.
    /// An empty page can have a continuation if its scan budget was exhausted.
    pub fn history(
        &self,
        filter: HistoryFilter,
        budget: QueryBudget,
        cursor: Option<HistoryCursor>,
    ) -> Result<HistoryPage<'_, T>, EventError> {
        if budget.max_results == 0 || budget.max_scanned == 0 {
            return Err(EventError::InvalidBudget);
        }
        if let (Some(cutoff), Some(first)) = (filter.known_as_of, self.events.first()) {
            if cutoff.clock != first.times.known.clock {
                return Err(EventError::KnownClockMismatch {
                    expected: first.times.known.clock,
                    actual: cutoff.clock,
                });
            }
        }
        let (snapshot_len, mut offset) = match cursor {
            Some(cursor) => {
                if cursor.source != self.source
                    || cursor.epoch != self.epoch
                    || cursor.filter != filter
                    || cursor.snapshot_len > self.events.len()
                    || cursor.next_offset > cursor.snapshot_len
                {
                    return Err(EventError::InvalidCursor);
                }
                (cursor.snapshot_len, cursor.next_offset)
            }
            None => (self.events.len(), 0),
        };
        let mut events = Vec::new();
        events
            .try_reserve_exact(
                budget
                    .max_results
                    .min(budget.max_scanned)
                    .min(snapshot_len - offset),
            )
            .map_err(|_| EventError::AllocationFailed)?;
        let mut scanned = 0;
        while offset < snapshot_len
            && scanned < budget.max_scanned
            && events.len() < budget.max_results
        {
            let event = &self.events[offset];
            offset += 1;
            scanned += 1;
            if filter.entity.is_some_and(|entity| entity != event.entity) {
                continue;
            }
            if let Some(cutoff) = filter.known_as_of {
                if event.times.known.ticks > cutoff.ticks {
                    continue;
                }
            }
            if let Some((axis, range)) = filter.time {
                if !event
                    .times
                    .on(axis)
                    .is_some_and(|time| range.contains(time))
                {
                    continue;
                }
            }
            events.push(event);
        }
        let continuation = (offset < snapshot_len).then_some(HistoryCursor {
            source: self.source,
            epoch: self.epoch,
            snapshot_len,
            next_offset: offset,
            filter,
        });
        Ok(HistoryPage {
            events,
            scanned,
            continuation,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use temnion_core::ShardId;

    fn input(slot: u32, valid: u64, known: u64) -> EventInput<u64> {
        EventInput {
            entity: EntityId {
                shard: ShardId(0),
                slot,
                generation: 0,
            },
            times: EventTimes {
                valid: Timestamp::new(ClockId(1), valid),
                observed: None,
                known: Timestamp::new(ClockId(2), known),
            },
            change: valid,
        }
    }

    fn log(capacity: usize) -> EventLog<u64> {
        EventLog::new(SourceId(1), SourceEpoch(3), capacity).unwrap()
    }

    const BUDGET: QueryBudget = QueryBudget {
        max_results: 100,
        max_scanned: 100,
    };

    #[test]
    fn batch_receipts_have_contiguous_source_local_ids() {
        let mut log = log(3);
        let receipt = log
            .append_batch(vec![input(1, 5, 1), input(1, 2, 1), input(2, 8, 2)])
            .unwrap();
        assert_eq!(
            receipt.first,
            EventId {
                source: SourceId(1),
                epoch: SourceEpoch(3),
                sequence: 0
            }
        );
        assert_eq!(receipt.last.sequence, 2);
        assert_eq!(receipt.count, 3);
        assert_eq!(log.get(receipt.last).unwrap().change, 8);
        let foreign = EventId {
            epoch: SourceEpoch(4),
            ..receipt.last
        };
        assert_eq!(log.get(foreign), Err(EventError::UnknownEvent(foreign)));
        let missing = EventId {
            sequence: u64::MAX,
            ..receipt.last
        };
        assert_eq!(log.get(missing), Err(EventError::UnknownEvent(missing)));
    }

    #[test]
    fn batch_validation_is_atomic_and_does_not_consume_sequences() {
        let mut log = log(4);
        log.append(input(1, 10, 2)).unwrap();
        assert_eq!(
            log.append_batch(vec![input(1, 11, 3), input(1, 12, 1)]),
            Err(EventError::KnownTimeRegression {
                previous: 3,
                actual: 1
            })
        );
        assert_eq!(log.len(), 1);
        let mut wrong_clock = input(1, 12, 4);
        wrong_clock.times.known.clock = ClockId(8);
        assert!(matches!(
            log.append_batch(vec![input(1, 11, 3), wrong_clock]),
            Err(EventError::KnownClockMismatch { .. })
        ));
        assert_eq!(log.len(), 1);
        assert_eq!(log.append(input(1, 9, 3)).unwrap().first.sequence, 1);
    }

    #[test]
    fn capacity_and_empty_batches_fail_explicitly() {
        let mut log = log(1);
        assert_eq!(log.append_batch(vec![]), Err(EventError::EmptyBatch));
        assert_eq!(
            log.append_batch(vec![input(1, 1, 1), input(1, 2, 2)]),
            Err(EventError::CapacityExceeded {
                available: 1,
                requested: 2
            })
        );
        assert!(log.is_empty());
        log.append(input(1, 1, 1)).unwrap();
        assert_eq!(
            log.append(input(1, 2, 2)),
            Err(EventError::CapacityExceeded {
                available: 0,
                requested: 1
            })
        );
        assert_eq!(log.remaining_capacity(), 0);
    }

    #[test]
    fn historical_knowledge_excludes_late_arriving_evidence() {
        let mut log = log(3);
        log.append_batch(vec![input(1, 5, 10), input(1, 3, 20), input(2, 6, 25)])
            .unwrap();
        let filter = HistoryFilter {
            entity: Some(input(1, 0, 0).entity),
            time: Some((TimeAxis::Valid, TimeRange::new(ClockId(1), 0, 10).unwrap())),
            known_as_of: Some(Timestamp::new(ClockId(2), 10)),
        };
        let page = log.history(filter, BUDGET, None).unwrap();
        assert_eq!(
            page.events.iter().map(|e| e.change).collect::<Vec<_>>(),
            vec![5]
        );
        assert_eq!(page.scanned, 3);
        assert!(page.continuation.is_none());
        let invalid = HistoryFilter {
            known_as_of: Some(Timestamp::new(ClockId(9), 10)),
            ..filter
        };
        assert!(matches!(
            log.history(invalid, BUDGET, None),
            Err(EventError::KnownClockMismatch { .. })
        ));
    }

    #[test]
    fn observations_are_not_fabricated_and_ranges_are_clock_scoped() {
        let mut log = log(2);
        let mut observed = input(1, 2, 2);
        observed.times.observed = Some(Timestamp::new(ClockId(3), 50));
        log.append_batch(vec![input(1, 1, 1), observed]).unwrap();
        let filter = HistoryFilter {
            time: Some((
                TimeAxis::Observed,
                TimeRange::new(ClockId(3), 50, 51).unwrap(),
            )),
            ..HistoryFilter::default()
        };
        let page = log.history(filter, BUDGET, None).unwrap();
        assert_eq!(page.events.len(), 1);
        assert_eq!(page.events[0].change, 2);
    }

    #[test]
    fn pagination_is_stable_across_appends_and_binds_to_filter() {
        let mut log = log(5);
        log.append_batch((0..4).map(|n| input(1, n, n)).collect())
            .unwrap();
        let filter = HistoryFilter::default();
        let budget = QueryBudget {
            max_results: 2,
            max_scanned: 2,
        };
        let first = log.history(filter, budget, None).unwrap();
        assert_eq!(first.events.len(), 2);
        let cursor = first.continuation.unwrap();
        log.append(input(1, 4, 4)).unwrap();
        let second = log.history(filter, budget, Some(cursor)).unwrap();
        assert_eq!(
            second.events.iter().map(|e| e.change).collect::<Vec<_>>(),
            vec![2, 3]
        );
        assert!(
            second.continuation.is_none(),
            "new append is not in the original snapshot"
        );
        let changed = HistoryFilter {
            entity: Some(input(1, 0, 0).entity),
            ..filter
        };
        assert!(matches!(
            log.history(changed, budget, Some(cursor)),
            Err(EventError::InvalidCursor)
        ));
        let other: EventLog<u64> = EventLog::new(SourceId(2), SourceEpoch(3), 0).unwrap();
        assert!(matches!(
            other.history(filter, budget, Some(cursor)),
            Err(EventError::InvalidCursor)
        ));
    }

    #[test]
    fn sparse_scans_are_bounded_and_empty_pages_can_continue() {
        let mut log = log(4);
        log.append_batch(vec![input(1, 0, 0), input(1, 1, 1), input(2, 2, 2)])
            .unwrap();
        let filter = HistoryFilter {
            entity: Some(input(2, 0, 0).entity),
            ..HistoryFilter::default()
        };
        let budget = QueryBudget {
            max_results: 1,
            max_scanned: 2,
        };
        let first = log.history(filter, budget, None).unwrap();
        assert!(first.events.is_empty());
        assert_eq!(first.scanned, 2);
        let second = log.history(filter, budget, first.continuation).unwrap();
        assert_eq!(second.events[0].change, 2);
        assert!(second.continuation.is_none());
        assert!(matches!(
            log.history(
                filter,
                QueryBudget {
                    max_results: 0,
                    max_scanned: 1
                },
                None
            ),
            Err(EventError::InvalidBudget)
        ));
    }

    #[test]
    fn paged_query_matches_reference_for_many_late_events() {
        let mut log = log(1000);
        let source: Vec<_> = (0..1000u64)
            .map(|n| input((n % 7) as u32, (n * 37) % 100, n))
            .collect();
        let expected: Vec<_> = source
            .iter()
            .filter(|e| {
                e.entity.slot == 3
                    && (20..70).contains(&e.times.valid.ticks)
                    && e.times.known.ticks <= 800
            })
            .map(|e| e.change)
            .collect();
        log.append_batch(source).unwrap();
        let filter = HistoryFilter {
            entity: Some(input(3, 0, 0).entity),
            time: Some((TimeAxis::Valid, TimeRange::new(ClockId(1), 20, 70).unwrap())),
            known_as_of: Some(Timestamp::new(ClockId(2), 800)),
        };
        let mut cursor = None;
        let mut actual = Vec::new();
        loop {
            let page = log
                .history(
                    filter,
                    QueryBudget {
                        max_results: 3,
                        max_scanned: 11,
                    },
                    cursor,
                )
                .unwrap();
            assert!(page.scanned <= 11);
            assert!(page.events.len() <= 3);
            actual.extend(page.events.iter().map(|e| e.change));
            cursor = page.continuation;
            if cursor.is_none() {
                break;
            }
        }
        assert_eq!(actual, expected);
    }
}
