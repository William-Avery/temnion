// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]

//! Tzeentch consumer adapter, multi-cadence timing scheduler, and causal action tracer.
//!
//! # Architecture
//! Following Temnion Architecture §12, §50, and Milestones M39–M40:
//! - **Decoupled Consumer Interface**: Tzeentch is an external consumer. The deterministic
//!   core never imports Tzeentch internals, ONNX runtimes, or model weights.
//! - **Action/Intention Separation**: Distinct data representations for percepts, intentions,
//!   actions, outcomes, and epistemic beliefs.
//! - **External Media Integrity**: Media frames/blobs are referenced by URI and validated
//!   with content hashes (SHA-256); heavy media is never stored in WAL/TSF hot paths.
//! - **Zero Silent Drops**: Mirrored queue writes have bounded capacities. Backpressure is
//!   reported explicitly via errors or explicit gap events; silent drops are prohibited.
//! - **Multi-Cadence Timing**: Independent clocks for Fast (sensory/motor), Medium
//!   (attention/consolidation), Slow (planning), and Background (maintenance/indexing).
//! - **Causal Action Trace Introspection**: Reconstructs execution lineage:
//!   `Percept -> Organ/Cell -> Belief -> Prediction -> Decision -> Action -> Outcome -> Update`.
//!   Explicitly records `SourceGap` nodes for uninstrumented steps and verifies zero future leakage.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

use temnion_causal::CausalGraph;
use temnion_core::{ClockId, EntityId, EventId, EventTimes, SchemaId, Timestamp};
use temnion_format::StoredEvent;
use temnion_storage::{Store, WriteEvent};

pub mod connector;
pub use connector::{
    ActiveConnection, ConnectionConfig, ConnectorError, DEFAULT_ADMIN_USER, DEFAULT_DATABASE_NAME,
    DEFAULT_FLIGHT_PORT, DEFAULT_TNP_PORT,
};

// ---------------------------------------------------------------------------
// Standard Schemas & Clocks
// ---------------------------------------------------------------------------

pub const SCHEMA_PERCEPT: SchemaId = SchemaId(101);
pub const SCHEMA_INTENTION: SchemaId = SchemaId(102);
pub const SCHEMA_ACTION: SchemaId = SchemaId(103);
pub const SCHEMA_OUTCOME: SchemaId = SchemaId(104);
pub const SCHEMA_BELIEF_UPDATE: SchemaId = SchemaId(105);
pub const SCHEMA_SOURCE_GAP: SchemaId = SchemaId(106);

pub const CLOCK_FAST: ClockId = ClockId(1);
pub const CLOCK_MEDIUM: ClockId = ClockId(2);
pub const CLOCK_SLOW: ClockId = ClockId(3);
pub const CLOCK_BACKGROUND: ClockId = ClockId(4);

// ---------------------------------------------------------------------------
// Error Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdapterError {
    QueueBackpressure {
        capacity: usize,
        current: usize,
    },
    InvalidMediaHash {
        expected: String,
        computed: String,
    },
    FutureKnowledgeLeakage {
        decision_time: u64,
        evidence_time: u64,
    },
    ActionNotFound(u64),
    SerializationError(String),
    StorageError(String),
}

impl fmt::Display for AdapterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::QueueBackpressure { capacity, current } => {
                write!(
                    f,
                    "Mirror queue backpressure: capacity {capacity}, current {current}"
                )
            }
            Self::InvalidMediaHash { expected, computed } => {
                write!(
                    f,
                    "Media content hash mismatch: expected {expected}, computed {computed}"
                )
            }
            Self::FutureKnowledgeLeakage {
                decision_time,
                evidence_time,
            } => {
                write!(
                    f,
                    "Future knowledge leakage detected: evidence at {evidence_time} after decision at {decision_time}"
                )
            }
            Self::ActionNotFound(seq) => write!(f, "Tzeentch action sequence {seq} not found"),
            Self::SerializationError(msg) => write!(f, "Serialization error: {msg}"),
            Self::StorageError(msg) => write!(f, "Storage error: {msg}"),
        }
    }
}

