// SPDX-License-Identifier: AGPL-3.0-only
//! Compact causal history indexing and event DAG tracing for Temnion.
//!
//! Following Temnion Architecture §18:
//! - Records directed causal edges between events (what caused an action or state change).
//! - Supports bi-directional traversal: immediate causes (upstream) and immediate effects (downstream).
//! - Deep transitive DAG queries: `trace_causes(depth)` and `trace_effects(depth)`.
//! - Compact CSR (Compressed Sparse Row) packed layout for minimal memory and zero-copy slicing.
//! - Enforces strict acyclicity (directed acyclic graph invariant).

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::fmt;

use temnion_core::EventId;

pub const CAUSAL_MAGIC: [u8; 4] = *b"TNCG";
pub const CAUSAL_VERSION: u16 = 1;

#[derive(Debug, PartialEq, Eq)]
pub enum CausalError {
    CycleDetected { from: EventId, to: EventId },
    EventNotFound(EventId),
    Format(&'static str),
    ChecksumMismatch,
    InvalidMagic,
    UnsupportedVersion(u16),
}

impl fmt::Display for CausalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CycleDetected { from, to } => {
                write!(
                    f,
                    "causal cycle detected: edge from {}:{}:{} to {}:{}:{}",
                    from.source.0,
                    from.epoch.0,
                    from.sequence,
                    to.source.0,
                    to.epoch.0,
                    to.sequence
                )
            }
            Self::EventNotFound(id) => {
                write!(
                    f,
                    "event not found in causal graph: {}:{}:{}",
                    id.source.0, id.epoch.0, id.sequence
                )
            }
            Self::Format(msg) => write!(f, "causal graph format error: {msg}"),
            Self::ChecksumMismatch => write!(f, "causal graph checksum mismatch"),
            Self::InvalidMagic => write!(f, "invalid causal graph magic; expected TNCG"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported causal graph version: {v}"),
        }
    }
}

impl Error for CausalError {}

/// Result of a transitive causal trace containing ordered event levels and path connectivity.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CausalTrace {
    /// Ordered list of unique events visited in BFS order.
    pub events: Vec<EventId>,
    /// Distance/depth of each visited event from the root event (root depth = 0).
    pub depths: BTreeMap<EventId, usize>,
    /// Directed edges discovered during traversal (parent -> child in trace direction).
    pub edges: Vec<(EventId, EventId)>,
}

/// Dynamic in-memory graph tracking event causal relationships.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CausalGraph {
    /// Maps each event to its set of direct causes (incoming edges: cause -> event).
    causes: BTreeMap<EventId, Vec<EventId>>,
    /// Maps each event to its set of direct effects (outgoing edges: event -> effect).
    effects: BTreeMap<EventId, Vec<EventId>>,
}

