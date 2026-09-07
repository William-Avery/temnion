// SPDX-License-Identifier: AGPL-3.0-only
//! Hierarchical summaries, zone maps, and predicate pushdown filters for Temnion.
//!
//! Following Temnion Architecture §11 and §12:
//! - Implements multi-level hierarchical summaries:
//!   - Level 0: Block-level zone maps and Bloom filters.
//!   - Level 1: Segment-level aggregate summaries.
//! - Predicate pushdown evaluation (`SkipDecision`):
//!   - Conservative skipping: A block is skipped if and only if it is mathematically
//!     guaranteed to contain zero matching records (zero false negatives).
//!   - Entity Bloom filters: probabilistic set membership with bounded false positive rate.
//!   - Temporal zone maps: fast interval exclusion over valid and known time domains.
//!   - Known-as-of cutoffs: eliminates scanning batches written after the cutoff.
//! - Binary format: `TNSM` magic, version 1, CRC32C checksum.

use std::error::Error;
use std::fmt;

use temnion_core::{
    ClockId, DatabaseId, EntityId, EventTimes, SchemaId, ShardId, SourceEpoch, SourceId, TimeAxis,
    TimeRange, Timestamp,
};
use temnion_events::HistoryFilter;

pub const SUMMARY_MAGIC: [u8; 4] = *b"TNSM";
pub const SUMMARY_VERSION: u16 = 1;

#[derive(Debug, PartialEq, Eq)]
pub enum IndexError {
    Io(&'static str),
    Format(&'static str),
    ChecksumMismatch,
    InvalidMagic,
    UnsupportedVersion(u16),
}

impl fmt::Display for IndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(msg) => write!(f, "index I/O error: {msg}"),
            Self::Format(msg) => write!(f, "index format error: {msg}"),
            Self::ChecksumMismatch => write!(f, "summary checksum mismatch"),
            Self::InvalidMagic => write!(f, "invalid summary magic; expected TNSM"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported summary version: {v}"),
        }
    }
}

impl Error for IndexError {}

/// A conservative bounding interval tracking minimum and maximum scalar values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZoneMap<T> {
    pub min: T,
    pub max: T,
}

impl<T: Copy + Ord> ZoneMap<T> {
    pub const fn new(val: T) -> Self {
        Self { min: val, max: val }
    }

    pub fn update(&mut self, val: T) {
        if val < self.min {
            self.min = val;
        }
        if val > self.max {
            self.max = val;
        }
    }

    pub fn contains(&self, val: &T) -> bool {
        val >= &self.min && val <= &self.max
    }

    pub fn overlaps(&self, other: &Self) -> bool {
        self.min <= other.max && self.max >= other.min
    }

    pub fn overlaps_bounds(&self, start: Option<T>, end: Option<T>) -> bool {
        if let Some(s) = start {
            if self.max < s {
                return false;
            }
        }
        if let Some(e) = end {
            if self.min > e {
                return false;
            }
        }
        true
    }
}

/// A conservative bounding interval tracking minimum and maximum ticks within a clock domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimestampZoneMap {
    pub clock: ClockId,
    pub min_ticks: u64,
    pub max_ticks: u64,
}

impl TimestampZoneMap {
    pub const fn new(ts: Timestamp) -> Self {
        Self {
            clock: ts.clock,
            min_ticks: ts.ticks,
            max_ticks: ts.ticks,
        }
    }

    pub fn update(&mut self, ts: Timestamp) {
        if ts.clock == self.clock {
            if ts.ticks < self.min_ticks {
                self.min_ticks = ts.ticks;
            }
            if ts.ticks > self.max_ticks {
                self.max_ticks = ts.ticks;
            }
        }
    }

    pub fn overlaps_range(&self, range: &TimeRange) -> bool {
        if self.clock != range.clock() {
            // Cannot prune based on a different clock domain
            return true;
        }
        self.min_ticks < range.end() && self.max_ticks >= range.start()
    }

    pub fn can_contain_known(&self, cutoff: Timestamp) -> bool {
        if self.clock != cutoff.clock {
            return true;
        }
        self.min_ticks <= cutoff.ticks
    }
}

