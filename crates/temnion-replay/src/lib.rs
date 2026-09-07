// SPDX-License-Identifier: AGPL-3.0-only
//! Deterministic reconstruction, checkpoints, and replay engine for Temnion.
//!
//! Following Temnion Architecture §10:
//! - State reconstruction model: S(t+1) = F(S(t), E(t), Actions(t), RNG(t))
//! - Stores checkpoint + events + deterministic RNG state rather than full snapshots.
//! - Checkpoints are periodic, checksummed, versioned snapshots pinned to exact
//!   source WAL sequences.
//! - Deterministic replay verifies bit-for-bit equivalence between fresh playback
//!   and checkpoint-resumed reconstruction.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use temnion_core::{
    ClockId, DatabaseId, EntityId, FieldId, SchemaId, ShardId, SourceEpoch, SourceId, Timestamp,
};
use temnion_events::HistoryFilter;
use temnion_format::{Limits, StoredEvent};
use temnion_schema::{Mutation, Schema, Value, decode_mutation};
use temnion_storage::{RecoveryMode, StorageError, StorageQueryBudget, Store};

const CHECKPOINT_MAGIC: [u8; 4] = *b"TNCP";
const CHECKPOINT_VERSION: u16 = 1;
pub const CHECKPOINT_HEADER_LEN: usize = 104;

#[derive(Debug)]
pub enum ReplayError {
    Io {
        operation: &'static str,
        source: std::io::Error,
    },
    Storage(StorageError),
    Format(&'static str),
    ChecksumMismatch,
    InvalidMagic,
    UnsupportedVersion(u16),
    DiscontinuousSequence {
        expected: u64,
        actual: u64,
    },
    CorruptedCheckpoint(&'static str),
    SchemaMismatch {
        expected: SchemaId,
        actual: SchemaId,
    },
    SchemaError(temnion_schema::SchemaError),
    MissingCheckpoint(u64),
    AllocationFailed,
}

impl fmt::Display for ReplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { operation, source } => write!(f, "I/O error during {operation}: {source}"),
            Self::Storage(err) => write!(f, "storage error during replay: {err}"),
            Self::Format(msg) => write!(f, "format error: {msg}"),
            Self::ChecksumMismatch => write!(f, "checkpoint checksum mismatch"),
            Self::InvalidMagic => write!(f, "invalid checkpoint magic; expected TNCP"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported checkpoint version {v}"),
            Self::DiscontinuousSequence { expected, actual } => {
                write!(
                    f,
                    "replay sequence gap: expected {expected}, actual {actual}"
                )
            }
            Self::CorruptedCheckpoint(msg) => write!(f, "corrupted checkpoint: {msg}"),
            Self::SchemaMismatch { expected, actual } => {
                write!(
                    f,
                    "schema mismatch: expected {expected:?}, actual {actual:?}"
                )
            }
            Self::SchemaError(err) => write!(f, "schema error during replay: {err}"),
            Self::MissingCheckpoint(seq) => write!(f, "missing checkpoint at sequence {seq}"),
            Self::AllocationFailed => write!(f, "memory allocation failed during replay"),
        }
    }
}

impl Error for ReplayError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Storage(err) => Some(err),
            Self::SchemaError(err) => Some(err),
            _ => None,
        }
    }
}

impl From<StorageError> for ReplayError {
    fn from(err: StorageError) -> Self {
        Self::Storage(err)
    }
}

impl From<temnion_schema::SchemaError> for ReplayError {
    fn from(err: temnion_schema::SchemaError) -> Self {
        Self::SchemaError(err)
    }
}

fn io_err(operation: &'static str) -> impl FnOnce(std::io::Error) -> ReplayError {
    move |source| ReplayError::Io { operation, source }
}

// ---------------------------------------------------------------------------
// Deterministic RNG Capture
// ---------------------------------------------------------------------------

/// Bit-identical, platform-independent deterministic pseudo-random number generator
/// (SplitMix64) with exact step counting for decision reproduction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeterministicRng {
    seed: u64,
    state: u64,
    steps: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RngSnapshot {
    pub seed: u64,
    pub state: u64,
    pub steps: u64,
}

