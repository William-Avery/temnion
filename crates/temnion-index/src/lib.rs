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

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use crc32fast::Hasher;
use temnion_core::{
    ClockId, DatabaseId, EntityId, EventTimes, SchemaId, ShardId, SourceEpoch, SourceId, TimeAxis,
    TimeRange, Timestamp,
};
use temnion_events::HistoryFilter;

pub const SUMMARY_MAGIC: [u8; 4] = *b"TNSM";
pub const SUMMARY_VERSION: u16 = 1;

pub const PROJECTION_MAGIC: [u8; 4] = *b"TNPR";
pub const PROJECTION_VERSION: u16 = 1;

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
            Self::ChecksumMismatch => write!(f, "index checksum mismatch"),
            Self::InvalidMagic => write!(f, "invalid index magic; expected TNSM or TNPR"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported index version: {v}"),
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

// ============================================================================
// Space-Filling Curves & N-Dimensional Chunking (M6)
// ============================================================================

/// Dilates the bits of a 32-bit integer into even bit positions of a 64-bit integer.
pub const fn morton_spread_2d(v: u32) -> u64 {
    let mut x = v as u64;
    x = (x | (x << 16)) & 0x0000_ffff_0000_ffff;
    x = (x | (x << 8)) & 0x00ff_00ff_00ff_00ff;
    x = (x | (x << 4)) & 0x0f0f_0f0f_0f0f_0f0f;
    x = (x | (x << 2)) & 0x3333_3333_3333_3333;
    x = (x | (x << 1)) & 0x5555_5555_5555_5555;
    x
}

/// Compacts the even bits of a 64-bit integer back into a 32-bit integer.
pub const fn morton_compact_2d(mut x: u64) -> u32 {
    x &= 0x5555_5555_5555_5555;
    x = (x | (x >> 1)) & 0x3333_3333_3333_3333;
    x = (x | (x >> 2)) & 0x0f0f_0f0f_0f0f_0f0f;
    x = (x | (x >> 4)) & 0x00ff_00ff_00ff_00ff;
    x = (x | (x >> 8)) & 0x0000_ffff_0000_ffff;
    x = (x | (x >> 16)) & 0x0000_0000_ffff_ffff;
    x as u32
}

/// Encodes 2D coordinates `(x, y)` into a 64-bit Morton code (Z-order curve).
pub const fn morton_encode_2d(x: u32, y: u32) -> u64 {
    morton_spread_2d(x) | (morton_spread_2d(y) << 1)
}

/// Decodes a 64-bit Morton code into 2D coordinates `(x, y)`.
pub const fn morton_decode_2d(code: u64) -> (u32, u32) {
    (morton_compact_2d(code), morton_compact_2d(code >> 1))
}

/// Dilates the lower 21 bits of a 32-bit integer into every 3rd bit of a 64-bit integer.
pub fn morton_spread_3d(v: u32) -> u64 {
    let mut code = 0u64;
    for i in 0..21 {
        code |= (((v as u64) >> i) & 1) << (3 * i);
    }
    code
}

/// Compacts every 3rd bit of a 64-bit integer into the lower 21 bits of a 32-bit integer.
pub fn morton_compact_3d(code: u64) -> u32 {
    let mut v = 0u32;
    for i in 0..21 {
        v |= (((code >> (3 * i)) & 1) as u32) << i;
    }
    v
}

/// Encodes 3D coordinates `(x, y, z)` (each up to 21 bits) into a 64-bit Morton code.
pub fn morton_encode_3d(x: u32, y: u32, z: u32) -> u64 {
    morton_spread_3d(x) | (morton_spread_3d(y) << 1) | (morton_spread_3d(z) << 2)
}

/// Decodes a 64-bit Morton code into 3D coordinates `(x, y, z)`.
pub fn morton_decode_3d(code: u64) -> (u32, u32, u32) {
    (
        morton_compact_3d(code),
        morton_compact_3d(code >> 1),
        morton_compact_3d(code >> 2),
    )
}

/// 2D axis-aligned bounding box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundingBox2D {
    pub min_x: u32,
    pub min_y: u32,
    pub max_x: u32,
    pub max_y: u32,
}