/// 512-bit (64-byte) Bloom filter for EntityId membership testing.
/// Provides zero false negatives and low false positive rate (~1-2% for 32-64 items).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntityBloomFilter {
    pub bits: [u8; 64],
}

impl Default for EntityBloomFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl EntityBloomFilter {
    pub const fn new() -> Self {
        Self { bits: [0u8; 64] }
    }

    pub fn is_empty(&self) -> bool {
        self.bits.iter().all(|&b| b == 0)
    }

    fn hash_entity(entity: EntityId, seed: u64) -> usize {
        let mut h = seed.wrapping_add(0x9e3779b97f4a7c15);
        h ^= ((entity.shard.0 as u64) << 32) | (entity.slot as u64);
        h = h.wrapping_mul(0xbf58476d1ce4e5b9);
        h ^= h >> 30;
        h ^= (entity.generation as u64).wrapping_mul(0x94d049bb133111eb);
        h = h.wrapping_mul(0xbf58476d1ce4e5b9);
        h ^= h >> 32;
        (h % 512) as usize
    }

    pub fn insert(&mut self, entity: EntityId) {
        const SEEDS: [u64; 4] = [
            0x517cc1b727220a95,
            0x6e3a39e7b401e62a,
            0x9b6b79f324e93081,
            0xc6a4a7935bd1e995,
        ];
        for &seed in &SEEDS {
            let bit_idx = Self::hash_entity(entity, seed);
            self.bits[bit_idx / 8] |= 1 << (bit_idx % 8);
        }
    }

    pub fn may_contain(&self, entity: EntityId) -> bool {
        const SEEDS: [u64; 4] = [
            0x517cc1b727220a95,
            0x6e3a39e7b401e62a,
            0x9b6b79f324e93081,
            0xc6a4a7935bd1e995,
        ];
        for &seed in &SEEDS {
            let bit_idx = Self::hash_entity(entity, seed);
            if (self.bits[bit_idx / 8] & (1 << (bit_idx % 8))) == 0 {
                return false;
            }
        }
        true
    }
}

/// Decision whether to read/decompress a block or prune it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipDecision {
    /// Definitely zero matching records in this block. Prune without reading.
    Skip,
    /// Block may contain matching records. Must scan.
    MustScan,
}

/// Metadata summary for a contiguous block or batch of records.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockSummary {
    pub batch_index: u32,
    pub record_count: u32,
    pub byte_offset: u64,
    pub byte_length: u32,
    pub sequence_range: ZoneMap<u64>,
    pub valid_time_range: TimestampZoneMap,
    pub known_time_range: TimestampZoneMap,
    pub entity_range: ZoneMap<EntityId>,
    pub entity_bloom: EntityBloomFilter,
    pub schema_mask: u64,
}

impl BlockSummary {
    /// Creates an initial summary from the first record of a block.
    pub fn new(
        batch_index: u32,
        byte_offset: u64,
        byte_length: u32,
        seq: u64,
        entity: EntityId,
        schema: SchemaId,
        times: &EventTimes,
    ) -> Self {
        let mut bloom = EntityBloomFilter::new();
        bloom.insert(entity);
        let schema_mask = if schema.0 < 64 { 1u64 << schema.0 } else { 0 };

        Self {
            batch_index,
            record_count: 1,
            byte_offset,
            byte_length,
            sequence_range: ZoneMap::new(seq),
            valid_time_range: TimestampZoneMap::new(times.valid),
            known_time_range: TimestampZoneMap::new(times.known),
            entity_range: ZoneMap::new(entity),
            entity_bloom: bloom,
            schema_mask,
        }
    }

    /// Extends this summary with an additional record in the same block.
    pub fn update(&mut self, seq: u64, entity: EntityId, schema: SchemaId, times: &EventTimes) {
        self.record_count += 1;
        self.sequence_range.update(seq);
        self.valid_time_range.update(times.valid);
        self.known_time_range.update(times.known);
        self.entity_range.update(entity);
        self.entity_bloom.insert(entity);
        if schema.0 < 64 {
            self.schema_mask |= 1u64 << schema.0;
        }
    }