impl DeterministicRng {
    pub fn new(seed: u64) -> Self {
        let mut rng = Self {
            seed,
            state: seed,
            steps: 0,
        };
        // Warm up state
        let _ = rng.next_u64();
        rng.steps = 0;
        rng
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        self.steps += 1;
        z ^ (z >> 31)
    }

    /// Generates a floating point number in [0.0, 1.0) deterministically.
    pub fn next_f64(&mut self) -> f64 {
        let bits = self.next_u64() >> 11;
        bits as f64 * (1.0 / ((1u64 << 53) as f64))
    }

    pub fn snapshot(&self) -> RngSnapshot {
        RngSnapshot {
            seed: self.seed,
            state: self.state,
            steps: self.steps,
        }
    }

    pub fn restore(&mut self, snapshot: RngSnapshot) {
        self.seed = snapshot.seed;
        self.state = snapshot.state;
        self.steps = snapshot.steps;
    }
}

// ---------------------------------------------------------------------------
// Checkpoint Format and Lifecycle
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckpointHeader {
    pub database: DatabaseId,
    pub source: SourceId,
    pub epoch: SourceEpoch,
    pub sequence: u64,
    pub valid_time: Timestamp,
    pub known_time: Timestamp,
    pub rng: RngSnapshot,
    pub state_bytes_len: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Checkpoint {
    pub header: CheckpointHeader,
    pub state_payload: Vec<u8>,
}

pub fn encode_checkpoint(checkpoint: &Checkpoint) -> Result<Vec<u8>, ReplayError> {
    let mut header_buf = [0u8; CHECKPOINT_HEADER_LEN];
    header_buf[0..4].copy_from_slice(&CHECKPOINT_MAGIC);
    header_buf[4..6].copy_from_slice(&CHECKPOINT_VERSION.to_le_bytes());
    header_buf[6..8].copy_from_slice(&0u16.to_le_bytes()); // flags = 0
    header_buf[8..24].copy_from_slice(&checkpoint.header.database.0);
    header_buf[24..28].copy_from_slice(&checkpoint.header.source.0.to_le_bytes());
    header_buf[28..36].copy_from_slice(&checkpoint.header.epoch.0.to_le_bytes());
    header_buf[36..44].copy_from_slice(&checkpoint.header.sequence.to_le_bytes());
    header_buf[44..48].copy_from_slice(&checkpoint.header.valid_time.clock.0.to_le_bytes());
    header_buf[48..56].copy_from_slice(&checkpoint.header.valid_time.ticks.to_le_bytes());
    header_buf[56..60].copy_from_slice(&checkpoint.header.known_time.clock.0.to_le_bytes());
    header_buf[60..68].copy_from_slice(&checkpoint.header.known_time.ticks.to_le_bytes());

    // RNG snapshot
    header_buf[68..76].copy_from_slice(&checkpoint.header.rng.seed.to_le_bytes());
    header_buf[76..84].copy_from_slice(&checkpoint.header.rng.state.to_le_bytes());
    header_buf[84..92].copy_from_slice(&checkpoint.header.rng.steps.to_le_bytes());

    let payload_len = checkpoint.state_payload.len() as u32;
    header_buf[92..96].copy_from_slice(&payload_len.to_le_bytes());

    let body_crc = crc32fast::hash(&checkpoint.state_payload);
    header_buf[96..100].copy_from_slice(&body_crc.to_le_bytes());

    let header_crc = crc32fast::hash(&header_buf[0..100]);
    header_buf[100..104].copy_from_slice(&header_crc.to_le_bytes());

    let mut out = Vec::new();
    out.try_reserve_exact(CHECKPOINT_HEADER_LEN + checkpoint.state_payload.len())
        .map_err(|_| ReplayError::AllocationFailed)?;
    out.extend_from_slice(&header_buf);
    out.extend_from_slice(&checkpoint.state_payload);
    Ok(out)
}

pub fn decode_checkpoint(bytes: &[u8]) -> Result<Checkpoint, ReplayError> {
    if bytes.len() < CHECKPOINT_HEADER_LEN {
        return Err(ReplayError::Format("incomplete checkpoint header"));
    }
    if bytes[0..4] != CHECKPOINT_MAGIC {
        return Err(ReplayError::InvalidMagic);
    }
    let version = u16::from_le_bytes(bytes[4..6].try_into().unwrap());
    if version != CHECKPOINT_VERSION {
        return Err(ReplayError::UnsupportedVersion(version));
    }
    let header_crc = u32::from_le_bytes(bytes[100..104].try_into().unwrap());
    if crc32fast::hash(&bytes[0..100]) != header_crc {
        return Err(ReplayError::ChecksumMismatch);
    }

    let database = DatabaseId(bytes[8..24].try_into().unwrap());
    let source = SourceId(u32::from_le_bytes(bytes[24..28].try_into().unwrap()));
    let epoch = SourceEpoch(u64::from_le_bytes(bytes[28..36].try_into().unwrap()));
    let sequence = u64::from_le_bytes(bytes[36..44].try_into().unwrap());

    let valid_clock = ClockId(u32::from_le_bytes(bytes[44..48].try_into().unwrap()));
    let valid_ticks = u64::from_le_bytes(bytes[48..56].try_into().unwrap());
    let known_clock = ClockId(u32::from_le_bytes(bytes[56..60].try_into().unwrap()));
    let known_ticks = u64::from_le_bytes(bytes[60..68].try_into().unwrap());

    let rng_seed = u64::from_le_bytes(bytes[68..76].try_into().unwrap());
    let rng_state = u64::from_le_bytes(bytes[76..84].try_into().unwrap());
    let rng_steps = u64::from_le_bytes(bytes[84..92].try_into().unwrap());

    let payload_len = u32::from_le_bytes(bytes[92..96].try_into().unwrap()) as usize;
    let body_crc = u32::from_le_bytes(bytes[96..100].try_into().unwrap());

    if bytes.len() != CHECKPOINT_HEADER_LEN + payload_len {
        return Err(ReplayError::Format("checkpoint length mismatch"));
    }
    let payload = &bytes[CHECKPOINT_HEADER_LEN..];
    if crc32fast::hash(payload) != body_crc {
        return Err(ReplayError::ChecksumMismatch);
    }

    Ok(Checkpoint {
        header: CheckpointHeader {
            database,
            source,
            epoch,
            sequence,
            valid_time: Timestamp::new(valid_clock, valid_ticks),
            known_time: Timestamp::new(known_clock, known_ticks),
            rng: RngSnapshot {
                seed: rng_seed,
                state: rng_state,
                steps: rng_steps,
            },
            state_bytes_len: payload_len as u32,
        },
        state_payload: payload.to_vec(),
    })
}

// ---------------------------------------------------------------------------
// Checkpoint Storage
// ---------------------------------------------------------------------------

pub struct CheckpointStore {
    dir: PathBuf,
}

impl CheckpointStore {
    pub fn new(database_dir: impl AsRef<Path>) -> Result<Self, ReplayError> {
        let dir = database_dir.as_ref().join("checkpoints");
        fs::create_dir_all(&dir).map_err(io_err("create checkpoints directory"))?;
        Ok(Self { dir })
    }

    pub fn save(&self, checkpoint: &Checkpoint) -> Result<PathBuf, ReplayError> {
        let encoded = encode_checkpoint(checkpoint)?;
        let filename = format!("{:020}.tcp", checkpoint.header.sequence);
        let destination = self.dir.join(&filename);

        // Atomic write via unique temporary file
        let mut tmp_id = [0u8; 8];
        getrandom::getrandom(&mut tmp_id).map_err(|_e| ReplayError::Format("entropy error"))?;
        let tmp_name = format!(".tmp-{:016x}.tcp", u64::from_le_bytes(tmp_id));
        let tmp_path = self.dir.join(tmp_name);

        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&tmp_path)
            .map_err(io_err("open temporary checkpoint"))?;
        file.write_all(&encoded)
            .map_err(io_err("write temporary checkpoint"))?;
        file.sync_all()
            .map_err(io_err("synchronize temporary checkpoint"))?;
        drop(file);

        fs::rename(&tmp_path, &destination).map_err(io_err("publish checkpoint"))?;
        Ok(destination)
    }

    pub fn load(&self, sequence: u64) -> Result<Checkpoint, ReplayError> {
        let path = self.dir.join(format!("{:020}.tcp", sequence));
        let mut file = File::open(&path).map_err(io_err("open checkpoint file"))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(io_err("read checkpoint file"))?;
        decode_checkpoint(&bytes)
    }

    /// Finds the highest checkpoint with sequence <= max_sequence.
    pub fn find_latest(&self, max_sequence: u64) -> Result<Option<Checkpoint>, ReplayError> {
        let mut highest = None;
        for entry in fs::read_dir(&self.dir).map_err(io_err("scan checkpoints"))? {
            let entry = entry.map_err(io_err("read checkpoint directory entry"))?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.ends_with(".tcp") && !name_str.starts_with(".tmp-") {
                if let Ok(seq) = name_str.trim_end_matches(".tcp").parse::<u64>() {
                    if seq <= max_sequence {
                        match highest {
                            None => highest = Some(seq),
                            Some(current) if seq > current => highest = Some(seq),
                            _ => {}
                        }
                    }
                }
            }
        }
        match highest {
            Some(seq) => Ok(Some(self.load(seq)?)),
            None => Ok(None),
        }
    }
}