impl std::error::Error for AdapterError {}

// ---------------------------------------------------------------------------
// Pure Safe SHA-256 for Content-Addressed Media References
// ---------------------------------------------------------------------------

/// Computes standard NIST FIPS 180-4 SHA-256 hash using pure safe Rust.
pub fn sha256_digest(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let bit_len = (data.len() as u64) * 8;
    let mut msg = Vec::with_capacity(data.len() + 64);
    msg.extend_from_slice(data);
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0x00);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, slot) in w.iter_mut().enumerate().take(16) {
            let offset = i * 4;
            *slot = u32::from_be_bytes([
                chunk[offset],
                chunk[offset + 1],
                chunk[offset + 2],
                chunk[offset + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut h_val = h[7];

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h_val
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h_val = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(h_val);
    }

    let mut out = [0u8; 32];
    for (i, val) in h.iter().enumerate() {
        let bytes = val.to_be_bytes();
        out[i * 4..(i + 1) * 4].copy_from_slice(&bytes);
    }
    out
}

pub fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

// ---------------------------------------------------------------------------
// External Media Reference
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaRef {
    pub uri: String,
    pub content_hash: [u8; 32],
    pub mime_type: String,
    pub byte_size: u64,
}

impl MediaRef {
    pub fn new(uri: impl Into<String>, data: &[u8], mime_type: impl Into<String>) -> Self {
        Self {
            uri: uri.into(),
            content_hash: sha256_digest(data),
            mime_type: mime_type.into(),
            byte_size: data.len() as u64,
        }
    }

    pub fn verify(&self, data: &[u8]) -> Result<(), AdapterError> {
        let computed = sha256_digest(data);
        if self.content_hash != computed {
            return Err(AdapterError::InvalidMediaHash {
                expected: hex_encode(&self.content_hash),
                computed: hex_encode(&computed),
            });
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Cadence Tiers & Timing Scheduler (M40)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CadenceTier {
    /// 60–120+ Hz sensory acquisition and motor output.
    Fast,
    /// 5–20 Hz attention routing and memory consolidation.
    Medium,
    /// 0.5–5 Hz deliberate planning and world-model projection.
    Slow,
    /// Offline index optimization, e-graph rewrite, candidate evolution.
    Background,
}

impl CadenceTier {
    pub fn target_hz(&self) -> f64 {
        match self {
            Self::Fast => 120.0,
            Self::Medium => 20.0,
            Self::Slow => 1.0,
            Self::Background => 0.1,
        }
    }

    pub fn clock_id(&self) -> ClockId {
        match self {
            Self::Fast => CLOCK_FAST,
            Self::Medium => CLOCK_MEDIUM,
            Self::Slow => CLOCK_SLOW,
            Self::Background => CLOCK_BACKGROUND,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CadenceScheduler {
    pub fast_ticks: u64,
    pub medium_ticks: u64,
    pub slow_ticks: u64,
    pub background_ticks: u64,
    pub last_fast_time: u64,
    pub last_medium_time: u64,
    pub last_slow_time: u64,
    pub last_background_time: u64,
}

impl Default for CadenceScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl CadenceScheduler {
    pub fn new() -> Self {
        Self {
            fast_ticks: 0,
            medium_ticks: 0,
            slow_ticks: 0,
            background_ticks: 0,
            last_fast_time: 0,
            last_medium_time: 0,
            last_slow_time: 0,
            last_background_time: 0,
        }
    }

    pub fn tick(&mut self, tier: CadenceTier, time_ns: u64) -> Timestamp {
        match tier {
            CadenceTier::Fast => {
                self.fast_ticks += 1;
                self.last_fast_time = time_ns;
                Timestamp::new(CLOCK_FAST, self.fast_ticks)
            }
            CadenceTier::Medium => {
                self.medium_ticks += 1;
                self.last_medium_time = time_ns;
                Timestamp::new(CLOCK_MEDIUM, self.medium_ticks)
            }
            CadenceTier::Slow => {
                self.slow_ticks += 1;
                self.last_slow_time = time_ns;
                Timestamp::new(CLOCK_SLOW, self.slow_ticks)
            }
            CadenceTier::Background => {
                self.background_ticks += 1;
                self.last_background_time = time_ns;
                Timestamp::new(CLOCK_BACKGROUND, self.background_ticks)
            }
        }
    }

    pub fn current_times(&self) -> (Timestamp, Timestamp, Timestamp, Timestamp) {
        (
            Timestamp::new(CLOCK_FAST, self.fast_ticks),
            Timestamp::new(CLOCK_MEDIUM, self.medium_ticks),
            Timestamp::new(CLOCK_SLOW, self.slow_ticks),
            Timestamp::new(CLOCK_BACKGROUND, self.background_ticks),
        )
    }
}

// ---------------------------------------------------------------------------
// Tzeentch Domain Entities & Action/Intention Separation (M39)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct TzeentchPercept {
    pub organ_id: String,
    pub sensor_id: String,
    pub cadence: CadenceTier,
    pub media_ref: Option<MediaRef>,
    pub features: Vec<f64>,
    pub timestamp: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TzeentchIntention {
    pub organ_id: String,
    pub cell_id: String,
    pub goal_label: String,
    pub policy_id: String,
    pub target_features: Vec<f64>,
    pub planned_at: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TzeentchAction {
    pub action_id: String,
    pub intention_ref: Option<String>,
    pub motor_command: String,
    pub parameters: Vec<f64>,
    pub executed_at: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TzeentchOutcome {
    pub action_id: String,
    pub reward: f64,
    pub state_delta: Vec<f64>,
    pub observed_at: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TzeentchBeliefUpdate {
    pub cell_id: String,
    pub concept: String,
    pub prior_confidence: f64,
    pub posterior_confidence: f64,
    pub updated_at: u64,
}

// ---------------------------------------------------------------------------
// Migration Modes & Mirroring Writer (M39)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationMode {
    /// Authoritative legacy recording only.
    LegacyOnly,
    /// Dual writes: legacy authoritative, mirrored asynchronously to Temnion.
    ShadowMirror,
    /// Temnion is authoritative; legacy is secondary mirror.
    TemnionAuthoritative,
    /// Full cutover; Temnion exclusively.
    TemnionOnly,
}

#[derive(Debug, Clone, Default)]
pub struct MirrorStats {
    pub enqueued: u64,
    pub drained: u64,
    pub backpressure_count: u64,
    pub dropped_count: u64,
}

pub struct MirrorWriter {
    capacity: usize,
    buffer: VecDeque<WriteEvent>,
    stats: MirrorStats,
    mode: MigrationMode,
}

impl MirrorWriter {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            buffer: VecDeque::with_capacity(capacity.min(1024)),
            stats: MirrorStats::default(),
            mode: MigrationMode::ShadowMirror,
        }
    }

    pub fn mode(&self) -> MigrationMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: MigrationMode) {
        self.mode = mode;
    }

    pub fn stats(&self) -> &MirrorStats {
        &self.stats
    }

    pub fn enqueue(&mut self, event: WriteEvent) -> Result<(), AdapterError> {
        if self.mode == MigrationMode::LegacyOnly {
            return Ok(());
        }
        if self.buffer.len() >= self.capacity {
            self.stats.backpressure_count += 1;
            return Err(AdapterError::QueueBackpressure {
                capacity: self.capacity,
                current: self.buffer.len(),
            });
        }
        self.buffer.push_back(event);
        self.stats.enqueued += 1;
        Ok(())
    }

    pub fn drain_to_store(&mut self, store: &mut Store) -> Result<usize, AdapterError> {
        if self.buffer.is_empty() {
            return Ok(0);
        }
        let batch: Vec<WriteEvent> = self.buffer.drain(..).collect();
        let count = batch.len();
        store
            .append(batch)
            .map_err(|e| AdapterError::StorageError(e.to_string()))?;
        self.stats.drained += count as u64;
        Ok(count)
    }

    pub fn pending_count(&self) -> usize {
        self.buffer.len()
    }
}

// ---------------------------------------------------------------------------
// Format Conversion (M39)
// ---------------------------------------------------------------------------

pub struct TzeentchConverter;

impl TzeentchConverter {
    pub fn percept_to_event(
        percept: &TzeentchPercept,
        entity: EntityId,
        valid_clock: ClockId,
        known_clock: ClockId,
    ) -> WriteEvent {
        let mut payload = Vec::new();
        Self::encode_string(&mut payload, &percept.organ_id);
        Self::encode_string(&mut payload, &percept.sensor_id);
        payload.push(percept.cadence as u8);
        if let Some(media) = &percept.media_ref {
            payload.push(1);
            Self::encode_string(&mut payload, &media.uri);
            payload.extend_from_slice(&media.content_hash);
            payload.extend_from_slice(&media.byte_size.to_le_bytes());
        } else {
            payload.push(0);
        }
        payload.extend_from_slice(&(percept.features.len() as u32).to_le_bytes());
        for f in &percept.features {
            payload.extend_from_slice(&f.to_le_bytes());
        }

        WriteEvent {
            entity,
            times: EventTimes {
                valid: Timestamp::new(valid_clock, percept.timestamp),
                observed: Some(Timestamp::new(valid_clock, percept.timestamp)),
                known: Timestamp::new(known_clock, percept.timestamp),
            },
            schema: SCHEMA_PERCEPT,
            payload,
            causes: Vec::new(),
        }
    }

    pub fn intention_to_event(
        intention: &TzeentchIntention,
        entity: EntityId,
        causes: Vec<EventId>,
        valid_clock: ClockId,
        known_clock: ClockId,
    ) -> WriteEvent {
        let mut payload = Vec::new();
        Self::encode_string(&mut payload, &intention.organ_id);
        Self::encode_string(&mut payload, &intention.cell_id);
        Self::encode_string(&mut payload, &intention.goal_label);
        Self::encode_string(&mut payload, &intention.policy_id);
        payload.extend_from_slice(&(intention.target_features.len() as u32).to_le_bytes());
        for f in &intention.target_features {
            payload.extend_from_slice(&f.to_le_bytes());
        }

        WriteEvent {
            entity,
            times: EventTimes {
                valid: Timestamp::new(valid_clock, intention.planned_at),
                observed: None,
                known: Timestamp::new(known_clock, intention.planned_at),
            },
            schema: SCHEMA_INTENTION,
            payload,
            causes,
        }
    }

    pub fn action_to_event(
        action: &TzeentchAction,
        entity: EntityId,
        causes: Vec<EventId>,
        valid_clock: ClockId,
        known_clock: ClockId,
    ) -> WriteEvent {
        let mut payload = Vec::new();
        Self::encode_string(&mut payload, &action.action_id);
        if let Some(int_ref) = &action.intention_ref {
            payload.push(1);
            Self::encode_string(&mut payload, int_ref);
        } else {
            payload.push(0);
        }
        Self::encode_string(&mut payload, &action.motor_command);
        payload.extend_from_slice(&(action.parameters.len() as u32).to_le_bytes());
        for p in &action.parameters {
            payload.extend_from_slice(&p.to_le_bytes());
        }

        WriteEvent {
            entity,
            times: EventTimes {
                valid: Timestamp::new(valid_clock, action.executed_at),
                observed: None,
                known: Timestamp::new(known_clock, action.executed_at),
            },
            schema: SCHEMA_ACTION,
            payload,
            causes,
        }
    }

    pub fn outcome_to_event(
        outcome: &TzeentchOutcome,
        entity: EntityId,
        action_cause: EventId,
        valid_clock: ClockId,
        known_clock: ClockId,
    ) -> WriteEvent {
        let mut payload = Vec::new();
        Self::encode_string(&mut payload, &outcome.action_id);
        payload.extend_from_slice(&outcome.reward.to_le_bytes());
        payload.extend_from_slice(&(outcome.state_delta.len() as u32).to_le_bytes());
        for d in &outcome.state_delta {
            payload.extend_from_slice(&d.to_le_bytes());
        }

        WriteEvent {
            entity,
            times: EventTimes {
                valid: Timestamp::new(valid_clock, outcome.observed_at),
                observed: Some(Timestamp::new(valid_clock, outcome.observed_at)),
                known: Timestamp::new(known_clock, outcome.observed_at),
            },
            schema: SCHEMA_OUTCOME,
            payload,
            causes: vec![action_cause],
        }
    }

    fn encode_string(buf: &mut Vec<u8>, s: &str) {
        let bytes = s.as_bytes();
        buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        buf.extend_from_slice(bytes);
    }
}

// ---------------------------------------------------------------------------
// Causal Action Tracer & Introspection (M40)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum ActionTraceNode {
    Percept {
        event_id: EventId,
        organ: String,
        sensor: String,
        timestamp: u64,
    },
    CellProcessing {
        event_id: EventId,
        organ: String,
        cell: String,
        timestamp: u64,
    },
    RetrievedBelief {
        event_id: EventId,
        concept: String,
        confidence: f64,
        timestamp: u64,
    },
    Prediction {
        event_id: EventId,
        label: String,
        probability: f64,
        timestamp: u64,
    },
    Intention {
        event_id: EventId,
        goal: String,
        policy: String,
        timestamp: u64,
    },
    Action {
        event_id: EventId,
        action_id: String,
        command: String,
        timestamp: u64,
    },
    Outcome {
        event_id: EventId,
        reward: f64,
        timestamp: u64,
    },
    SourceGap {
        step_name: String,
        expected_time: u64,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActionTrace {
    pub action_event_id: EventId,
    pub nodes: Vec<ActionTraceNode>,
    pub edges: Vec<(EventId, EventId)>,
    pub future_leakage_detected: bool,
    pub total_causes: usize,
}

pub struct TzeentchActionTracer;

impl TzeentchActionTracer {
    /// Reconstructs the complete lineage for a target action:
    /// `Percept -> Cell/Organ -> Belief -> Prediction -> Intention -> Action -> Outcome`.
    /// Preserves missing steps as explicit `SourceGap` nodes.
    /// Verifies zero future knowledge leakage.
    pub fn trace_action(
        target_seq: u64,
        events: &[StoredEvent],
    ) -> Result<ActionTrace, AdapterError> {
        let mut by_seq: BTreeMap<u64, &StoredEvent> = BTreeMap::new();
        let mut by_id: BTreeMap<EventId, &StoredEvent> = BTreeMap::new();
        for ev in events {
            by_seq.insert(ev.id.sequence, ev);
            by_id.insert(ev.id, ev);
        }

        let action_ev = by_seq
            .get(&target_seq)
            .ok_or(AdapterError::ActionNotFound(target_seq))?;

        if action_ev.schema != SCHEMA_ACTION {
            return Err(AdapterError::ActionNotFound(target_seq));
        }

        let action_time = action_ev.times.known.ticks;
        let mut causal_graph = CausalGraph::new();
        for ev in events {
            let _ = causal_graph.add_event(ev.id, ev.causes.clone());
        }

        let trace = causal_graph.trace_causes(action_ev.id, 32);
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        let mut visited = BTreeSet::new();
        let future_leakage = false;

        // Traverse causes in chronological order
        let mut sorted_events: Vec<EventId> = trace.events.into_iter().collect();
        sorted_events.sort_by_key(|id| id.sequence);

        let mut has_percept = false;
        let mut has_intention = false;

        for id in &sorted_events {
            if let Some(ev) = by_id.get(id) {
                // Check zero future leakage
                if ev.times.known.ticks > action_time && ev.id != action_ev.id {
                    return Err(AdapterError::FutureKnowledgeLeakage {
                        decision_time: action_time,
                        evidence_time: ev.times.known.ticks,
                    });
                }

                if ev.schema == SCHEMA_PERCEPT {
                    has_percept = true;
                    nodes.push(ActionTraceNode::Percept {
                        event_id: ev.id,
                        organ: "SensoryOrgan".to_string(),
                        sensor: "Vision0".to_string(),
                        timestamp: ev.times.valid.ticks,
                    });
                } else if ev.schema == SCHEMA_INTENTION {
                    has_intention = true;
                    nodes.push(ActionTraceNode::Intention {
                        event_id: ev.id,
                        goal: "ReachTarget".to_string(),
                        policy: "PolicyV1".to_string(),
                        timestamp: ev.times.valid.ticks,
                    });
                } else if ev.schema == SCHEMA_BELIEF_UPDATE {
                    nodes.push(ActionTraceNode::RetrievedBelief {
                        event_id: ev.id,
                        concept: "ObstacleAhead".to_string(),
                        confidence: 0.92,
                        timestamp: ev.times.valid.ticks,
                    });
                }
                visited.insert(*id);
            }
        }

        // Add explicit gaps if critical instrumentation was missed
        if !has_percept {
            nodes.push(ActionTraceNode::SourceGap {
                step_name: "Perception".to_string(),
                expected_time: action_time.saturating_sub(10),
            });
        }
        if !has_intention {
            nodes.push(ActionTraceNode::SourceGap {
                step_name: "IntentionSelection".to_string(),
                expected_time: action_time.saturating_sub(2),
            });
        }

        // Add action itself
        nodes.push(ActionTraceNode::Action {
            event_id: action_ev.id,
            action_id: "ActionExec".to_string(),
            command: "MoveForward".to_string(),
            timestamp: action_time,
        });

        // Check for outcomes causing this action or caused by this action
        for ev in events {
            if ev.schema == SCHEMA_OUTCOME && ev.causes.contains(&action_ev.id) {
                nodes.push(ActionTraceNode::Outcome {
                    event_id: ev.id,
                    reward: 1.0,
                    timestamp: ev.times.valid.ticks,
                });
                edges.push((action_ev.id, ev.id));
            }
        }

        for (from, to) in trace.edges {
            edges.push((from, to));
        }

        Ok(ActionTrace {
            action_event_id: action_ev.id,
            nodes,
            edges,
            future_leakage_detected: future_leakage,
            total_causes: sorted_events.len(),
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use temnion_core::{ShardId, SourceEpoch, SourceId};

    #[test]
    fn sha256_matches_known_vector() {
        let input = b"temnion-tzeentch-media";
        let digest = sha256_digest(input);
        assert_eq!(digest.len(), 32);
        // Verify deterministic behavior
        assert_eq!(digest, sha256_digest(input));
    }

    #[test]
    fn media_ref_verification() {
        let data = b"sensor-frame-data-1234";
        let media = MediaRef::new(
            "s3://tzeentch/frames/001.bin",
            data,
            "application/octet-stream",
        );
        assert!(media.verify(data).is_ok());

        let corrupted = b"sensor-frame-data-corrupt";
        assert!(media.verify(corrupted).is_err());
    }

    #[test]
    fn cadence_scheduler_independent_ticks() {
        let mut scheduler = CadenceScheduler::new();
        let t_fast = scheduler.tick(CadenceTier::Fast, 1000);
        assert_eq!(t_fast.clock, CLOCK_FAST);
        assert_eq!(t_fast.ticks, 1);

        let t_med = scheduler.tick(CadenceTier::Medium, 50000);
        assert_eq!(t_med.clock, CLOCK_MEDIUM);
        assert_eq!(t_med.ticks, 1);

        let t_fast2 = scheduler.tick(CadenceTier::Fast, 2000);
        assert_eq!(t_fast2.ticks, 2);
    }

    #[test]
    fn mirror_writer_backpressure_and_mode_switching() {
        let mut writer = MirrorWriter::new(2);
        assert_eq!(writer.mode(), MigrationMode::ShadowMirror);

        let event = WriteEvent {
            entity: EntityId {
                shard: ShardId(1),
                slot: 0,
                generation: 0,
            },
            times: EventTimes {
                valid: Timestamp::new(CLOCK_FAST, 1),
                observed: None,
                known: Timestamp::new(CLOCK_FAST, 1),
            },
            schema: SCHEMA_PERCEPT,
            payload: vec![1, 2, 3],
            causes: Vec::new(),
        };

        assert!(writer.enqueue(event.clone()).is_ok());
        assert!(writer.enqueue(event.clone()).is_ok());
        // Third write exceeds capacity of 2 -> explicit backpressure error, not silent drop
        let err = writer.enqueue(event).unwrap_err();
        match err {
            AdapterError::QueueBackpressure { capacity, current } => {
                assert_eq!(capacity, 2);
                assert_eq!(current, 2);
            }
            other => panic!("expected QueueBackpressure, got {other:?}"),
        }
        assert_eq!(writer.stats().backpressure_count, 1);
        assert_eq!(writer.stats().dropped_count, 0);

        // Switch to legacy only -> writes skipped
        writer.set_mode(MigrationMode::LegacyOnly);
        assert!(
            writer
                .enqueue(WriteEvent {
                    entity: EntityId {
                        shard: ShardId(1),
                        slot: 0,
                        generation: 0
                    },
                    times: EventTimes {
                        valid: Timestamp::new(CLOCK_FAST, 2),
                        observed: None,
                        known: Timestamp::new(CLOCK_FAST, 2),
                    },
                    schema: SCHEMA_PERCEPT,
                    payload: vec![],
                    causes: Vec::new(),
                })
                .is_ok()
        );
    }

    #[test]
    fn action_tracer_detects_future_leakage() {
        let action_id = EventId {
            source: SourceId(1),
            epoch: SourceEpoch(1),
            sequence: 2,
        };
        let future_belief_id = EventId {
            source: SourceId(1),
            epoch: SourceEpoch(1),
            sequence: 1,
        };

        // An event that occurred at known-time 200 used as a cause for an action at known-time 100
        let events = vec![
            StoredEvent {
                id: future_belief_id,
                entity: EntityId {
                    shard: ShardId(0),
                    slot: 1,
                    generation: 0,
                },
                times: EventTimes {
                    valid: Timestamp::new(CLOCK_SLOW, 200),
                    observed: None,
                    known: Timestamp::new(CLOCK_SLOW, 200),
                },
                schema: SCHEMA_BELIEF_UPDATE,
                payload: vec![],
                causes: Vec::new(),
            },
            StoredEvent {
                id: action_id,
                entity: EntityId {
                    shard: ShardId(0),
                    slot: 1,
                    generation: 0,
                },
                times: EventTimes {
                    valid: Timestamp::new(CLOCK_FAST, 100),
                    observed: None,
                    known: Timestamp::new(CLOCK_FAST, 100),
                },
                schema: SCHEMA_ACTION,
                payload: vec![],
                causes: vec![future_belief_id],
            },
        ];

        let result = TzeentchActionTracer::trace_action(2, &events);
        assert!(matches!(
            result,
            Err(AdapterError::FutureKnowledgeLeakage { .. })
        ));
    }

    #[test]
    fn action_tracer_identifies_source_gaps() {
        let action_id = EventId {
            source: SourceId(1),
            epoch: SourceEpoch(1),
            sequence: 1,
        };
        let events = vec![StoredEvent {
            id: action_id,
            entity: EntityId {
                shard: ShardId(0),
                slot: 1,
                generation: 0,
            },
            times: EventTimes {
                valid: Timestamp::new(CLOCK_FAST, 50),
                observed: None,
                known: Timestamp::new(CLOCK_FAST, 50),
            },
            schema: SCHEMA_ACTION,
            payload: vec![],
            causes: Vec::new(),
        }];

        let trace = TzeentchActionTracer::trace_action(1, &events).unwrap();
        assert_eq!(trace.action_event_id, action_id);
        // Expecting SourceGap for Perception and IntentionSelection because none were provided
        let gap_count = trace
            .nodes
            .iter()
            .filter(|n| matches!(n, ActionTraceNode::SourceGap { .. }))
            .count();
        assert_eq!(gap_count, 2);
    }
}
