// SPDX-License-Identifier: AGPL-3.0-only
//! Dense live values with generation-safe, shard-local handles.
//!
//! Values are contiguous without an `Option<T>` or free-list field per value.
//! Stable handles use a separate slot directory. Deletion swaps the last dense
//! value into the hole, so iteration order is not stable.

use std::error::Error;
use std::fmt;

use temnion_core::{EntityId, ShardId};

const VACANT: u32 = u32::MAX;

#[derive(Clone, Copy, Debug)]
struct Slot {
    dense_index: u32,
    generation: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateError {
    InvalidCapacity,
    AllocationFailed,
    CapacityExceeded,
    InvalidEntity(EntityId),
}

impl fmt::Display for StateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCapacity => write!(f, "state capacity exceeds the u32 slot address space"),
            Self::AllocationFailed => write!(f, "could not reserve state storage"),
            Self::CapacityExceeded => write!(f, "state slot capacity exhausted"),
            Self::InvalidEntity(id) => write!(f, "stale, missing, or wrong-shard entity {id:?}"),
        }
    }
}

impl Error for StateError {}

/// One owner's live state. Mutations require exclusive access.
#[derive(Debug)]
pub struct StateSlab<T> {
    shard: ShardId,
    capacity: usize,
    slots: Vec<Slot>,
    values: Vec<T>,
    dense_slots: Vec<u32>,
    free_slots: Vec<u32>,
}