// ---------------------------------------------------------------------------
// State Reducer and Replay Engine
// ---------------------------------------------------------------------------

/// Trait defining a deterministic state transition function:
/// S(t+1) = F(S(t), Event(t), RNG(t))
pub trait StateReducer<S> {
    fn apply(
        &mut self,
        state: &mut S,
        event: &StoredEvent,
        rng: &mut DeterministicRng,
    ) -> Result<(), ReplayError>;
}

/// In-memory entity catalog mapping EntityId -> field values.
pub type EntityMap = BTreeMap<EntityId, (SchemaId, BTreeMap<FieldId, Value>)>;

/// Serializes EntityMap into deterministic binary representation for checkpoint storage.
pub fn encode_entity_map(map: &EntityMap) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(map.len() as u32).to_le_bytes());
    for (entity, (schema_id, fields)) in map {
        out.extend_from_slice(&entity.shard.0.to_le_bytes());
        out.extend_from_slice(&entity.slot.to_le_bytes());
        out.extend_from_slice(&entity.generation.to_le_bytes());
        out.extend_from_slice(&schema_id.0.to_le_bytes());
        out.extend_from_slice(&(fields.len() as u32).to_le_bytes());
        for (field_id, value) in fields {
            out.extend_from_slice(&field_id.0.to_le_bytes());
            // Use canonical sparse mutation value encoder
            let dummy_mutation = Mutation::Upsert(vec![temnion_schema::FieldUpdate {
                field: *field_id,
                value: value.clone(),
            }]);
            let encoded_mut = temnion_schema::encode_mutation(&dummy_mutation, 1024 * 1024)
                .expect("encode value mutation");
            out.extend_from_slice(&(encoded_mut.len() as u32).to_le_bytes());
            out.extend_from_slice(&encoded_mut);
        }
    }
    out
}