impl BoundingBox2D {
    pub const fn new(min_x: u32, min_y: u32, max_x: u32, max_y: u32) -> Self {
        Self {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }

    pub const fn contains_point(&self, x: u32, y: u32) -> bool {
        x >= self.min_x && x <= self.max_x && y >= self.min_y && y <= self.max_y
    }

    pub const fn intersects(&self, other: &Self) -> bool {
        self.min_x <= other.max_x
            && self.max_x >= other.min_x
            && self.min_y <= other.max_y
            && self.max_y >= other.min_y
    }

    /// Decomposes the bounding box into a set of contiguous Morton code intervals in chunk space.
    pub fn morton_intervals_chunked(&self, chunker: &GridChunker2D) -> Vec<(u64, u64)> {
        let (min_cx, min_cy) = chunker.chunk_coords(self.min_x, self.min_y);
        let (max_cx, max_cy) = chunker.chunk_coords(self.max_x, self.max_y);

        let mut codes = Vec::new();
        for cy in min_cy..=max_cy {
            for cx in min_cx..=max_cx {
                codes.push(morton_encode_2d(cx, cy));
            }
        }
        codes.sort_unstable();
        codes.dedup();

        merge_consecutive_codes(&codes)
    }
}

/// 3D axis-aligned bounding box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundingBox3D {
    pub min_x: u32,
    pub min_y: u32,
    pub min_z: u32,
    pub max_x: u32,
    pub max_y: u32,
    pub max_z: u32,
}

impl BoundingBox3D {
    pub const fn new(
        min_x: u32,
        min_y: u32,
        min_z: u32,
        max_x: u32,
        max_y: u32,
        max_z: u32,
    ) -> Self {
        Self {
            min_x,
            min_y,
            min_z,
            max_x,
            max_y,
            max_z,
        }
    }

    pub const fn contains_point(&self, x: u32, y: u32, z: u32) -> bool {
        x >= self.min_x
            && x <= self.max_x
            && y >= self.min_y
            && y <= self.max_y
            && z >= self.min_z
            && z <= self.max_z
    }

    pub const fn intersects(&self, other: &Self) -> bool {
        self.min_x <= other.max_x
            && self.max_x >= other.min_x
            && self.min_y <= other.max_y
            && self.max_y >= other.min_y
            && self.min_z <= other.max_z
            && self.max_z >= other.min_z
    }

    /// Decomposes the 3D bounding box into a set of contiguous Morton code intervals in chunk space.
    pub fn morton_intervals_chunked(&self, chunker: &GridChunker3D) -> Vec<(u64, u64)> {
        let (min_cx, min_cy, min_cz) = chunker.chunk_coords(self.min_x, self.min_y, self.min_z);
        let (max_cx, max_cy, max_cz) = chunker.chunk_coords(self.max_x, self.max_y, self.max_z);

        let mut codes = Vec::new();
        for cz in min_cz..=max_cz {
            for cy in min_cy..=max_cy {
                for cx in min_cx..=max_cx {
                    codes.push(morton_encode_3d(cx, cy, cz));
                }
            }
        }
        codes.sort_unstable();
        codes.dedup();

        merge_consecutive_codes(&codes)
    }
}

/// Merges sorted unique codes into contiguous `(start, end)` inclusive intervals.
fn merge_consecutive_codes(codes: &[u64]) -> Vec<(u64, u64)> {
    let mut intervals = Vec::new();
    if codes.is_empty() {
        return intervals;
    }

    let mut start = codes[0];
    let mut end = codes[0];

    for &code in &codes[1..] {
        if code == end + 1 {
            end = code;
        } else {
            intervals.push((start, end));
            start = code;
            end = code;
        }
    }
    intervals.push((start, end));
    intervals
}

/// Uniform 2D spatial grid chunker dividing continuous coordinate space into discrete tiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridChunker2D {
    pub chunk_size: u32,
}

impl GridChunker2D {
    pub fn new(chunk_size: u32) -> Result<Self, IndexError> {
        if chunk_size == 0 {
            return Err(IndexError::Format("chunk size must be positive"));
        }
        Ok(Self { chunk_size })
    }

    pub const fn chunk_coords(&self, x: u32, y: u32) -> (u32, u32) {
        (x / self.chunk_size, y / self.chunk_size)
    }

    pub const fn chunk_morton(&self, x: u32, y: u32) -> u64 {
        let (cx, cy) = self.chunk_coords(x, y);
        morton_encode_2d(cx, cy)
    }
}

/// Uniform 3D spatial grid chunker dividing continuous coordinate space into discrete voxels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridChunker3D {
    pub chunk_size: u32,
}

