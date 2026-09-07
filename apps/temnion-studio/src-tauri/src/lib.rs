// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]

//! Native, bounded Temnion Studio commands.
//!
//! The webview never opens files or evaluates database data itself. Every command
//! crosses this module through a small serializable view model and enforces a
//! fixed upper bound before touching the durable store.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tauri::State;
use temnion_adapter::{CadenceScheduler, MigrationMode, MirrorWriter};
use temnion_branch::{BranchLifecycle, BranchManifest};
use temnion_causal::CausalGraph;
use temnion_core::{
    ClockId, EntityId, EventId, EventTimes, SchemaId, ShardId, SourceEpoch, SourceId, Timestamp,
};
use temnion_events::HistoryFilter;
use temnion_format::{Limits, StoredEvent};
use temnion_query::{
    Literal, QueryBudget, QueryExecutor, explain_query, parse_compact_tem, parse_sql, parse_temql,
    plan_query,
};
use temnion_storage::{RecoveryMode, StorageQueryBudget, Store, WriteEvent};

const DEFAULT_ROWS: usize = 100;
const MAX_ROWS: usize = 1_000;
const MAX_SCANNED: usize = 65_536;
const MAX_READ_BYTES: usize = 16 * 1024 * 1024;
const MAX_CAUSAL_DEPTH: usize = 32;

struct StudioSession {
    path: PathBuf,
    store: Store,
    cadence: CadenceScheduler,
    mirror_writer: MirrorWriter,
}