/// Deserializes EntityMap from deterministic binary checkpoint payload.
pub fn decode_entity_map(mut bytes: &[u8]) -> Result<EntityMap, ReplayError> {
    if bytes.len() < 4 {
        return Err(ReplayError::Format("incomplete entity map payload"));
    }
    let count = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
    bytes = &bytes[4..];

    let mut map = BTreeMap::new();
    for _ in 0..count {
        if bytes.len() < 20 {
            return Err(ReplayError::Format("incomplete entity record"));
        }
        let shard = temnion_core::ShardId(u32::from_le_bytes(bytes[0..4].try_into().unwrap()));
        let slot = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        let generation = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
        let schema_id = SchemaId(u32::from_le_bytes(bytes[12..16].try_into().unwrap()));
        let field_count = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;
        bytes = &bytes[20..];

        let mut fields = BTreeMap::new();
        for _ in 0..field_count {
            if bytes.len() < 6 {
                return Err(ReplayError::Format("incomplete field record"));
            }
            let field_id = FieldId(u16::from_le_bytes(bytes[0..2].try_into().unwrap()));
            let mut_len = u32::from_le_bytes(bytes[2..6].try_into().unwrap()) as usize;
            bytes = &bytes[6..];
            if bytes.len() < mut_len {
                return Err(ReplayError::Format("incomplete mutation payload"));
            }
            let mutation = decode_mutation(&bytes[..mut_len], mut_len)?;
            bytes = &bytes[mut_len..];

            if let Mutation::Upsert(updates) = mutation {
                if let Some(up) = updates.into_iter().next() {
                    fields.insert(field_id, up.value);
                }
            }
        }
        map.insert(
            EntityId {
                shard,
                slot,
                generation,
            },
            (schema_id, fields),
        );
    }
    Ok(map)
}