impl<T> StateSlab<T> {
    /// Preallocates metadata and value storage; a zero-capacity slab is valid.
    pub fn new(shard: ShardId, capacity: usize) -> Result<Self, StateError> {
        if capacity > u32::MAX as usize {
            return Err(StateError::InvalidCapacity);
        }
        let mut slab = Self {
            shard,
            capacity,
            slots: Vec::new(),
            values: Vec::new(),
            dense_slots: Vec::new(),
            free_slots: Vec::new(),
        };
        slab.slots
            .try_reserve_exact(capacity)
            .map_err(|_| StateError::AllocationFailed)?;
        slab.values
            .try_reserve_exact(capacity)
            .map_err(|_| StateError::AllocationFailed)?;
        slab.dense_slots
            .try_reserve_exact(capacity)
            .map_err(|_| StateError::AllocationFailed)?;
        slab.free_slots
            .try_reserve_exact(capacity)
            .map_err(|_| StateError::AllocationFailed)?;
        Ok(slab)
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Maximum slot count, including slots retired after generation exhaustion.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn shard(&self) -> ShardId {
        self.shard
    }

    pub fn insert(&mut self, value: T) -> Result<EntityId, StateError> {
        let slot_index = if let Some(index) = self.free_slots.pop() {
            index
        } else {
            if self.slots.len() == self.capacity {
                return Err(StateError::CapacityExceeded);
            }
            let index = self.slots.len() as u32;
            self.slots.push(Slot {
                dense_index: VACANT,
                generation: 0,
            });
            index
        };
        let slot = &mut self.slots[slot_index as usize];
        slot.dense_index = self.values.len() as u32;
        self.values.push(value);
        self.dense_slots.push(slot_index);
        Ok(EntityId {
            shard: self.shard,
            slot: slot_index,
            generation: slot.generation,
        })
    }

    fn dense_index(&self, id: EntityId) -> Result<usize, StateError> {
        let slot = self.slots.get(id.slot as usize);
        match slot {
            Some(slot)
                if id.shard == self.shard
                    && id.generation == slot.generation
                    && slot.dense_index != VACANT =>
            {
                Ok(slot.dense_index as usize)
            }
            _ => Err(StateError::InvalidEntity(id)),
        }
    }

    pub fn get(&self, id: EntityId) -> Result<&T, StateError> {
        Ok(&self.values[self.dense_index(id)?])
    }

    pub fn get_mut(&mut self, id: EntityId) -> Result<&mut T, StateError> {
        let index = self.dense_index(id)?;
        Ok(&mut self.values[index])
    }

    pub fn remove(&mut self, id: EntityId) -> Result<T, StateError> {
        let dense_index = self.dense_index(id)?;
        let value = self.values.swap_remove(dense_index);
        self.dense_slots.swap_remove(dense_index);
        if let Some(&moved_slot) = self.dense_slots.get(dense_index) {
            self.slots[moved_slot as usize].dense_index = dense_index as u32;
        }
        let slot = &mut self.slots[id.slot as usize];
        slot.dense_index = VACANT;
        // Retire exhausted generations rather than making a stale handle valid.
        if let Some(generation) = slot.generation.checked_add(1) {
            slot.generation = generation;
            self.free_slots.push(id.slot);
        }
        Ok(value)
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = (EntityId, &T)> + '_ {
        self.dense_slots
            .iter()
            .copied()
            .zip(&self.values)
            .map(|(index, value)| {
                let slot = &self.slots[index as usize];
                (
                    EntityId {
                        shard: self.shard,
                        slot: index,
                        generation: slot.generation,
                    },
                    value,
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn dense_removal_preserves_other_handles_and_reuse_invalidates_old_handles() {
        let mut slab = StateSlab::new(ShardId(7), 3).unwrap();
        let a = slab.insert(10).unwrap();
        let b = slab.insert(20).unwrap();
        let c = slab.insert(30).unwrap();
        assert_eq!(slab.remove(b), Ok(20));
        assert_eq!(slab.get(c), Ok(&30));
        assert_eq!(slab.get(a), Ok(&10));
        let d = slab.insert(40).unwrap();
        assert_eq!(d.slot, b.slot);
        assert_ne!(d.generation, b.generation);
        assert_eq!(slab.get(b), Err(StateError::InvalidEntity(b)));
        assert_eq!(slab.remove(b), Err(StateError::InvalidEntity(b)));
        assert_eq!(slab.get(d), Ok(&40));
        assert_eq!(slab.len(), 3);
    }

    #[test]
    fn wrong_shard_and_unknown_slots_fail_without_mutating_state() {
        let mut slab = StateSlab::new(ShardId(1), 1).unwrap();
        let id = slab.insert(8).unwrap();
        let other = EntityId {
            shard: ShardId(2),
            ..id
        };
        let missing = EntityId {
            slot: u32::MAX,
            ..id
        };
        for invalid in [other, missing] {
            assert_eq!(slab.get(invalid), Err(StateError::InvalidEntity(invalid)));
            assert_eq!(
                slab.get_mut(invalid),
                Err(StateError::InvalidEntity(invalid))
            );
            assert_eq!(
                slab.remove(invalid),
                Err(StateError::InvalidEntity(invalid))
            );
        }
        assert_eq!(slab.get(id), Ok(&8));
    }

    #[test]
    fn limits_and_mutation_are_explicit() {
        let mut empty = StateSlab::new(ShardId(0), 0).unwrap();
        assert!(empty.is_empty());
        assert_eq!(empty.insert(1), Err(StateError::CapacityExceeded));
        let mut slab = StateSlab::new(ShardId(0), 1).unwrap();
        let id = slab.insert(1).unwrap();
        assert_eq!(slab.insert(2), Err(StateError::CapacityExceeded));
        *slab.get_mut(id).unwrap() = 9;
        assert_eq!(slab.iter().collect::<Vec<_>>(), vec![(id, &9)]);
    }

    #[test]
    fn exhausted_generations_are_retired() {
        let mut slab = StateSlab::new(ShardId(0), 1).unwrap();
        let initial = slab.insert(5).unwrap();
        slab.slots[initial.slot as usize].generation = u32::MAX;
        let last = EntityId {
            generation: u32::MAX,
            ..initial
        };
        assert_eq!(slab.remove(last), Ok(5));
        assert_eq!(slab.insert(6), Err(StateError::CapacityExceeded));
        assert_eq!(slab.get(last), Err(StateError::InvalidEntity(last)));
        assert_eq!(slab.get(initial), Err(StateError::InvalidEntity(initial)));
    }

    #[test]
    fn operation_sequence_matches_a_reference_map() {
        let mut slab = StateSlab::new(ShardId(3), 64).unwrap();
        let mut reference = BTreeMap::new();
        let mut rng = 42u64;
        for step in 0..10_000 {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
            if reference.is_empty() || (rng & 1 == 0 && reference.len() < 64) {
                let id = slab.insert(step).unwrap();
                reference.insert(id, step);
            } else {
                let index = (rng as usize) % reference.len();
                let id = *reference.keys().nth(index).unwrap();
                if rng & 2 == 0 {
                    assert_eq!(slab.remove(id).unwrap(), reference.remove(&id).unwrap());
                    assert_eq!(slab.get(id), Err(StateError::InvalidEntity(id)));
                } else {
                    *slab.get_mut(id).unwrap() = step;
                    *reference.get_mut(&id).unwrap() = step;
                }
            }
            assert_eq!(
                slab.iter()
                    .map(|(id, value)| (id, *value))
                    .collect::<BTreeMap<_, _>>(),
                reference
            );
        }
    }
}