impl GridChunker3D {
    pub fn new(chunk_size: u32) -> Result<Self, IndexError> {
        if chunk_size == 0 {
            return Err(IndexError::Format("chunk size must be positive"));
        }
        Ok(Self { chunk_size })
    }

    pub const fn chunk_coords(&self, x: u32, y: u32, z: u32) -> (u32, u32, u32) {
        (
            x / self.chunk_size,
            y / self.chunk_size,
            z / self.chunk_size,
        )
    }

    pub fn chunk_morton(&self, x: u32, y: u32, z: u32) -> u64 {
        let (cx, cy, cz) = self.chunk_coords(x, y, z);
        morton_encode_3d(cx, cy, cz)
    }
}

// ============================================================================
// Alternate Projections (M7)
// ============================================================================

/// Inverted entity projection mapping EntityId to sorted sequence numbers without payload duplication.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EntityProjection {
    pub entries: BTreeMap<EntityId, Vec<u64>>,
}

impl EntityProjection {
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, entity: EntityId, sequence: u64) {
        let list = self.entries.entry(entity).or_default();
        if list.last().copied() != Some(sequence) {
            list.push(sequence);
        }
    }

    pub fn sequences_for_entity(&self, entity: EntityId) -> &[u64] {
        self.entries
            .get(&entity)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
}

/// Temporal projection mapping (ClockId, tick) to sorted sequence numbers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TemporalProjection {
    pub entries: BTreeMap<(ClockId, u64), Vec<u64>>,
}

impl TemporalProjection {
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, clock: ClockId, ticks: u64, sequence: u64) {
        let list = self.entries.entry((clock, ticks)).or_default();
        if list.last().copied() != Some(sequence) {
            list.push(sequence);
        }
    }

    pub fn sequences_in_range(&self, clock: ClockId, min_ticks: u64, max_ticks: u64) -> Vec<u64> {
        let mut results = Vec::new();
        for ((c, _ticks), seqs) in self.entries.range((clock, min_ticks)..=(clock, max_ticks)) {
            if *c == clock {
                results.extend_from_slice(seqs);
            }
        }
        results.sort_unstable();
        results.dedup();
        results
    }
}

/// Spatial Morton projection mapping Morton code to sorted sequence numbers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpatialMortonProjection {
    pub entries: BTreeMap<u64, Vec<u64>>,
}

impl SpatialMortonProjection {
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, morton_code: u64, sequence: u64) {
        let list = self.entries.entry(morton_code).or_default();
        if list.last().copied() != Some(sequence) {
            list.push(sequence);
        }
    }

    pub fn sequences_in_intervals(&self, intervals: &[(u64, u64)]) -> Vec<u64> {
        let mut results = Vec::new();
        for &(start, end) in intervals {
            for (_, seqs) in self.entries.range(start..=end) {
                results.extend_from_slice(seqs);
            }
        }
        results.sort_unstable();
        results.dedup();
        results
    }

    pub fn sequences_in_box(&self, bbox: &BoundingBox2D, chunker: &GridChunker2D) -> Vec<u64> {
        let intervals = bbox.morton_intervals_chunked(chunker);
        self.sequences_in_intervals(&intervals)
    }
}

/// Schema/event-type bitmap projection mapping SchemaId to sorted sequence numbers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SchemaBitmapProjection {
    pub entries: BTreeMap<SchemaId, Vec<u64>>,
}

impl SchemaBitmapProjection {
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, schema: SchemaId, sequence: u64) {
        let list = self.entries.entry(schema).or_default();
        if list.last().copied() != Some(sequence) {
            list.push(sequence);
        }
    }

    pub fn sequences_for_schema(&self, schema: SchemaId) -> &[u64] {
        self.entries
            .get(&schema)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
}

/// Helper function to intersect two sorted slices of sequence numbers.
fn intersect_sorted(a: &[u64], b: &[u64]) -> Vec<u64> {
    let mut out = Vec::new();
    let mut i = 0;
    let mut j = 0;
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                out.push(a[i]);
                i += 1;
                j += 1;
            }
        }
    }
    out
}

/// Composite projection index aggregating entity, temporal, spatial, and schema projections.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProjectionIndex {
    pub entities: EntityProjection,
    pub temporal: TemporalProjection,
    pub spatial: SpatialMortonProjection,
    pub schemas: SchemaBitmapProjection,
    pub total_indexed: u64,
}