/// Standard schema-based state reducer.
pub struct SchemaStateReducer {
    schemas: BTreeMap<SchemaId, Schema>,
}

impl SchemaStateReducer {
    pub fn new(schemas: Vec<Schema>) -> Self {
        let mut map = BTreeMap::new();
        for s in schemas {
            map.insert(s.id(), s);
        }
        Self { schemas: map }
    }
}

impl StateReducer<EntityMap> for SchemaStateReducer {
    fn apply(
        &mut self,
        state: &mut EntityMap,
        event: &StoredEvent,
        _rng: &mut DeterministicRng,
    ) -> Result<(), ReplayError> {
        let schema = self
            .schemas
            .get(&event.schema)
            .ok_or(ReplayError::SchemaMismatch {
                expected: event.schema,
                actual: event.schema,
            })?;

        let mutation = decode_mutation(&event.payload, event.payload.len())?;
        match mutation {
            Mutation::Delete => {
                state.remove(&event.entity);
            }
            Mutation::Upsert(updates) => {
                let entry = state
                    .entry(event.entity)
                    .or_insert_with(|| (event.schema, BTreeMap::new()));
                for update in updates {
                    schema.validate_mutation(&Mutation::Upsert(vec![update.clone()]))?;
                    entry.1.insert(update.field, update.value);
                }
            }
        }
        Ok(())
    }
}

/// Raw map of entity ID to its latest schema ID and raw payload.
pub type RawEntityMap = BTreeMap<EntityId, (SchemaId, Vec<u8>)>;

/// Raw reducer that tracks the latest payload per entity without requiring schema validation.
#[derive(Debug, Default, Clone)]
pub struct RawEntityReducer;

impl StateReducer<RawEntityMap> for RawEntityReducer {
    fn apply(
        &mut self,
        state: &mut RawEntityMap,
        event: &StoredEvent,
        _rng: &mut DeterministicRng,
    ) -> Result<(), ReplayError> {
        state.insert(event.entity, (event.schema, event.payload.clone()));
        Ok(())
    }
}

pub fn encode_raw_entity_map(map: &RawEntityMap) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&(map.len() as u32).to_le_bytes());
    for (entity, (schema, payload)) in map {
        buf.extend_from_slice(&entity.shard.0.to_le_bytes());
        buf.extend_from_slice(&entity.slot.to_le_bytes());
        buf.extend_from_slice(&entity.generation.to_le_bytes());
        buf.extend_from_slice(&schema.0.to_le_bytes());
        buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        buf.extend_from_slice(payload);
    }
    buf
}

pub fn decode_raw_entity_map(bytes: &[u8]) -> Result<RawEntityMap, ReplayError> {
    let mut map = BTreeMap::new();
    if bytes.is_empty() {
        return Ok(map);
    }
    if bytes.len() < 4 {
        return Err(ReplayError::Format("truncated raw entity map count"));
    }
    let count = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
    let mut offset = 4;
    for _ in 0..count {
        if bytes.len() < offset + 4 + 4 + 4 + 4 + 4 {
            return Err(ReplayError::Format("truncated raw entity entry"));
        }
        let shard = ShardId(u32::from_le_bytes(
            bytes[offset..offset + 4].try_into().unwrap(),
        ));
        offset += 4;
        let slot = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        offset += 4;
        let generation = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        offset += 4;
        let schema = SchemaId(u32::from_le_bytes(
            bytes[offset..offset + 4].try_into().unwrap(),
        ));
        offset += 4;
        let p_len = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        if bytes.len() < offset + p_len {
            return Err(ReplayError::Format("truncated raw entity payload"));
        }
        let payload = bytes[offset..offset + p_len].to_vec();
        offset += p_len;
        map.insert(
            EntityId {
                shard,
                slot,
                generation,
            },
            (schema, payload),
        );
    }
    Ok(map)
}