    /// Evaluates a query filter against this block's summary.
    /// Returns `SkipDecision::Skip` only if zero matching records are guaranteed.
    pub fn matches_filter(&self, filter: &HistoryFilter) -> SkipDecision {
        // 1. Entity filter
        if let Some(entity) = filter.entity {
            if !self.entity_range.contains(&entity) {
                return SkipDecision::Skip;
            }
            if !self.entity_bloom.may_contain(entity) {
                return SkipDecision::Skip;
            }
        }

        // 2. Known-as-of cutoff
        if let Some(cutoff) = filter.known_as_of {
            if !self.known_time_range.can_contain_known(cutoff) {
                return SkipDecision::Skip;
            }
        }

        // 3. Time axis filter
        if let Some((axis, range)) = filter.time {
            match axis {
                TimeAxis::Valid => {
                    if !self.valid_time_range.overlaps_range(&range) {
                        return SkipDecision::Skip;
                    }
                }
                TimeAxis::Known => {
                    if !self.known_time_range.overlaps_range(&range) {
                        return SkipDecision::Skip;
                    }
                }
                TimeAxis::Observed => {
                    // Observed times may be missing/optional; cannot conservatively skip
                }
            }
        }

        SkipDecision::MustScan
    }

    /// Evaluates sequence range overlap.
    pub fn matches_sequence_range(&self, start: u64, end: u64) -> SkipDecision {
        if self.sequence_range.min > end || self.sequence_range.max < start {
            SkipDecision::Skip
        } else {
            SkipDecision::MustScan
        }
    }
}

/// Aggregate summary for an entire segment (Level 1 in the hierarchy).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SegmentSummary {
    pub database: DatabaseId,
    pub source: SourceId,
    pub epoch: SourceEpoch,
    pub sequence_range: ZoneMap<u64>,
    pub valid_time_range: TimestampZoneMap,
    pub known_time_range: TimestampZoneMap,
    pub entity_range: ZoneMap<EntityId>,
    pub total_records: u64,
    pub blocks: Vec<BlockSummary>,
}

impl SegmentSummary {
    pub fn new(
        database: DatabaseId,
        source: SourceId,
        epoch: SourceEpoch,
        blocks: Vec<BlockSummary>,
    ) -> Result<Self, IndexError> {
        if blocks.is_empty() {
            return Err(IndexError::Format(
                "cannot create segment summary from empty blocks",
            ));
        }

        let first = &blocks[0];
        let mut seq_range = first.sequence_range;
        let mut valid_range = first.valid_time_range;
        let mut known_range = first.known_time_range;
        let mut entity_range = first.entity_range;
        let mut total_records = 0u64;

        for block in &blocks {
            seq_range.update(block.sequence_range.min);
            seq_range.update(block.sequence_range.max);
            valid_range.update(Timestamp::new(
                block.valid_time_range.clock,
                block.valid_time_range.min_ticks,
            ));
            valid_range.update(Timestamp::new(
                block.valid_time_range.clock,
                block.valid_time_range.max_ticks,
            ));
            known_range.update(Timestamp::new(
                block.known_time_range.clock,
                block.known_time_range.min_ticks,
            ));
            known_range.update(Timestamp::new(
                block.known_time_range.clock,
                block.known_time_range.max_ticks,
            ));
            entity_range.update(block.entity_range.min);
            entity_range.update(block.entity_range.max);
            total_records += block.record_count as u64;
        }

        Ok(Self {
            database,
            source,
            epoch,
            sequence_range: seq_range,
            valid_time_range: valid_range,
            known_time_range: known_range,
            entity_range,
            total_records,
            blocks,
        })
    }

    /// Evaluates a query filter against the entire segment.
    /// If `SkipDecision::Skip` is returned, the entire segment can be skipped without
    /// reading block summaries.
    pub fn matches_filter(&self, filter: &HistoryFilter) -> SkipDecision {
        if let Some(entity) = filter.entity {
            if !self.entity_range.contains(&entity) {
                return SkipDecision::Skip;
            }
        }

        if let Some(cutoff) = filter.known_as_of {
            if !self.known_time_range.can_contain_known(cutoff) {
                return SkipDecision::Skip;
            }
        }

        if let Some((axis, range)) = filter.time {
            match axis {
                TimeAxis::Valid => {
                    if !self.valid_time_range.overlaps_range(&range) {
                        return SkipDecision::Skip;
                    }
                }
                TimeAxis::Known => {
                    if !self.known_time_range.overlaps_range(&range) {
                        return SkipDecision::Skip;
                    }
                }
                TimeAxis::Observed => {}
            }
        }

        SkipDecision::MustScan
    }