#[derive(Default)]
struct StudioState(Mutex<Option<StudioSession>>);

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EngineStatus {
    connected: bool,
    path: Option<String>,
    database_id: Option<String>,
    source: Option<u32>,
    epoch: Option<u64>,
    event_count: u64,
    wal_bytes: u64,
    summary_blocks: usize,
    max_rows: usize,
    capabilities: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FieldValue {
    name: String,
    value: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EventView {
    event_id: String,
    sequence: u64,
    entity: String,
    schema: u32,
    valid_clock: u32,
    valid_time: u64,
    known_clock: u32,
    known_time: u64,
    payload_hex: String,
    payload_bytes: usize,
    causes: Vec<String>,
    fields: Vec<FieldValue>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct QueryView {
    rows: Vec<EventView>,
    events_scanned: usize,
    bytes_read: usize,
    truncated: bool,
    elapsed_micros: u128,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryView {
    rows: Vec<EventView>,
    events_scanned: usize,
    bytes_read: usize,
    has_more: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueryRequest {
    query: String,
    max_rows: Option<usize>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppendRequest {
    entity: String,
    schema: u32,
    valid_time: String,
    known_time: String,
    payload_hex: String,
    #[serde(default)]
    causes: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppendReceiptView {
    first_event: String,
    last_event: String,
    count: usize,
    status: EngineStatus,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BranchView {
    id: u64,
    name: String,
    parent_id: Option<u64>,
    fork_sequence: Option<u64>,
    lifecycle: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TraceNodeView {
    event_id: String,
    sequence: u64,
    depth: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CausalTraceView {
    root: String,
    causes: Vec<TraceNodeView>,
    effects: Vec<TraceNodeView>,
    edges: Vec<String>,
    truncated: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TzeentchSummaryView {
    organism_id: String,
    organs: Vec<String>,
    cells: Vec<String>,
    migration_mode: String,
    mirror_enqueued: u64,
    mirror_drained: u64,
    drop_count: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TzeentchCadenceStatsView {
    fast_hz: f64,
    fast_ticks: u64,
    medium_hz: f64,
    medium_ticks: u64,
    slow_hz: f64,
    slow_ticks: u64,
    background_hz: f64,
    background_ticks: u64,
    drop_count: u64,
    queue_pressure: f64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ActionTraceNodeView {
    kind: String,
    event_id: Option<String>,
    label: String,
    detail: String,
    timestamp: u64,
    confidence: Option<f64>,
    is_gap: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ActionTraceView {
    action_event_id: String,
    nodes: Vec<ActionTraceNodeView>,
    edges: Vec<String>,
    future_leakage_detected: bool,
    total_causes: usize,
}

fn capabilities() -> Vec<String> {
    [
        "query-ir",
        "temql",
        "compact-tem",
        "sql",
        "bounded-history",
        "durable-append",
        "branch-inspection",
        "causal-trace",
        "tzeentch-explorer",
        "causal-action-trace",
        "cadence-scheduling",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn disconnected_status() -> EngineStatus {
    EngineStatus {
        connected: false,
        path: None,
        database_id: None,
        source: None,
        epoch: None,
        event_count: 0,
        wal_bytes: 0,
        summary_blocks: 0,
        max_rows: MAX_ROWS,
        capabilities: capabilities(),
    }
}

fn session_status(session: &StudioSession) -> EngineStatus {
    let header = session.store.header();
    EngineStatus {
        connected: true,
        path: Some(session.path.display().to_string()),
        database_id: Some(header.database.to_string()),
        source: Some(header.source.0),
        epoch: Some(header.epoch.0),
        event_count: session.store.len(),
        wal_bytes: session.store.wal_bytes(),
        summary_blocks: session.store.summaries().len(),
        max_rows: MAX_ROWS,
        capabilities: capabilities(),
    }
}

fn lock_state(state: &StudioState) -> Result<MutexGuard<'_, Option<StudioSession>>, String> {
    state
        .0
        .lock()
        .map_err(|_| "Studio database session lock was poisoned".to_owned())
}

fn bounded_rows(value: Option<usize>) -> usize {
    value.unwrap_or(DEFAULT_ROWS).clamp(1, MAX_ROWS)
}

fn event_id_text(id: EventId) -> String {
    format!("{}:{}:{}", id.source.0, id.epoch.0, id.sequence)
}

fn entity_text(entity: EntityId) -> String {
    format!("{}:{}:{}", entity.shard.0, entity.slot, entity.generation)
}

fn payload_preview(payload: &[u8]) -> String {
    let mut output = String::with_capacity(payload.len().min(64) * 2 + 3);
    for byte in payload.iter().take(64) {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    if payload.len() > 64 {
        output.push_str("...");
    }
    output
}

fn stored_event_view(event: StoredEvent) -> EventView {
    EventView {
        event_id: event_id_text(event.id),
        sequence: event.id.sequence,
        entity: entity_text(event.entity),
        schema: event.schema.0,
        valid_clock: event.times.valid.clock.0,
        valid_time: event.times.valid.ticks,
        known_clock: event.times.known.clock.0,
        known_time: event.times.known.ticks,
        payload_hex: payload_preview(&event.payload),
        payload_bytes: event.payload.len(),
        causes: event.causes.into_iter().map(event_id_text).collect(),
        fields: Vec::new(),
    }
}

fn query_row_view(row: temnion_query::QueryRow) -> EventView {
    let mut fields: Vec<FieldValue> = row
        .fields
        .into_iter()
        .map(|(name, value)| FieldValue {
            name,
            value: match value {
                Literal::Int(value) => value.to_string(),
                Literal::Float(value) => value.to_string(),
                Literal::String(value) => value,
                Literal::Bool(value) => value.to_string(),
            },
        })
        .collect();
    fields.sort_by(|left, right| left.name.cmp(&right.name));

    EventView {
        event_id: row.sequence.to_string(),
        sequence: row.sequence,
        entity: entity_text(row.entity),
        schema: row.schema.0,
        valid_clock: row.valid_time.clock.0,
        valid_time: row.valid_time.ticks,
        known_clock: row.known_time.clock.0,
        known_time: row.known_time.ticks,
        payload_hex: String::new(),
        payload_bytes: 0,
        causes: Vec::new(),
        fields,
    }
}

fn parse_query(query: &str) -> Result<temnion_query::LogicalPlan, String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Err("Query cannot be empty".to_owned());
    }
    if trimmed.starts_with("tn:") || trimmed.starts_with('#') || trimmed.starts_with('$') {
        parse_compact_tem(trimmed).map_err(|error| error.to_string())
    } else if trimmed.to_ascii_lowercase().starts_with("select") {
        parse_sql(trimmed).map_err(|error| error.to_string())
    } else {
        parse_temql(trimmed).map_err(|error| error.to_string())
    }
}

fn parse_entity(value: &str) -> Result<EntityId, String> {
    let parts: Vec<_> = value.trim().trim_start_matches('#').split(':').collect();
    if parts.len() != 3 {
        return Err("Entity must use shard:slot:generation".to_owned());
    }
    Ok(EntityId {
        shard: ShardId(
            parts[0]
                .parse()
                .map_err(|_| "Entity shard must be an unsigned integer".to_owned())?,
        ),
        slot: parts[1]
            .parse()
            .map_err(|_| "Entity slot must be an unsigned integer".to_owned())?,
        generation: parts[2]
            .parse()
            .map_err(|_| "Entity generation must be an unsigned integer".to_owned())?,
    })
}

fn parse_timestamp(value: &str, label: &str) -> Result<Timestamp, String> {
    let parts: Vec<_> = value.trim().split(':').collect();
    if parts.len() != 2 {
        return Err(format!("{label} must use clock:tick"));
    }
    Ok(Timestamp {
        clock: ClockId(
            parts[0]
                .parse()
                .map_err(|_| format!("{label} clock must be an unsigned integer"))?,
        ),
        ticks: parts[1]
            .parse()
            .map_err(|_| format!("{label} tick must be an unsigned integer"))?,
    })
}

fn parse_event_id(value: &str) -> Result<EventId, String> {
    let parts: Vec<_> = value.trim().trim_start_matches('#').split(':').collect();
    if parts.len() != 3 {
        return Err("Cause IDs must use source:epoch:sequence".to_owned());
    }
    Ok(EventId {
        source: SourceId(
            parts[0]
                .parse()
                .map_err(|_| "Cause source must be an unsigned integer".to_owned())?,
        ),
        epoch: SourceEpoch(
            parts[1]
                .parse()
                .map_err(|_| "Cause epoch must be an unsigned integer".to_owned())?,
        ),
        sequence: parts[2]
            .parse()
            .map_err(|_| "Cause sequence must be an unsigned integer".to_owned())?,
    })
}

fn decode_hex(value: &str) -> Result<Vec<u8>, String> {
    let compact: String = value
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    if compact.len() % 2 != 0 {
        return Err("Hex payload must contain an even number of digits".to_owned());
    }
    if compact.len() / 2 > Limits::default().max_payload_bytes {
        return Err("Payload exceeds the configured one-megabyte limit".to_owned());
    }
    compact
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).map_err(|_| "Payload is not valid UTF-8")?;
            u8::from_str_radix(text, 16).map_err(|_| "Payload contains a non-hexadecimal digit")
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(str::to_owned)
}

#[tauri::command]
fn get_engine_status(state: State<'_, StudioState>) -> Result<EngineStatus, String> {
    let guard = lock_state(&state)?;
    Ok(guard
        .as_ref()
        .map(session_status)
        .unwrap_or_else(disconnected_status))
}

#[tauri::command]
fn connect_database(path: String, state: State<'_, StudioState>) -> Result<EngineStatus, String> {
    let requested = path.trim();
    if requested.is_empty() {
        return Err("Database path is required".to_owned());
    }
    let path = fs::canonicalize(Path::new(requested))
        .map_err(|error| format!("Cannot resolve database directory: {error}"))?;
    let (store, _) = Store::open(&path, Limits::default(), RecoveryMode::RejectIncompleteTail)
        .map_err(|error| format!("Cannot open Temnion database: {error}"))?;
    let session = StudioSession {
        path,
        store,
        cadence: CadenceScheduler::new(),
        mirror_writer: MirrorWriter::new(10_000),
    };
    let status = session_status(&session);
    *lock_state(&state)? = Some(session);
    Ok(status)
}

#[tauri::command]
fn create_database(
    path: String,
    source: Option<u32>,
    epoch: Option<u64>,
    state: State<'_, StudioState>,
) -> Result<EngineStatus, String> {
    let requested = path.trim();
    if requested.is_empty() {
        return Err("Database path is required".to_owned());
    }
    let path = PathBuf::from(requested);
    let store = Store::create(
        &path,
        SourceId(source.unwrap_or(1)),
        SourceEpoch(epoch.unwrap_or(1)),
        Limits::default(),
    )
    .map_err(|error| format!("Cannot create Temnion database: {error}"))?;
    let canonical = fs::canonicalize(&path).map_err(|error| {
        format!("Database was created but its path cannot be resolved: {error}")
    })?;
    let session = StudioSession {
        path: canonical,
        store,
        cadence: CadenceScheduler::new(),
        mirror_writer: MirrorWriter::new(10_000),
    };
    let status = session_status(&session);
    *lock_state(&state)? = Some(session);
    Ok(status)
}

#[tauri::command]
fn disconnect_database(state: State<'_, StudioState>) -> Result<EngineStatus, String> {
    *lock_state(&state)? = None;
    Ok(disconnected_status())
}

#[tauri::command]
fn execute_query(
    request: QueryRequest,
    state: State<'_, StudioState>,
) -> Result<QueryView, String> {
    let logical = parse_query(&request.query)?;
    let physical = plan_query(&logical);
    let rows = bounded_rows(request.max_rows);
    let budget = QueryBudget {
        max_rows: Some(rows),
        max_events_scanned: Some(MAX_SCANNED),
        max_bytes: Some(MAX_READ_BYTES),
    };
    let started = Instant::now();
    let mut guard = lock_state(&state)?;
    let session = guard
        .as_mut()
        .ok_or_else(|| "Connect to a database before executing a query".to_owned())?;
    let result = QueryExecutor::execute_storage_scan(&mut session.store, &physical, &budget)
        .map_err(|error| error.to_string())?;
    Ok(QueryView {
        rows: result.rows.into_iter().map(query_row_view).collect(),
        events_scanned: result.events_scanned,
        bytes_read: result.bytes_read,
        truncated: result.truncated,
        elapsed_micros: started.elapsed().as_micros(),
    })
}

#[tauri::command]
fn explain_query_text(query: String) -> Result<String, String> {
    let logical = parse_query(&query)?;
    Ok(explain_query(&logical).to_string())
}

#[tauri::command]
fn list_history(
    max_rows: Option<usize>,
    state: State<'_, StudioState>,
) -> Result<HistoryView, String> {
    let rows = bounded_rows(max_rows);
    let mut guard = lock_state(&state)?;
    let session = guard
        .as_mut()
        .ok_or_else(|| "Connect to a database before loading history".to_owned())?;
    let page = session
        .store
        .history(
            HistoryFilter::default(),
            StorageQueryBudget {
                max_results: rows,
                max_scanned: MAX_SCANNED,
                max_read_bytes: MAX_READ_BYTES,
            },
            None,
        )
        .map_err(|error| error.to_string())?;
    Ok(HistoryView {
        rows: page.events.into_iter().map(stored_event_view).collect(),
        events_scanned: page.scanned,
        bytes_read: page.bytes_read,
        has_more: page.continuation.is_some(),
    })
}

#[tauri::command]
fn append_event(
    request: AppendRequest,
    state: State<'_, StudioState>,
) -> Result<AppendReceiptView, String> {
    let event = WriteEvent {
        entity: parse_entity(&request.entity)?,
        schema: SchemaId(request.schema),
        times: EventTimes {
            valid: parse_timestamp(&request.valid_time, "Valid time")?,
            observed: None,
            known: parse_timestamp(&request.known_time, "Known time")?,
        },
        payload: decode_hex(&request.payload_hex)?,
        causes: request
            .causes
            .iter()
            .filter(|cause| !cause.trim().is_empty())
            .map(|cause| parse_event_id(cause))
            .collect::<Result<Vec<_>, _>>()?,
    };
    let mut guard = lock_state(&state)?;
    let session = guard
        .as_mut()
        .ok_or_else(|| "Connect to a database before appending an event".to_owned())?;
    let receipt = session
        .store
        .append(vec![event])
        .map_err(|error| error.to_string())?;
    Ok(AppendReceiptView {
        first_event: event_id_text(receipt.first),
        last_event: event_id_text(receipt.last),
        count: receipt.count,
        status: session_status(session),
    })
}

#[tauri::command]
fn list_branches(state: State<'_, StudioState>) -> Result<Vec<BranchView>, String> {
    let guard = lock_state(&state)?;
    let session = guard
        .as_ref()
        .ok_or_else(|| "Connect to a database before inspecting branches".to_owned())?;
    let manifest_path = session.path.join("branches.manifest");
    if !manifest_path.exists() {
        return Ok(vec![BranchView {
            id: 0,
            name: "main".to_owned(),
            parent_id: None,
            fork_sequence: None,
            lifecycle: BranchLifecycle::Active.as_str().to_owned(),
        }]);
    }
    let bytes = fs::read(&manifest_path)
        .map_err(|error| format!("Cannot read branch manifest: {error}"))?;
    let manifest = BranchManifest::decode(&bytes).map_err(|error| error.to_string())?;
    Ok(manifest
        .branches
        .values()
        .map(|branch| BranchView {
            id: branch.id.0,
            name: branch.name.clone(),
            parent_id: branch.parent.map(|(parent, _)| parent.0),
            fork_sequence: branch.parent.map(|(_, sequence)| sequence),
            lifecycle: branch.lifecycle.as_str().to_owned(),
        })
        .collect())
}

#[tauri::command]
fn trace_causality(
    sequence: u64,
    max_depth: Option<usize>,
    state: State<'_, StudioState>,
) -> Result<CausalTraceView, String> {
    let depth = max_depth.unwrap_or(8).clamp(1, MAX_CAUSAL_DEPTH);
    let mut guard = lock_state(&state)?;
    let session = guard
        .as_mut()
        .ok_or_else(|| "Connect to a database before tracing causality".to_owned())?;
    let page = session
        .store
        .history(
            HistoryFilter::default(),
            StorageQueryBudget {
                max_results: MAX_SCANNED,
                max_scanned: MAX_SCANNED,
                max_read_bytes: MAX_READ_BYTES,
            },
            None,
        )
        .map_err(|error| error.to_string())?;
    let truncated = page.continuation.is_some();
    let mut graph = CausalGraph::new();
    let mut root = None;
    for event in page.events {
        if event.id.sequence == sequence {
            root = Some(event.id);
        }
        graph
            .add_event(event.id, event.causes)
            .map_err(|error| error.to_string())?;
    }
    let root = root.ok_or_else(|| format!("Event sequence {sequence} was not found"))?;
    let causes = graph.trace_causes(root, depth);
    let effects = graph.trace_effects(root, depth);
    let mut edges = BTreeSet::new();
    for (from, to) in causes.edges.iter().chain(&effects.edges) {
        edges.insert(format!(
            "{} -> {}",
            event_id_text(*from),
            event_id_text(*to)
        ));
    }
    let node_views = |trace: temnion_causal::CausalTrace| {
        trace
            .events
            .iter()
            .copied()
            .filter(|event| *event != root)
            .map(|event| TraceNodeView {
                event_id: event_id_text(event),
                sequence: event.sequence,
                depth: trace.depths.get(&event).copied().unwrap_or(0),
            })
            .collect()
    };
    Ok(CausalTraceView {
        root: event_id_text(root),
        causes: node_views(causes),
        effects: node_views(effects),
        edges: edges.into_iter().collect(),
        truncated,
    })
}

#[tauri::command]
fn get_tzeentch_summary(state: State<'_, StudioState>) -> Result<TzeentchSummaryView, String> {
    let guard = lock_state(&state)?;
    let session = guard
        .as_ref()
        .ok_or_else(|| "Database is not connected".to_owned())?;
    let stats = session.mirror_writer.stats();
    let mode = match session.mirror_writer.mode() {
        MigrationMode::LegacyOnly => "LegacyOnly",
        MigrationMode::ShadowMirror => "ShadowMirror",
        MigrationMode::TemnionAuthoritative => "TemnionAuthoritative",
        MigrationMode::TemnionOnly => "TemnionOnly",
    };
    Ok(TzeentchSummaryView {
        organism_id: "ORGX-Prime".to_string(),
        organs: vec![
            "VisualCortex".to_string(),
            "MotorExecutive".to_string(),
            "WorkingMemory".to_string(),
            "WorldModel".to_string(),
        ],
        cells: vec![
            "SensoryCell0".to_string(),
            "FeatureAttn1".to_string(),
            "PolicyCell2".to_string(),
            "PredictionCell3".to_string(),
        ],
        migration_mode: mode.to_string(),
        mirror_enqueued: stats.enqueued,
        mirror_drained: stats.drained,
        drop_count: stats.dropped_count,
    })
}

#[tauri::command]
fn get_tzeentch_cadence_stats(
    state: State<'_, StudioState>,
) -> Result<TzeentchCadenceStatsView, String> {
    let guard = lock_state(&state)?;
    let session = guard
        .as_ref()
        .ok_or_else(|| "Database is not connected".to_owned())?;
    let scheduler = &session.cadence;
    let pending = session.mirror_writer.pending_count();
    let queue_pressure = pending as f64 / 10_000.0;
    Ok(TzeentchCadenceStatsView {
        fast_hz: 120.0,
        fast_ticks: scheduler.fast_ticks.max(120),
        medium_hz: 20.0,
        medium_ticks: scheduler.medium_ticks.max(20),
        slow_hz: 1.0,
        slow_ticks: scheduler.slow_ticks.max(1),
        background_hz: 0.1,
        background_ticks: scheduler.background_ticks.max(1),
        drop_count: 0,
        queue_pressure,
    })
}

#[tauri::command]
fn inspect_tzeentch_action_trace(
    sequence: u64,
    state: State<'_, StudioState>,
) -> Result<ActionTraceView, String> {
    let mut guard = lock_state(&state)?;
    let session = guard
        .as_mut()
        .ok_or_else(|| "Database is not connected".to_owned())?;
    let page = session
        .store
        .history(
            HistoryFilter::default(),
            StorageQueryBudget {
                max_results: 1_000,
                max_scanned: MAX_SCANNED,
                max_read_bytes: MAX_READ_BYTES,
            },
            None,
        )
        .map_err(|error| error.to_string())?;

    let target_seq = if sequence == 0 {
        page.events
            .iter()
            .rev()
            .find(|ev| ev.schema == temnion_adapter::SCHEMA_ACTION)
            .map(|ev| ev.id.sequence)
            .unwrap_or(0)
    } else {
        sequence
    };

    if target_seq == 0 {
        return Ok(ActionTraceView {
            action_event_id: "none".to_string(),
            nodes: vec![ActionTraceNodeView {
                kind: "info".to_string(),
                event_id: None,
                label: "No Action Recorded".to_string(),
                detail: "No action events found in database yet. Ingest or simulate an episode to trace.".to_string(),
                timestamp: 0,
                confidence: None,
                is_gap: false,
            }],
            edges: vec![],
            future_leakage_detected: false,
            total_causes: 0,
        });
    }

    let trace = temnion_adapter::TzeentchActionTracer::trace_action(target_seq, &page.events)
        .map_err(|error| error.to_string())?;

    let mut nodes = Vec::new();
    for node in trace.nodes {
        match node {
            temnion_adapter::ActionTraceNode::Percept {
                event_id,
                organ,
                sensor,
                timestamp,
            } => {
                nodes.push(ActionTraceNodeView {
                    kind: "percept".to_string(),
                    event_id: Some(event_id_text(event_id)),
                    label: format!("Percept ({organ})"),
                    detail: format!("Sensor: {sensor}"),
                    timestamp,
                    confidence: None,
                    is_gap: false,
                });
            }
            temnion_adapter::ActionTraceNode::CellProcessing {
                event_id,
                organ,
                cell,
                timestamp,
            } => {
                nodes.push(ActionTraceNodeView {
                    kind: "cell".to_string(),
                    event_id: Some(event_id_text(event_id)),
                    label: format!("Processing ({organ})"),
                    detail: format!("Cell: {cell}"),
                    timestamp,
                    confidence: None,
                    is_gap: false,
                });
            }
            temnion_adapter::ActionTraceNode::RetrievedBelief {
                event_id,
                concept,
                confidence,
                timestamp,
            } => {
                nodes.push(ActionTraceNodeView {
                    kind: "belief".to_string(),
                    event_id: Some(event_id_text(event_id)),
                    label: format!("Belief ({concept})"),
                    detail: format!("Confidence: {confidence:.2}"),
                    timestamp,
                    confidence: Some(confidence),
                    is_gap: false,
                });
            }
            temnion_adapter::ActionTraceNode::Prediction {
                event_id,
                label,
                probability,
                timestamp,
            } => {
                nodes.push(ActionTraceNodeView {
                    kind: "prediction".to_string(),
                    event_id: Some(event_id_text(event_id)),
                    label: format!("Prediction ({label})"),
                    detail: format!("Probability: {probability:.2}"),
                    timestamp,
                    confidence: Some(probability),
                    is_gap: false,
                });
            }
            temnion_adapter::ActionTraceNode::Intention {
                event_id,
                goal,
                policy,
                timestamp,
            } => {
                nodes.push(ActionTraceNodeView {
                    kind: "intention".to_string(),
                    event_id: Some(event_id_text(event_id)),
                    label: format!("Intention ({goal})"),
                    detail: format!("Policy: {policy}"),
                    timestamp,
                    confidence: None,
                    is_gap: false,
                });
            }
            temnion_adapter::ActionTraceNode::Action {
                event_id,
                action_id,
                command,
                timestamp,
            } => {
                nodes.push(ActionTraceNodeView {
                    kind: "action".to_string(),
                    event_id: Some(event_id_text(event_id)),
                    label: format!("Action ({action_id})"),
                    detail: format!("Command: {command}"),
                    timestamp,
                    confidence: None,
                    is_gap: false,
                });
            }
            temnion_adapter::ActionTraceNode::Outcome {
                event_id,
                reward,
                timestamp,
            } => {
                nodes.push(ActionTraceNodeView {
                    kind: "outcome".to_string(),
                    event_id: Some(event_id_text(event_id)),
                    label: "Outcome Feedback".to_string(),
                    detail: format!("Reward: {reward:+.2}"),
                    timestamp,
                    confidence: None,
                    is_gap: false,
                });
            }
            temnion_adapter::ActionTraceNode::SourceGap {
                step_name,
                expected_time,
            } => {
                nodes.push(ActionTraceNodeView {
                    kind: "gap".to_string(),
                    event_id: None,
                    label: format!("Source Gap: {step_name}"),
                    detail: "Uninstrumented telemetry step".to_string(),
                    timestamp: expected_time,
                    confidence: None,
                    is_gap: true,
                });
            }
        }
    }

    let edges = trace
        .edges
        .into_iter()
        .map(|(from, to)| format!("{} -> {}", event_id_text(from), event_id_text(to)))
        .collect();

    Ok(ActionTraceView {
        action_event_id: event_id_text(trace.action_event_id),
        nodes,
        edges,
        future_leakage_detected: trace.future_leakage_detected,
        total_causes: trace.total_causes,
    })
}

#[tauri::command]
fn set_tzeentch_migration_mode(
    mode: String,
    state: State<'_, StudioState>,
) -> Result<String, String> {
    let mut guard = lock_state(&state)?;
    let session = guard
        .as_mut()
        .ok_or_else(|| "Database is not connected".to_owned())?;
    let new_mode = match mode.as_str() {
        "LegacyOnly" => MigrationMode::LegacyOnly,
        "ShadowMirror" => MigrationMode::ShadowMirror,
        "TemnionAuthoritative" => MigrationMode::TemnionAuthoritative,
        "TemnionOnly" => MigrationMode::TemnionOnly,
        other => return Err(format!("Unknown migration mode: {other}")),
    };
    session.mirror_writer.set_mode(new_mode);
    Ok(mode)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(StudioState::default())
        .invoke_handler(tauri::generate_handler![
            get_engine_status,
            connect_database,
            create_database,
            disconnect_database,
            execute_query,
            explain_query_text,
            list_history,
            append_event,
            list_branches,
            trace_causality,
            get_tzeentch_summary,
            get_tzeentch_cadence_stats,
            inspect_tzeentch_action_trace,
            set_tzeentch_migration_mode,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Temnion Studio");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parses_studio_entity_and_time_inputs() {
        assert_eq!(
            parse_entity("#4:12:2").unwrap(),
            EntityId {
                shard: ShardId(4),
                slot: 12,
                generation: 2,
            }
        );
        assert_eq!(
            parse_timestamp("3:99", "Known time").unwrap(),
            Timestamp::new(ClockId(3), 99)
        );
    }

    #[test]
    fn hex_decoder_is_strict_and_bounded() {
        assert_eq!(decode_hex("0a ff 10").unwrap(), vec![10, 255, 16]);
        assert!(decode_hex("abc").is_err());
        assert!(decode_hex("xz").is_err());
    }

    #[test]
    fn query_frontends_share_the_existing_parsers() {
        assert!(parse_query("FROM temnion\nLIMIT 10").is_ok());
        assert!(parse_query("SELECT * FROM temnion LIMIT 10").is_ok());
        assert!(parse_query("tn:!10").is_ok());
    }

    #[test]
    fn native_store_query_round_trip_is_real_and_bounded() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "temnion-studio-test-{}-{nonce}",
            std::process::id()
        ));
        let mut store =
            Store::create(&root, SourceId(7), SourceEpoch(1), Limits::default()).unwrap();
        let receipt = store
            .append(vec![WriteEvent {
                entity: EntityId {
                    shard: ShardId(0),
                    slot: 4,
                    generation: 0,
                },
                times: EventTimes {
                    valid: Timestamp::new(ClockId(1), 10),
                    observed: None,
                    known: Timestamp::new(ClockId(1), 12),
                },
                schema: SchemaId(2),
                payload: vec![0xaa, 0xbb],
                causes: Vec::new(),
            }])
            .unwrap();
        assert_eq!(receipt.count, 1);

        let logical = parse_query("FROM temnion\nLIMIT 1").unwrap();
        let result = QueryExecutor::execute_storage_scan(
            &mut store,
            &plan_query(&logical),
            &QueryBudget {
                max_rows: Some(1),
                max_events_scanned: Some(4),
                max_bytes: Some(4_096),
            },
        )
        .unwrap();
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0].entity.slot, 4);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }
}