/// ReplayEngine manages checkpoints and deterministic reconstruction of state
/// up to an exact event sequence or target timestamp.
pub struct ReplayEngine {
    store_dir: PathBuf,
    checkpoints: CheckpointStore,
    rng: DeterministicRng,
}

impl ReplayEngine {
    pub fn new(store_dir: impl AsRef<Path>, seed: u64) -> Result<Self, ReplayError> {
        let store_dir = store_dir.as_ref().to_path_buf();
        let checkpoints = CheckpointStore::new(&store_dir)?;
        Ok(Self {
            store_dir,
            checkpoints,
            rng: DeterministicRng::new(seed),
        })
    }

    pub fn checkpoints(&self) -> &CheckpointStore {
        &self.checkpoints
    }

    pub fn rng_mut(&mut self) -> &mut DeterministicRng {
        &mut self.rng
    }

    /// Reconstructs the exact entity state at `target_sequence`.
    /// Restores from the closest prior checkpoint and deterministically applies WAL events.
    pub fn reconstruct_at_sequence<S: Default>(
        &mut self,
        target_sequence: u64,
        reducer: &mut dyn StateReducer<S>,
        decode_state: impl Fn(&[u8]) -> Result<S, ReplayError>,
    ) -> Result<(S, CheckpointHeader), ReplayError> {
        let mut store = Store::open(
            &self.store_dir,
            Limits::default(),
            RecoveryMode::RejectIncompleteTail,
        )?
        .0;

        if target_sequence >= store.len() {
            return Err(ReplayError::Storage(StorageError::InvalidCursor));
        }

        // 1. Locate closest checkpoint <= target_sequence
        let maybe_checkpoint = self.checkpoints.find_latest(target_sequence)?;

        let (mut state, mut next_seq, mut last_header) = match maybe_checkpoint {
            Some(checkpoint) => {
                let state = decode_state(&checkpoint.state_payload)?;
                self.rng.restore(checkpoint.header.rng);
                let next = checkpoint.header.sequence + 1;
                (state, next, checkpoint.header)
            }
            None => {
                let default_header = CheckpointHeader {
                    database: store.header().database,
                    source: store.header().source,
                    epoch: store.header().epoch,
                    sequence: 0,
                    valid_time: Timestamp::new(ClockId(0), 0),
                    known_time: Timestamp::new(ClockId(0), 0),
                    rng: self.rng.snapshot(),
                    state_bytes_len: 0,
                };
                (S::default(), 0, default_header)
            }
        };

        // 2. Stream events from next_seq up to target_sequence
        if next_seq <= target_sequence {
            let cursor = store.cursor_from_sequence(next_seq, HistoryFilter::default())?;
            let page = store.history(
                HistoryFilter::default(),
                StorageQueryBudget {
                    max_results: (target_sequence - next_seq + 1) as usize,
                    max_scanned: 65_536,
                    max_read_bytes: 64 * 1024 * 1024,
                },
                Some(cursor),
            )?;

            for event in page.events {
                if event.id.sequence > target_sequence {
                    break;
                }
                if event.id.sequence != next_seq {
                    return Err(ReplayError::DiscontinuousSequence {
                        expected: next_seq,
                        actual: event.id.sequence,
                    });
                }
                reducer.apply(&mut state, &event, &mut self.rng)?;
                last_header.sequence = event.id.sequence;
                last_header.valid_time = event.times.valid;
                last_header.known_time = event.times.known;
                last_header.rng = self.rng.snapshot();
                next_seq += 1;
            }
        }

        Ok((state, last_header))
    }
}