impl CausalGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an event with its declared causal dependencies.
    /// Checks for cycle hazards prior to committing the edges.
    pub fn add_event(
        &mut self,
        event: EventId,
        mut causes: Vec<EventId>,
    ) -> Result<(), CausalError> {
        // Normalize and deduplicate causes
        causes.sort_unstable();
        causes.dedup();

        // Cycle check: event cannot be a causal ancestor of any of its own causes
        for &cause in &causes {
            if cause == event || self.is_ancestor_of(event, cause) {
                return Err(CausalError::CycleDetected {
                    from: event,
                    to: cause,
                });
            }
        }

        // Register causes
        for &cause in &causes {
            let effect_list = self.effects.entry(cause).or_default();
            if !effect_list.contains(&event) {
                effect_list.push(event);
            }
        }

        self.causes.insert(event, causes);
        self.effects.entry(event).or_default();

        Ok(())
    }

    /// Returns whether `ancestor` is an upstream causal ancestor of `descendant`.
    pub fn is_ancestor_of(&self, ancestor: EventId, descendant: EventId) -> bool {
        if ancestor == descendant {
            return true;
        }

        let mut queue = VecDeque::new();
        let mut visited = BTreeSet::new();

        queue.push_back(descendant);
        visited.insert(descendant);

        while let Some(current) = queue.pop_front() {
            if let Some(causes) = self.causes.get(&current) {
                for &c in causes {
                    if c == ancestor {
                        return true;
                    }
                    if visited.insert(c) {
                        queue.push_back(c);
                    }
                }
            }
        }

        false
    }

    /// Returns direct causes of `event`.
    pub fn immediate_causes(&self, event: EventId) -> &[EventId] {
        self.causes.get(&event).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Returns direct effects of `event`.
    pub fn immediate_effects(&self, event: EventId) -> &[EventId] {
        self.effects
            .get(&event)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Traces transitive causal ancestry (causes of causes) up to `max_depth`.
    pub fn trace_causes(&self, root: EventId, max_depth: usize) -> CausalTrace {
        let mut trace = CausalTrace::default();
        let mut visited = BTreeSet::new();
        let mut queue = VecDeque::new();

        queue.push_back((root, 0));
        visited.insert(root);
        trace.events.push(root);
        trace.depths.insert(root, 0);

        while let Some((curr, depth)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }

            for &cause in self.immediate_causes(curr) {
                trace.edges.push((curr, cause));
                if visited.insert(cause) {
                    trace.events.push(cause);
                    trace.depths.insert(cause, depth + 1);
                    queue.push_back((cause, depth + 1));
                }
            }
        }

        trace
    }

    /// Traces transitive downstream effects (effects of effects) up to `max_depth`.
    pub fn trace_effects(&self, root: EventId, max_depth: usize) -> CausalTrace {
        let mut trace = CausalTrace::default();
        let mut visited = BTreeSet::new();
        let mut queue = VecDeque::new();

        queue.push_back((root, 0));
        visited.insert(root);
        trace.events.push(root);
        trace.depths.insert(root, 0);

        while let Some((curr, depth)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }

            for &effect in self.immediate_effects(curr) {
                trace.edges.push((curr, effect));
                if visited.insert(effect) {
                    trace.events.push(effect);
                    trace.depths.insert(effect, depth + 1);
                    queue.push_back((effect, depth + 1));
                }
            }
        }

        trace
    }

    /// Computes a valid topological ordering of events in the graph.
    /// Returns an error if an internal inconsistency or cycle is encountered.
    pub fn topological_sort(&self) -> Result<Vec<EventId>, CausalError> {
        let mut in_degree: BTreeMap<EventId, usize> = BTreeMap::new();

        for (&event, causes) in &self.causes {
            in_degree.entry(event).or_insert(0);
            for &cause in causes {
                in_degree.entry(cause).or_insert(0);
            }
            *in_degree.entry(event).or_default() += causes.len();
        }

        let mut zero_in_degree = VecDeque::new();
        for (&event, &deg) in &in_degree {
            if deg == 0 {
                zero_in_degree.push_back(event);
            }
        }

        let mut order = Vec::with_capacity(in_degree.len());

        while let Some(u) = zero_in_degree.pop_front() {
            order.push(u);

            if let Some(effects) = self.effects.get(&u) {
                for &v in effects {
                    if let Some(deg) = in_degree.get_mut(&v) {
                        *deg = deg.saturating_sub(1);
                        if *deg == 0 {
                            zero_in_degree.push_back(v);
                        }
                    }
                }
            }
        }

        if order.len() != in_degree.len() {
            return Err(CausalError::Format(
                "cycle detected during topological sorting",
            ));
        }

        Ok(order)
    }

    /// Packs the in-memory graph into a zero-allocation CSR (Compressed Sparse Row) representation.
    pub fn to_csr(&self) -> CsrCausalGraph {
        let mut events: Vec<EventId> = self.causes.keys().copied().collect();
        for k in self.effects.keys() {
            if !events.contains(k) {
                events.push(*k);
            }
        }
        events.sort_unstable();
        events.dedup();

        let event_to_idx: BTreeMap<EventId, usize> =
            events.iter().enumerate().map(|(i, &e)| (e, i)).collect();

        let mut cause_offsets = Vec::with_capacity(events.len() + 1);
        let mut cause_indices = Vec::new();

        let mut effect_offsets = Vec::with_capacity(events.len() + 1);
        let mut effect_indices = Vec::new();

        for &event in &events {
            cause_offsets.push(cause_indices.len());
            if let Some(causes) = self.causes.get(&event) {
                for &cause in causes {
                    if let Some(&idx) = event_to_idx.get(&cause) {
                        cause_indices.push(idx as u32);
                    }
                }
            }

            effect_offsets.push(effect_indices.len());
            if let Some(effects) = self.effects.get(&event) {
                for &effect in effects {
                    if let Some(&idx) = event_to_idx.get(&effect) {
                        effect_indices.push(idx as u32);
                    }
                }
            }
        }

        cause_offsets.push(cause_indices.len());
        effect_offsets.push(effect_indices.len());

        CsrCausalGraph {
            events,
            cause_offsets,
            cause_indices,
            effect_offsets,
            effect_indices,
        }
    }
}

/// Compact Compressed Sparse Row (CSR) representation of the causal graph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CsrCausalGraph {
    pub events: Vec<EventId>,
    pub cause_offsets: Vec<usize>,
    pub cause_indices: Vec<u32>,
    pub effect_offsets: Vec<usize>,
    pub effect_indices: Vec<u32>,
}

impl CsrCausalGraph {
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    pub fn immediate_causes(&self, index: usize) -> &[u32] {
        if index >= self.events.len() {
            return &[];
        }
        let start = self.cause_offsets[index];
        let end = self.cause_offsets[index + 1];
        &self.cause_indices[start..end]
    }

    pub fn immediate_effects(&self, index: usize) -> &[u32] {
        if index >= self.events.len() {
            return &[];
        }
        let start = self.effect_offsets[index];
        let end = self.effect_offsets[index + 1];
        &self.effect_indices[start..end]
    }