    /// Serializes the summary to binary with CRC32C checksum framing.
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&self.database.0);
        body.extend_from_slice(&self.source.0.to_le_bytes());
        body.extend_from_slice(&self.epoch.0.to_le_bytes());
        body.extend_from_slice(&self.total_records.to_le_bytes());
        body.extend_from_slice(&(self.blocks.len() as u32).to_le_bytes());

        for block in &self.blocks {
            body.extend_from_slice(&block.batch_index.to_le_bytes());
            body.extend_from_slice(&block.record_count.to_le_bytes());
            body.extend_from_slice(&block.byte_offset.to_le_bytes());
            body.extend_from_slice(&block.byte_length.to_le_bytes());
            body.extend_from_slice(&block.sequence_range.min.to_le_bytes());
            body.extend_from_slice(&block.sequence_range.max.to_le_bytes());

            body.extend_from_slice(&block.valid_time_range.clock.0.to_le_bytes());
            body.extend_from_slice(&block.valid_time_range.min_ticks.to_le_bytes());
            body.extend_from_slice(&block.valid_time_range.max_ticks.to_le_bytes());

            body.extend_from_slice(&block.known_time_range.clock.0.to_le_bytes());
            body.extend_from_slice(&block.known_time_range.min_ticks.to_le_bytes());
            body.extend_from_slice(&block.known_time_range.max_ticks.to_le_bytes());

            body.extend_from_slice(&block.entity_range.min.shard.0.to_le_bytes());
            body.extend_from_slice(&block.entity_range.min.slot.to_le_bytes());
            body.extend_from_slice(&block.entity_range.min.generation.to_le_bytes());

            body.extend_from_slice(&block.entity_range.max.shard.0.to_le_bytes());
            body.extend_from_slice(&block.entity_range.max.slot.to_le_bytes());
            body.extend_from_slice(&block.entity_range.max.generation.to_le_bytes());

            body.extend_from_slice(&block.schema_mask.to_le_bytes());
            body.extend_from_slice(&block.entity_bloom.bits);
        }

        let checksum = crc32fast::hash(&body);
        let mut out = Vec::with_capacity(10 + body.len());
        out.extend_from_slice(&SUMMARY_MAGIC);
        out.extend_from_slice(&SUMMARY_VERSION.to_le_bytes());
        out.extend_from_slice(&checksum.to_le_bytes());
        out.extend_from_slice(&body);
        out
    }

    /// Deserializes and checksum-verifies a binary summary.
    pub fn decode(bytes: &[u8]) -> Result<Self, IndexError> {
        if bytes.len() < 10 + 16 + 4 + 8 + 8 + 4 {
            return Err(IndexError::Format("summary payload too short for header"));
        }

        if bytes[0..4] != SUMMARY_MAGIC {
            return Err(IndexError::InvalidMagic);
        }

        let version = u16::from_le_bytes(bytes[4..6].try_into().unwrap());
        if version != SUMMARY_VERSION {
            return Err(IndexError::UnsupportedVersion(version));
        }

        let expected_checksum = u32::from_le_bytes(bytes[6..10].try_into().unwrap());
        let body = &bytes[10..];

        let actual_checksum = crc32fast::hash(body);
        if actual_checksum != expected_checksum {
            return Err(IndexError::ChecksumMismatch);
        }

        let mut offset = 0;
        let mut db_bytes = [0u8; 16];
        db_bytes.copy_from_slice(&body[offset..offset + 16]);
        let database = DatabaseId(db_bytes);
        offset += 16;

        let source = SourceId(u32::from_le_bytes(
            body[offset..offset + 4].try_into().unwrap(),
        ));
        offset += 4;

        let epoch = SourceEpoch(u64::from_le_bytes(
            body[offset..offset + 8].try_into().unwrap(),
        ));
        offset += 8;

        let total_records = u64::from_le_bytes(body[offset..offset + 8].try_into().unwrap());
        offset += 8;

        let block_count = u32::from_le_bytes(body[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;

        const BLOCK_BYTES: usize = 4 + 4 + 8 + 4 + 16 + 20 + 20 + 12 + 12 + 8 + 64; // 172 bytes
        if body.len() < offset + block_count * BLOCK_BYTES {
            return Err(IndexError::Format("truncated block summaries"));
        }

        let mut blocks = Vec::with_capacity(block_count);
        for _ in 0..block_count {
            let batch_index = u32::from_le_bytes(body[offset..offset + 4].try_into().unwrap());
            offset += 4;

            let record_count = u32::from_le_bytes(body[offset..offset + 4].try_into().unwrap());
            offset += 4;

            let byte_offset = u64::from_le_bytes(body[offset..offset + 8].try_into().unwrap());
            offset += 8;

            let byte_length = u32::from_le_bytes(body[offset..offset + 4].try_into().unwrap());
            offset += 4;

            let seq_min = u64::from_le_bytes(body[offset..offset + 8].try_into().unwrap());
            offset += 8;
            let seq_max = u64::from_le_bytes(body[offset..offset + 8].try_into().unwrap());
            offset += 8;

            let v_clock = ClockId(u32::from_le_bytes(
                body[offset..offset + 4].try_into().unwrap(),
            ));
            offset += 4;
            let v_ticks_min = u64::from_le_bytes(body[offset..offset + 8].try_into().unwrap());
            offset += 8;
            let v_ticks_max = u64::from_le_bytes(body[offset..offset + 8].try_into().unwrap());
            offset += 8;

            let k_clock = ClockId(u32::from_le_bytes(
                body[offset..offset + 4].try_into().unwrap(),
            ));
            offset += 4;
            let k_ticks_min = u64::from_le_bytes(body[offset..offset + 8].try_into().unwrap());
            offset += 8;
            let k_ticks_max = u64::from_le_bytes(body[offset..offset + 8].try_into().unwrap());
            offset += 8;

            let e_min_shard = ShardId(u32::from_le_bytes(
                body[offset..offset + 4].try_into().unwrap(),
            ));
            offset += 4;
            let e_min_slot = u32::from_le_bytes(body[offset..offset + 4].try_into().unwrap());
            offset += 4;
            let e_min_gen = u32::from_le_bytes(body[offset..offset + 4].try_into().unwrap());
            offset += 4;

            let e_max_shard = ShardId(u32::from_le_bytes(
                body[offset..offset + 4].try_into().unwrap(),
            ));
            offset += 4;
            let e_max_slot = u32::from_le_bytes(body[offset..offset + 4].try_into().unwrap());
            offset += 4;
            let e_max_gen = u32::from_le_bytes(body[offset..offset + 4].try_into().unwrap());
            offset += 4;

            let schema_mask = u64::from_le_bytes(body[offset..offset + 8].try_into().unwrap());
            offset += 8;

            let mut bloom_bits = [0u8; 64];
            bloom_bits.copy_from_slice(&body[offset..offset + 64]);
            offset += 64;

            blocks.push(BlockSummary {
                batch_index,
                record_count,
                byte_offset,
                byte_length,
                sequence_range: ZoneMap {
                    min: seq_min,
                    max: seq_max,
                },
                valid_time_range: TimestampZoneMap {
                    clock: v_clock,
                    min_ticks: v_ticks_min,
                    max_ticks: v_ticks_max,
                },
                known_time_range: TimestampZoneMap {
                    clock: k_clock,
                    min_ticks: k_ticks_min,
                    max_ticks: k_ticks_max,
                },
                entity_range: ZoneMap {
                    min: EntityId {
                        shard: e_min_shard,
                        slot: e_min_slot,
                        generation: e_min_gen,
                    },
                    max: EntityId {
                        shard: e_max_shard,
                        slot: e_max_slot,
                        generation: e_max_gen,
                    },
                },
                entity_bloom: EntityBloomFilter { bits: bloom_bits },
                schema_mask,
            });
        }

        let _ = total_records;
        Self::new(database, source, epoch, blocks)
    }
}