impl ProjectionIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn index_event(
        &mut self,
        sequence: u64,
        entity: EntityId,
        schema: SchemaId,
        valid_time: Timestamp,
        spatial_coords: Option<(u32, u32)>,
        chunker: Option<&GridChunker2D>,
    ) {
        self.entities.insert(entity, sequence);
        self.schemas.insert(schema, sequence);
        self.temporal
            .insert(valid_time.clock, valid_time.ticks, sequence);

        if let (Some((x, y)), Some(c)) = (spatial_coords, chunker) {
            let code = c.chunk_morton(x, y);
            self.spatial.insert(code, sequence);
        }
        self.total_indexed += 1;
    }

    /// Evaluates a multi-predicate intersection across projections, returning matching sequences.
    pub fn query_intersect(
        &self,
        entity: Option<EntityId>,
        schema: Option<SchemaId>,
        time_range: Option<(ClockId, u64, u64)>,
        spatial_intervals: Option<&[(u64, u64)]>,
    ) -> Vec<u64> {
        let mut candidates: Option<Vec<u64>> = None;

        if let Some(e) = entity {
            candidates = Some(self.entities.sequences_for_entity(e).to_vec());
            if candidates.as_ref().unwrap().is_empty() {
                return Vec::new();
            }
        }

        if let Some(s) = schema {
            let seqs = self.schemas.sequences_for_schema(s);
            match &mut candidates {
                None => candidates = Some(seqs.to_vec()),
                Some(existing) => {
                    *existing = intersect_sorted(existing, seqs);
                }
            }
            if candidates.as_ref().unwrap().is_empty() {
                return Vec::new();
            }
        }

        if let Some((clock, min_t, max_t)) = time_range {
            let seqs = self.temporal.sequences_in_range(clock, min_t, max_t);
            match &mut candidates {
                None => candidates = Some(seqs),
                Some(existing) => {
                    *existing = intersect_sorted(existing, &seqs);
                }
            }
            if candidates.as_ref().unwrap().is_empty() {
                return Vec::new();
            }
        }

        if let Some(intervals) = spatial_intervals {
            let seqs = self.spatial.sequences_in_intervals(intervals);
            match &mut candidates {
                None => candidates = Some(seqs),
                Some(existing) => {
                    *existing = intersect_sorted(existing, &seqs);
                }
            }
        }

        candidates.unwrap_or_default()
    }

    /// Serializes the projection index into a binary byte vector (`TNPR` format).
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();

        // total_indexed
        body.extend_from_slice(&self.total_indexed.to_le_bytes());

        // Entities
        body.extend_from_slice(&(self.entities.entries.len() as u32).to_le_bytes());
        for (entity, seqs) in &self.entities.entries {
            body.extend_from_slice(&entity.shard.0.to_le_bytes());
            body.extend_from_slice(&entity.slot.to_le_bytes());
            body.extend_from_slice(&entity.generation.to_le_bytes());
            body.extend_from_slice(&(seqs.len() as u32).to_le_bytes());
            for &seq in seqs {
                body.extend_from_slice(&seq.to_le_bytes());
            }
        }

        // Temporal
        body.extend_from_slice(&(self.temporal.entries.len() as u32).to_le_bytes());
        for ((clock, ticks), seqs) in &self.temporal.entries {
            body.extend_from_slice(&clock.0.to_le_bytes());
            body.extend_from_slice(&ticks.to_le_bytes());
            body.extend_from_slice(&(seqs.len() as u32).to_le_bytes());
            for &seq in seqs {
                body.extend_from_slice(&seq.to_le_bytes());
            }
        }

        // Spatial
        body.extend_from_slice(&(self.spatial.entries.len() as u32).to_le_bytes());
        for (&code, seqs) in &self.spatial.entries {
            body.extend_from_slice(&code.to_le_bytes());
            body.extend_from_slice(&(seqs.len() as u32).to_le_bytes());
            for &seq in seqs {
                body.extend_from_slice(&seq.to_le_bytes());
            }
        }

        // Schemas
        body.extend_from_slice(&(self.schemas.entries.len() as u32).to_le_bytes());
        for (&schema, seqs) in &self.schemas.entries {
            body.extend_from_slice(&schema.0.to_le_bytes());
            body.extend_from_slice(&(seqs.len() as u32).to_le_bytes());
            for &seq in seqs {
                body.extend_from_slice(&seq.to_le_bytes());
            }
        }

        let mut hasher = Hasher::new();
        hasher.update(&body);
        let checksum = hasher.finalize();

        let mut output = Vec::with_capacity(12 + body.len());
        output.extend_from_slice(&PROJECTION_MAGIC);
        output.extend_from_slice(&PROJECTION_VERSION.to_le_bytes());
        output.extend_from_slice(&0u16.to_le_bytes()); // flags
        output.extend_from_slice(&checksum.to_le_bytes());
        output.extend_from_slice(&body);
        output
    }

    /// Deserializes a projection index from binary bytes with CRC32C integrity validation.
    pub fn decode(bytes: &[u8]) -> Result<Self, IndexError> {
        if bytes.len() < 12 {
            return Err(IndexError::Format("projection index truncated"));
        }
        if bytes[0..4] != PROJECTION_MAGIC {
            return Err(IndexError::InvalidMagic);
        }
        let version = u16::from_le_bytes(bytes[4..6].try_into().unwrap());
        if version != PROJECTION_VERSION {
            return Err(IndexError::UnsupportedVersion(version));
        }
        let expected_checksum = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
        let body = &bytes[12..];

        let mut hasher = Hasher::new();
        hasher.update(body);
        if hasher.finalize() != expected_checksum {
            return Err(IndexError::ChecksumMismatch);
        }

        let mut offset = 0;
        let read_u32 = |offset: &mut usize| -> Result<u32, IndexError> {
            if *offset + 4 > body.len() {
                return Err(IndexError::Format("unexpected EOF reading u32"));
            }
            let v = u32::from_le_bytes(body[*offset..*offset + 4].try_into().unwrap());
            *offset += 4;
            Ok(v)
        };
        let read_u64 = |offset: &mut usize| -> Result<u64, IndexError> {
            if *offset + 8 > body.len() {
                return Err(IndexError::Format("unexpected EOF reading u64"));
            }
            let v = u64::from_le_bytes(body[*offset..*offset + 8].try_into().unwrap());
            *offset += 8;
            Ok(v)
        };

        let total_indexed = read_u64(&mut offset)?;

        // Entities
        let entity_count = read_u32(&mut offset)? as usize;
        let mut entities = EntityProjection::new();
        for _ in 0..entity_count {
            let shard = ShardId(read_u32(&mut offset)?);
            let slot = read_u32(&mut offset)?;
            let generation = read_u32(&mut offset)?;
            let seq_count = read_u32(&mut offset)? as usize;
            let mut seqs = Vec::with_capacity(seq_count);
            for _ in 0..seq_count {
                seqs.push(read_u64(&mut offset)?);
            }
            entities.entries.insert(
                EntityId {
                    shard,
                    slot,
                    generation,
                },
                seqs,
            );
        }

        // Temporal
        let temp_count = read_u32(&mut offset)? as usize;
        let mut temporal = TemporalProjection::new();
        for _ in 0..temp_count {
            let clock = ClockId(read_u32(&mut offset)?);
            let ticks = read_u64(&mut offset)?;
            let seq_count = read_u32(&mut offset)? as usize;
            let mut seqs = Vec::with_capacity(seq_count);
            for _ in 0..seq_count {
                seqs.push(read_u64(&mut offset)?);
            }
            temporal.entries.insert((clock, ticks), seqs);
        }

        // Spatial
        let spat_count = read_u32(&mut offset)? as usize;
        let mut spatial = SpatialMortonProjection::new();
        for _ in 0..spat_count {
            let code = read_u64(&mut offset)?;
            let seq_count = read_u32(&mut offset)? as usize;
            let mut seqs = Vec::with_capacity(seq_count);
            for _ in 0..seq_count {
                seqs.push(read_u64(&mut offset)?);
            }
            spatial.entries.insert(code, seqs);
        }

        // Schemas
        let schema_count = read_u32(&mut offset)? as usize;
        let mut schemas = SchemaBitmapProjection::new();
        for _ in 0..schema_count {
            let schema = SchemaId(read_u32(&mut offset)?);
            let seq_count = read_u32(&mut offset)? as usize;
            let mut seqs = Vec::with_capacity(seq_count);
            for _ in 0..seq_count {
                seqs.push(read_u64(&mut offset)?);
            }
            schemas.entries.insert(schema, seqs);
        }

        Ok(Self {
            entities,
            temporal,
            spatial,
            schemas,
            total_indexed,
        })
    }
}