    /// Serializes CSR structure with checksum framing.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        // Frame header: MAGIC (4) + VERSION (2) + EVENT_COUNT (4) + CRC (4)
        buf.extend_from_slice(&CAUSAL_MAGIC);
        buf.extend_from_slice(&CAUSAL_VERSION.to_le_bytes());
        buf.extend_from_slice(&(self.events.len() as u32).to_le_bytes());

        let mut body = Vec::new();
        // Events
        for e in &self.events {
            body.extend_from_slice(&e.source.0.to_le_bytes());
            body.extend_from_slice(&e.epoch.0.to_le_bytes());
            body.extend_from_slice(&e.sequence.to_le_bytes());
        }

        // Cause offsets & indices
        body.extend_from_slice(&(self.cause_indices.len() as u32).to_le_bytes());
        for &offset in &self.cause_offsets {
            body.extend_from_slice(&(offset as u32).to_le_bytes());
        }
        for &idx in &self.cause_indices {
            body.extend_from_slice(&idx.to_le_bytes());
        }

        // Effect offsets & indices
        body.extend_from_slice(&(self.effect_indices.len() as u32).to_le_bytes());
        for &offset in &self.effect_offsets {
            body.extend_from_slice(&(offset as u32).to_le_bytes());
        }
        for &idx in &self.effect_indices {
            body.extend_from_slice(&idx.to_le_bytes());
        }

        let checksum = crc32fast::hash(&body);
        buf.extend_from_slice(&checksum.to_le_bytes());
        buf.extend_from_slice(&body);
        buf
    }

    /// Deserializes CSR structure and validates checksum and bounds.
    pub fn decode(bytes: &[u8]) -> Result<Self, CausalError> {
        if bytes.len() < 14 {
            return Err(CausalError::Format("truncated causal graph header"));
        }

        if bytes[0..4] != CAUSAL_MAGIC {
            return Err(CausalError::InvalidMagic);
        }

        let version = u16::from_le_bytes(bytes[4..6].try_into().unwrap());
        if version != CAUSAL_VERSION {
            return Err(CausalError::UnsupportedVersion(version));
        }

        let event_count = u32::from_le_bytes(bytes[6..10].try_into().unwrap()) as usize;
        let expected_checksum = u32::from_le_bytes(bytes[10..14].try_into().unwrap());
        let body = &bytes[14..];

        let actual_checksum = crc32fast::hash(body);
        if actual_checksum != expected_checksum {
            return Err(CausalError::ChecksumMismatch);
        }

        let mut offset = 0;
        let mut events = Vec::with_capacity(event_count);
        for _ in 0..event_count {
            if body.len() < offset + 4 + 8 + 8 {
                return Err(CausalError::Format("truncated event records"));
            }
            let source = temnion_core::SourceId(u32::from_le_bytes(
                body[offset..offset + 4].try_into().unwrap(),
            ));
            offset += 4;
            let epoch = temnion_core::SourceEpoch(u64::from_le_bytes(
                body[offset..offset + 8].try_into().unwrap(),
            ));
            offset += 8;
            let sequence = u64::from_le_bytes(body[offset..offset + 8].try_into().unwrap());
            offset += 8;
            events.push(EventId {
                source,
                epoch,
                sequence,
            });
        }

        // Cause offsets & indices
        if body.len() < offset + 4 {
            return Err(CausalError::Format("truncated cause indices length"));
        }
        let cause_indices_len =
            u32::from_le_bytes(body[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;

        if body.len() < offset + (event_count + 1) * 4 + cause_indices_len * 4 {
            return Err(CausalError::Format("truncated cause offsets/indices"));
        }

        let mut cause_offsets = Vec::with_capacity(event_count + 1);
        for _ in 0..=event_count {
            let off = u32::from_le_bytes(body[offset..offset + 4].try_into().unwrap()) as usize;
            offset += 4;
            cause_offsets.push(off);
        }

        let mut cause_indices = Vec::with_capacity(cause_indices_len);
        for _ in 0..cause_indices_len {
            let idx = u32::from_le_bytes(body[offset..offset + 4].try_into().unwrap());
            offset += 4;
            cause_indices.push(idx);
        }

        // Effect offsets & indices
        if body.len() < offset + 4 {
            return Err(CausalError::Format("truncated effect indices length"));
        }
        let effect_indices_len =
            u32::from_le_bytes(body[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;

        if body.len() < offset + (event_count + 1) * 4 + effect_indices_len * 4 {
            return Err(CausalError::Format("truncated effect offsets/indices"));
        }

        let mut effect_offsets = Vec::with_capacity(event_count + 1);
        for _ in 0..=event_count {
            let off = u32::from_le_bytes(body[offset..offset + 4].try_into().unwrap()) as usize;
            offset += 4;
            effect_offsets.push(off);
        }

        let mut effect_indices = Vec::with_capacity(effect_indices_len);
        for _ in 0..effect_indices_len {
            let idx = u32::from_le_bytes(body[offset..offset + 4].try_into().unwrap());
            offset += 4;
            effect_indices.push(idx);
        }

        Ok(CsrCausalGraph {
            events,
            cause_offsets,
            cause_indices,
            effect_offsets,
            effect_indices,
        })
    }
}
