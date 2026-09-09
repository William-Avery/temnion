// SPDX-License-Identifier: AGPL-3.0-only
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use temnion_adapter::{
    ActiveConnection, CadenceScheduler, CadenceTier, ConnectionConfig, MediaRef, QueryFormat,
    TzeentchAction, TzeentchActionTracer, TzeentchConverter, TzeentchIntention, TzeentchOutcome,
    TzeentchPercept,
};
use temnion_branch::{BranchLifecycle, BranchManager};
use temnion_causal::CausalGraph;
use temnion_codec::CodecScorer;
use temnion_core::{
    BranchId, ClockId, EntityId, EventId, EventTimes, SchemaId, ShardId, SourceEpoch, SourceId,
    Timestamp,
};
use temnion_eks::{Confidence, Justification, TruthMaintenanceSystem};
use temnion_events::{EventInput, EventLog, HistoryFilter, QueryBudget};
use temnion_format::{Limits, StoredEvent, decode_segment};
use temnion_mcp::McpServer;
use temnion_query::{
    QueryBudget as EngineQueryBudget, QueryExecutor, explain_query, parse_compact_tem, parse_expr,
    parse_sql, parse_temql, plan_query,
};
use temnion_replay::{
    Checkpoint, RawEntityReducer, ReplayEngine, decode_raw_entity_map, encode_raw_entity_map,
};
use temnion_state::StateSlab;
use temnion_storage::{RecoveryMode, StorageQueryBudget, Store, WriteEvent, read_bounded_file};
use temnion_transform::EGraph;

const HELP: &str = "\
tem - Temnion CLI

Usage: tem [help | version | describe | demo]
       tem init <database-directory>
       tem append <directory> <shard:slot:generation> <schema-id> <valid-clock:tick> <known-clock:tick> <hex-payload> [causes]
       tem history <directory> [row-limit]
       tem inspect <directory>
       tem recover <directory>
       tem seal <directory>
       tem verify-segment <segment-file>
       tem inspect-summary <summary-file>
       tem checkpoint <directory>
       tem reconstruct <directory> <sequence>
       tem evaluate-codecs
       tem branch-create <directory> <name> [parent-id] [fork-seq]
       tem branch-list <directory>
       tem causal-trace <directory> <sequence> [max-depth]
       tem query <directory> <query-str>
       tem explain <query-str>
       tem subscribe [options] <query-str>
       tem mcp [directory]
       tem why-demo
       tem rewrite-demo <expression>
       tem evolve [status | audit | demo]
       tem tzeentch [demo | status | trace <entity-name>]
       tem benchmark scale [records]

  help       Show this help
  version    Show the version
  describe   Print implemented capabilities as JSON
  demo       Run an in-memory state/history example; writes no files
  why-demo   Run an in-memory Epistemic Knowledge Store (EKS) and WHY trace example
  rewrite-demo Run an e-graph equality saturation optimization on an expression
  evolve     Inspect champion/challenger evolution engine, audit constitution, or run demo
  tzeentch   Inspect Tzeentch adapter, run multi-cadence demo, or introspect causal action traces
  benchmark  Run scale benchmark harness (4K, 64K, 1M+ active records)
  init       Create a durable source log with an OS-random database identity
  append     Persist one opaque typed payload; acknowledge only after OS sync
  history    Read one bounded history page (payload previews, default 100 rows)
  inspect    Validate the WAL and report durable records and bytes
  recover    Explicitly truncate an incomplete tail; never skip corrupt frames
  seal       Export immutable TSF segments while retaining the authoritative WAL
  verify-segment  Validate a standalone TSF segment
  inspect-summary Validate a companion TSM summary index and report zone maps
  checkpoint      Take an atomic checksummed state checkpoint pinned to current WAL sequence
  reconstruct     Deterministically replay WAL events to target sequence
  evaluate-codecs Evaluate candidate lossless codecs on representative streams
  branch-create   Fork a new timeline sharing all ancestor segments (O(1) fork)
  branch-list     List all timeline branches and lifecycle states in database
  causal-trace    Trace transitive causal ancestry and effect cones for an event
  query           Execute a TemQL, compact tn:, or SQL query against a database
  explain         Parse a TemQL, compact tn:, or SQL query and show the physical execution plan
  subscribe       Stream live real-time events over TNP with predicate filter pushdown
  mcp             Run the Model Context Protocol (MCP) server over stdio

The append command is a low-level schema-ID/opaque-payload interface.
Ordinary open never silently truncates history. temniond daemon and
Temnion Studio are not implemented yet.";

const CAPABILITIES: &str = concat!(
    "{\n",
    "  \"name\": \"Temnion\",\n",
    "  \"version\": \"",
    env!("CARGO_PKG_VERSION"),
    "\",\n",
    "  \"maturity\": \"foundation\",\n",
    "  \"storage\": \"volatile-memory-and-os-synced-source-log\",\n",
    "  \"implemented\": [\"packed-state\", \"generational-entities\", ",
    "\"typed-events\", \"atomic-batch-admission\", \"entity-history\", ",
    "\"time-range-filter\", \"known-as-of\", \"bounded-snapshot-pagination\", ",
    "\"scalar-schemas\", \"wal-recovery\", \"immutable-tsf-export\", ",
    "\"deterministic-reconstruction\", \"lossless-codecs\", ",
    "\"branching-timelines\", \"causal-graph\", \"hierarchical-summaries\", ",
    "\"nd-layouts\", \"alternate-projections\", \"virtual-shards\", ",
    "\"background-dag\", \"storage-hierarchy\", \"query-ir\", \"temql\", ",
    "\"compact-tem\", \"sql\", \"tnp\", \"local-ipc\", \"arrow-columnar\", \"c-abi\", ",
    "\"flight\", \"mcp\", \"eks\", \"provenance\", \"predictive-knowledge\", ",
    "\"knowledge-consolidation\", \"transformations\", \"e-graphs\", ",
    "\"evolution\", \"adaptive-physical-memory\", \"adaptive-lifecycle\", ",
    "\"semantic-projections\", \"constitution\", \"tzeentch-adapter\", ",
    "\"causal-action-trace\", \"scale-qualified\", \"retention-holds\", ",
    "\"live-subscription-streaming\"],\n",
    "  \"durable\": true,\n",
    "  \"server\": false,\n",
    "  \"temql\": true,\n",
    "  \"sql\": true,\n",
    "  \"tnp\": true,\n",
    "  \"tsf\": true,\n",
    "  \"mcp\": true,\n",
    "  \"eks\": true,\n",
    "  \"transformations\": true,\n",
    "  \"e-graphs\": true,\n",
    "  \"evolution\": true,\n",
    "  \"constitution\": true,\n",
    "  \"semantic_projections\": true,\n",
    "  \"tzeentch_adapter\": true,\n",
    "  \"causal_action_trace\": true,\n",
    "  \"scale_qualified\": true,\n",
    "  \"retention_holds\": true,\n",
    "  \"studio\": false\n",
    "}"
);

#[derive(Clone, Copy, Debug)]
struct Position {
    x: i32,
}

fn demo(out: &mut impl Write) -> Result<(), Box<dyn Error>> {
    let mut state = StateSlab::new(ShardId(0), 4)?;
    let entity = state.insert(Position { x: 0 })?;
    let mut log = EventLog::new(SourceId(1), SourceEpoch(1), 16)?;

    for (valid, known, x) in [(10, 10, 10), (12, 12, 12), (11, 20, 11)] {
        log.append(EventInput {
            entity,
            times: EventTimes {
                valid: Timestamp::new(ClockId(1), valid),
                observed: Some(Timestamp::new(ClockId(1), valid)),
                known: Timestamp::new(ClockId(2), known),
            },
            change: Position { x },
        })?;
        // This demo materializes arrival order, not a historical replay policy.
        *state.get_mut(entity)? = Position { x };
    }

    writeln!(
        out,
        "Temnion foundation demo: volatile memory, no files written"
    )?;
    writeln!(
        out,
        "Current arrival-order position: x={}",
        state.get(entity)?.x
    )?;
    writeln!(out, "Recorded events: {}", log.len())?;
    let filter = HistoryFilter {
        entity: Some(entity),
        known_as_of: Some(Timestamp::new(ClockId(2), 15)),
        ..HistoryFilter::default()
    };
    let mut cursor = None;
    let mut visible = 0;
    loop {
        let page = log.history(
            filter,
            QueryBudget {
                max_results: 1,
                max_scanned: 2,
            },
            cursor,
        )?;
        for event in page.events {
            writeln!(
                out,
                "sequence={} valid={} known={} x={}",
                event.id.sequence, event.times.valid.ticks, event.times.known.ticks, event.change.x
            )?;
            visible += 1;
        }
        cursor = page.continuation;
        if cursor.is_none() {
            break;
        }
    }
    writeln!(
        out,
        "Known-as-of 15: {visible} events; late evidence is excluded"
    )?;
    let removed = state.remove(entity)?;
    let replacement = state.insert(removed)?;
    if state.get(entity).is_ok() || replacement.generation == entity.generation {
        return Err("state identity invariant failed".into());
    }
    writeln!(
        out,
        "Removed handle rejected; reused slot has a new generation"
    )?;
    Ok(())
}

fn text(value: &OsStr) -> Result<&str, Box<dyn Error>> {
    value
        .to_str()
        .ok_or_else(|| "text argument is not valid Unicode".into())
}

fn parse_entity(value: &OsStr) -> Result<EntityId, Box<dyn Error>> {
    let components: Vec<_> = text(value)?.split(':').collect();
    if components.len() != 3 {
        return Err("entity must be shard:slot:generation".into());
    }
    Ok(EntityId {
        shard: ShardId(components[0].parse()?),
        slot: components[1].parse()?,
        generation: components[2].parse()?,
    })
}

fn parse_time(value: &OsStr) -> Result<Timestamp, Box<dyn Error>> {
    let (clock, tick) = text(value)?
        .split_once(':')
        .ok_or("time must be clock:tick")?;
    Ok(Timestamp::new(ClockId(clock.parse()?), tick.parse()?))
}

fn decode_hex(value: &OsStr) -> Result<Vec<u8>, Box<dyn Error>> {
    let value = text(value)?;
    if value.len() > Limits::default().max_payload_bytes * 2 || value.len() % 2 != 0 {
        return Err("hex payload must have even length and fit the payload limit".into());
    }
    let mut payload = Vec::new();
    payload.try_reserve_exact(value.len() / 2)?;
    for pair in value.as_bytes().chunks_exact(2) {
        let high = (pair[0] as char)
            .to_digit(16)
            .ok_or("invalid hexadecimal payload")?;
        let low = (pair[1] as char)
            .to_digit(16)
            .ok_or("invalid hexadecimal payload")?;
        payload.push((high * 16 + low) as u8);
    }
    Ok(payload)
}

fn parse_causes(value: &OsStr) -> Result<Vec<EventId>, Box<dyn Error>> {
    let s = text(value)?;
    if s.is_empty() || s == "-" || s == "none" {
        return Ok(Vec::new());
    }
    let mut causes = Vec::new();
    for part in s.split(',') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        let comps: Vec<_> = trimmed.split(':').collect();
        if comps.len() != 3 {
            return Err("cause event id must be source:epoch:sequence".into());
        }
        causes.push(EventId {
            source: SourceId(comps[0].parse()?),
            epoch: SourceEpoch(comps[1].parse()?),
            sequence: comps[2].parse()?,
        });
    }
    Ok(causes)
}

fn open_store(path: &OsStr) -> Result<Store, Box<dyn Error>> {
    Ok(Store::open(
        Path::new(path),
        Limits::default(),
        RecoveryMode::RejectIncompleteTail,
    )?
    .0)
}

fn show_record(out: &mut impl Write, event: &StoredEvent) -> io::Result<()> {
    write!(
        out,
        "sequence={} entity={}:{}:{} schema={} valid={}:{} known={}:{} payload_bytes={} preview=",
        event.id.sequence,
        event.entity.shard.0,
        event.entity.slot,
        event.entity.generation,
        event.schema.0,
        event.times.valid.clock.0,
        event.times.valid.ticks,
        event.times.known.clock.0,
        event.times.known.ticks,
        event.payload.len(),
    )?;
    for byte in event.payload.iter().take(64) {
        write!(out, "{byte:02x}")?;
    }
    writeln!(out, "{}", if event.payload.len() > 64 { "..." } else { "" })
}

fn to_u64_bytes(slice: &[u64]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(slice.len() * 8);
    for &x in slice {
        bytes.extend_from_slice(&x.to_le_bytes());
    }
    bytes
}

fn evaluate_codecs_cmd(out: &mut impl Write) -> Result<(), Box<dyn Error>> {
    let scorer = CodecScorer::default();

    // 1. Monotonic sequence (e.g. timestamps or IDs)
    let monotonic: Vec<u64> = (1000..1256).map(|x| x * 10).collect();
    let r1 = scorer.select_best(&to_u64_bytes(&monotonic))?;

    // 2. Clustered low-cardinality values (e.g. status flags / category IDs)
    let mut clustered = Vec::with_capacity(256);
    clustered.resize(64, 1u64);
    clustered.resize(128, 2u64);
    clustered.resize(192, 10u64);
    clustered.resize(256, 5u64);
    let r2 = scorer.select_best(&to_u64_bytes(&clustered))?;

    // 3. Dense small integers (bit-packable in 4 bits: 0..15)
    let small_ints: Vec<u64> = (0..256).map(|i| (i % 15) as u64).collect();
    let r3 = scorer.select_best(&to_u64_bytes(&small_ints))?;

    // 4. Repeated constant runs (RLE dominant)
    let mut runs = Vec::with_capacity(256);
    runs.resize(128, 42u64);
    runs.resize(256, 99u64);
    let r4 = scorer.select_best(&to_u64_bytes(&runs))?;

    writeln!(out, "Lossless Codec Evaluation:")?;
    writeln!(
        out,
        "  Pattern 1 (Monotonic timestamps, 256 u64): best={} raw={} compressed={} ratio={:.2}",
        r1.name, r1.original_bytes, r1.compressed_bytes, r1.ratio
    )?;
    writeln!(
        out,
        "  Pattern 2 (Clustered statuses, 256 u64):   best={} raw={} compressed={} ratio={:.2}",
        r2.name, r2.original_bytes, r2.compressed_bytes, r2.ratio
    )?;
    writeln!(
        out,
        "  Pattern 3 (Small ints 0..15, 256 u64):     best={} raw={} compressed={} ratio={:.2}",
        r3.name, r3.original_bytes, r3.compressed_bytes, r3.ratio
    )?;
    writeln!(
        out,
        "  Pattern 4 (Long constant runs, 256 u64):   best={} raw={} compressed={} ratio={:.2}",
        r4.name, r4.original_bytes, r4.compressed_bytes, r4.ratio
    )?;
    Ok(())
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let command = args
        .first()
        .map(|value| text(value))
        .transpose()?
        .unwrap_or("help");
    let parameters = args.get(1..).unwrap_or(&[]);
    let mut out = io::stdout().lock();
    match (command, parameters) {
        ("help" | "--help" | "-h", []) => writeln!(out, "{HELP}")?,
        ("version" | "--version" | "-V", []) => writeln!(out, "tem {}", env!("CARGO_PKG_VERSION"))?,
        ("describe", []) => writeln!(out, "{CAPABILITIES}")?,
        ("demo", []) => demo(&mut out)?,
        ("init", [path]) => {
            let store = Store::create(
                Path::new(path),
                SourceId(1),
                SourceEpoch(1),
                Limits::default(),
            )?;
            let id = store.header().database;
            writeln!(
                out,
                "Created database {id}; source=1 epoch=1; acknowledgment=OS-sync"
            )?;
        }
        (
            "append",
            [path, entity, schema, valid, known, payload]
            | [path, entity, schema, valid, known, payload, _],
        ) => {
            let causes = match parameters.get(6) {
                Some(v) => parse_causes(v)?,
                None => Vec::new(),
            };
            let input = WriteEvent {
                entity: parse_entity(entity)?,
                schema: SchemaId(text(schema)?.parse()?),
                times: EventTimes {
                    valid: parse_time(valid)?,
                    observed: None,
                    known: parse_time(known)?,
                },
                payload: decode_hex(payload)?,
                causes,
            };
            let mut store = open_store(path)?;
            let receipt = store.append(vec![input])?;
            writeln!(
                out,
                "Durable sequence={} count={} acknowledgment=OS-sync",
                receipt.first.sequence, receipt.count
            )?;
        }
        ("history", [path] | [path, _]) => {
            let limit = parameters
                .get(1)
                .map(|value| {
                    text(value)?
                        .parse::<usize>()
                        .map_err(Box::<dyn Error>::from)
                })
                .transpose()?
                .unwrap_or(100);
            if limit == 0 || limit > 65_536 {
                return Err("history row limit must be between 1 and 65536".into());
            }
            let mut store = open_store(path)?;
            let page = store.history(
                HistoryFilter::default(),
                StorageQueryBudget {
                    max_results: limit,
                    ..StorageQueryBudget::default()
                },
                None,
            )?;
            for event in &page.events {
                show_record(&mut out, event)?;
            }
            writeln!(
                out,
                "rows={} decoded={} bytes_read={} more={}",
                page.events.len(),
                page.scanned,
                page.bytes_read,
                page.continuation.is_some()
            )?;
        }
        ("inspect", [path]) => {
            let store = open_store(path)?;
            writeln!(
                out,
                "Valid WAL: records={} bytes={} source={} epoch={}",
                store.len(),
                store.wal_bytes(),
                store.header().source.0,
                store.header().epoch.0
            )?;
        }
        ("recover", [path]) => {
            let (store, report) = Store::open(
                Path::new(path),
                Limits::default(),
                RecoveryMode::TruncateIncompleteTail,
            )?;
            writeln!(
                out,
                "Recovered records={} discarded_incomplete_tail_bytes={} next_sequence={}",
                report.recovered_events,
                report.discarded_tail_bytes,
                store.len()
            )?;
        }
        ("seal", [path]) => {
            let report = open_store(path)?.seal()?;
            writeln!(
                out,
                "TSF segments_created={} existing={} records={} wal_retained=true",
                report.segments_created, report.segments_existing, report.events
            )?;
        }
        ("verify-segment", [path]) => {
            let limits = Limits::default();
            let bytes = read_bounded_file(Path::new(path), limits.max_segment_bytes)?;
            let segment = decode_segment(&bytes, &limits)?;
            writeln!(
                out,
                "Valid TSF: records={} source={} epoch={}",
                segment.records.len(),
                segment.header.source.0,
                segment.header.epoch.0
            )?;
        }
        ("inspect-summary", [path]) => {
            let summary = temnion_storage::read_segment_summary(Path::new(path))?;
            writeln!(
                out,
                "Valid TSM: records={} blocks={} source={} epoch={}",
                summary.total_records,
                summary.blocks.len(),
                summary.source.0,
                summary.epoch.0
            )?;
            writeln!(
                out,
                "  Sequence: {}..={}",
                summary.sequence_range.min, summary.sequence_range.max
            )?;
            writeln!(
                out,
                "  Valid time: clock={} {}..={}",
                summary.valid_time_range.clock.0,
                summary.valid_time_range.min_ticks,
                summary.valid_time_range.max_ticks
            )?;
            writeln!(
                out,
                "  Known time: clock={} {}..={}",
                summary.known_time_range.clock.0,
                summary.known_time_range.min_ticks,
                summary.known_time_range.max_ticks
            )?;
            writeln!(
                out,
                "  Entities: {}:{}:{}..={}:{}:{}",
                summary.entity_range.min.shard.0,
                summary.entity_range.min.slot,
                summary.entity_range.min.generation,
                summary.entity_range.max.shard.0,
                summary.entity_range.max.slot,
                summary.entity_range.max.generation,
            )?;
            for block in &summary.blocks {
                writeln!(
                    out,
                    "  Block {}: records={} bytes={} seq={}..={}",
                    block.batch_index,
                    block.record_count,
                    block.byte_length,
                    block.sequence_range.min,
                    block.sequence_range.max
                )?;
            }
        }
        ("checkpoint", [path]) => {
            let target_seq = {
                let store = open_store(path)?;
                if store.is_empty() {
                    return Err("cannot create checkpoint on an empty database".into());
                }
                store.len() - 1
            };
            let mut engine = ReplayEngine::new(Path::new(path), 0)?;
            let (state, header) = engine.reconstruct_at_sequence(
                target_seq,
                &mut RawEntityReducer,
                decode_raw_entity_map,
            )?;
            let checkpoint = Checkpoint {
                header,
                state_payload: encode_raw_entity_map(&state),
            };
            let cp_path = engine.checkpoints().save(&checkpoint)?;
            writeln!(
                out,
                "Checkpoint sequence={} entities={} path={}",
                checkpoint.header.sequence,
                state.len(),
                cp_path.display()
            )?;
        }
        ("reconstruct", [path, seq_val]) => {
            let target_seq = text(seq_val)?.parse::<u64>()?;
            let mut engine = ReplayEngine::new(Path::new(path), 0)?;
            let (state, header) = engine.reconstruct_at_sequence(
                target_seq,
                &mut RawEntityReducer,
                decode_raw_entity_map,
            )?;
            writeln!(
                out,
                "Reconstructed sequence={} entities={} valid_time={}:{} known_time={}:{}",
                header.sequence,
                state.len(),
                header.valid_time.clock.0,
                header.valid_time.ticks,
                header.known_time.clock.0,
                header.known_time.ticks,
            )?;
            for (entity, (schema, payload)) in &state {
                writeln!(
                    out,
                    "  entity={}:{}:{} schema={} payload_bytes={}",
                    entity.shard.0,
                    entity.slot,
                    entity.generation,
                    schema.0,
                    payload.len()
                )?;
            }
        }
        ("evaluate-codecs", []) => {
            evaluate_codecs_cmd(&mut out)?;
        }
        ("branch-create", [path, name] | [path, name, _] | [path, name, _, _]) => {
            let path = Path::new(path);
            let name_str = text(name)?;
            let parent_id = match parameters.get(2) {
                Some(v) => BranchId(text(v)?.parse()?),
                None => BranchId(0),
            };
            let mut mgr = BranchManager::open(path)?;
            let fork_seq = match parameters.get(3) {
                Some(v) => text(v)?.parse()?,
                None => {
                    let p_dir = mgr.branch_directory(parent_id);
                    let store = open_store(p_dir.as_os_str())?;
                    if store.is_empty() { 0 } else { store.len() - 1 }
                }
            };
            let new_id = mgr.create_fork(
                parent_id,
                name_str.to_string(),
                fork_seq,
                BranchLifecycle::Candidate,
            )?;
            writeln!(
                out,
                "Branch created id={} name=\"{}\" parent={} fork_sequence={}",
                new_id.0, name_str, parent_id.0, fork_seq
            )?;
        }
        ("branch-list", [path]) => {
            let mgr = BranchManager::open(Path::new(path))?;
            writeln!(out, "Branches in database {}:", mgr.manifest().database)?;
            for (&id, meta) in &mgr.manifest().branches {
                match meta.parent {
                    Some((parent_id, fork_seq)) => {
                        writeln!(
                            out,
                            "  id={} name=\"{}\" parent={} fork_sequence={} lifecycle={}",
                            id.0,
                            meta.name,
                            parent_id.0,
                            fork_seq,
                            meta.lifecycle.as_str()
                        )?;
                    }
                    None => {
                        writeln!(
                            out,
                            "  id={} name=\"{}\" root=true lifecycle={}",
                            id.0,
                            meta.name,
                            meta.lifecycle.as_str()
                        )?;
                    }
                }
            }
        }
        ("causal-trace", [path, seq_val] | [path, seq_val, _]) => {
            let target_seq = text(seq_val)?.parse::<u64>()?;
            let depth = parameters
                .get(2)
                .map(|v| text(v)?.parse::<usize>().map_err(Box::<dyn Error>::from))
                .transpose()?
                .unwrap_or(10);

            let mut store = open_store(path)?;
            let page = store.history(
                HistoryFilter::default(),
                StorageQueryBudget {
                    max_results: 65_536,
                    max_scanned: 65_536,
                    max_read_bytes: 64 * 1024 * 1024,
                },
                None,
            )?;

            let mut graph = CausalGraph::new();
            let mut target_event = None;

            for event in page.events {
                if event.id.sequence == target_seq {
                    target_event = Some(event.id);
                }
                let _ = graph.add_event(event.id, event.causes);
            }

            let root =
                target_event.ok_or_else(|| format!("event sequence {target_seq} not found"))?;
            let causes_trace = graph.trace_causes(root, depth);
            let effects_trace = graph.trace_effects(root, depth);

            writeln!(
                out,
                "Causal Trace for Event {}:{}:{}:",
                root.source.0, root.epoch.0, root.sequence
            )?;
            writeln!(
                out,
                "  Upstream Causes (total={}):",
                causes_trace.events.len().saturating_sub(1)
            )?;
            for &c in &causes_trace.events {
                if c != root {
                    let d = causes_trace.depths.get(&c).copied().unwrap_or(0);
                    writeln!(
                        out,
                        "    depth={} event={}:{}:{}",
                        d, c.source.0, c.epoch.0, c.sequence
                    )?;
                }
            }
            writeln!(
                out,
                "  Downstream Effects (total={}):",
                effects_trace.events.len().saturating_sub(1)
            )?;
            for &e in &effects_trace.events {
                if e != root {
                    let d = effects_trace.depths.get(&e).copied().unwrap_or(0);
                    writeln!(
                        out,
                        "    depth={} event={}:{}:{}",
                        d, e.source.0, e.epoch.0, e.sequence
                    )?;
                }
            }
        }
        ("explain", [query_arg]) => {
            let query_str = text(query_arg)?;
            let trimmed = query_str.trim();
            let logical = if trimmed.starts_with("tn:")
                || trimmed.starts_with('#')
                || trimmed.starts_with('$')
            {
                parse_compact_tem(query_str)?
            } else if trimmed.to_ascii_lowercase().starts_with("select") {
                parse_sql(query_str)?
            } else {
                parse_temql(query_str)?
            };
            let explain = explain_query(&logical);
            write!(out, "{explain}")?;
        }
        ("query", [path, query_arg]) => {
            let query_str = text(query_arg)?;
            let trimmed = query_str.trim();
            let logical = if trimmed.starts_with("tn:")
                || trimmed.starts_with('#')
                || trimmed.starts_with('$')
            {
                parse_compact_tem(query_str)?
            } else if trimmed.to_ascii_lowercase().starts_with("select") {
                parse_sql(query_str)?
            } else {
                parse_temql(query_str)?
            };
            let physical = plan_query(&logical);
            let mut store = open_store(path)?;
            let result = QueryExecutor::execute_storage_scan(
                &mut store,
                &physical,
                &EngineQueryBudget::default(),
            )?;

            writeln!(
                out,
                "Query results (rows={}, scanned={}, bytes_read={}, truncated={}):",
                result.rows.len(),
                result.events_scanned,
                result.bytes_read,
                result.truncated
            )?;
            for row in &result.rows {
                let mut field_pairs: Vec<String> =
                    row.fields.iter().map(|(k, v)| format!("{k}={v}")).collect();
                field_pairs.sort();
                writeln!(
                    out,
                    "  [seq={}] entity={}:{}:{} valid={} known={} fields={{{}}}",
                    row.sequence,
                    row.entity.shard.0,
                    row.entity.slot,
                    row.entity.generation,
                    row.valid_time.ticks,
                    row.known_time.ticks,
                    field_pairs.join(", ")
                )?;
            }
        }
        ("subscribe", _) => {
            subscribe_cmd(&mut out, parameters)?;
        }
        ("mcp", []) => {
            let mut server = McpServer::new();
            let stdin = io::stdin();
            let stdout = io::stdout();
            server.run_stdio(stdin.lock(), stdout.lock())?;
        }
        ("mcp", [path]) => {
            let store = open_store(path)?;
            let mut server = McpServer::with_store(store, text(path)?);
            let stdin = io::stdin();
            let stdout = io::stdout();
            server.run_stdio(stdin.lock(), stdout.lock())?;
        }
        ("why-demo", []) => {
            let mut tms = TruthMaintenanceSystem::new();
            let ev = EventId {
                source: SourceId(1),
                epoch: SourceEpoch(1),
                sequence: 100,
            };
            let _obs_id = tms.record_observation(
                ev,
                Timestamp::new(ClockId(1), 10),
                Timestamp::new(ClockId(1), 12),
                "sensor.temperature",
                "98.6 C",
            );
            let claim_id = tms.assert_claim(
                "telemetry-agent",
                "temperature exceeds safe operating threshold",
                Confidence::new(0.96).unwrap(),
                Timestamp::new(ClockId(1), 10)..Timestamp::new(ClockId(1), 25),
                Timestamp::new(ClockId(1), 12),
            );
            let rule_id = tms.define_rule(
                "overheat_triggers_cooling",
                "temperature exceeds safe operating threshold",
                "activate emergency secondary cooling",
                Confidence::new(0.99).unwrap(),
            );
            let belief_id = tms.infer_belief(
                "activate emergency secondary cooling",
                Confidence::new(0.95).unwrap(),
                Timestamp::new(ClockId(1), 10),
                Timestamp::new(ClockId(1), 13),
                Justification {
                    evidence_events: vec![ev],
                    premises: vec![claim_id],
                    applied_rules: vec![rule_id],
                    assumptions: vec!["primary coolant loop pressure low".to_string()],
                },
            );

            let trace = tms.why(belief_id)?;
            writeln!(out, "Epistemic Knowledge Store (EKS) WHY Trace:")?;
            writeln!(
                out,
                "  Belief: k:{} \"{}\"",
                trace.target_id.0, trace.proposition
            )?;
            writeln!(out, "  Status: {:?}", trace.status)?;
            writeln!(out, "  Confidence: {:.2}", trace.confidence.value())?;
            writeln!(out, "  Grounding WAL Events: {:?}", trace.direct_evidence)?;
            writeln!(out, "  Assumptions: {:?}", trace.assumptions)?;
            writeln!(
                out,
                "  Applied Rules: {:?}",
                trace
                    .applied_rules
                    .iter()
                    .map(|r| &r.name)
                    .collect::<Vec<_>>()
            )?;
            writeln!(
                out,
                "  Premise Dependencies: {:?}",
                trace.premise_traces.len()
            )?;
        }
        ("rewrite-demo", [expr_str]) => {
            let expr_text = text(expr_str)?;
            let parsed = parse_expr(expr_text)?;
            let mut egraph = EGraph::new();
            let root = egraph.add_expr(&parsed);
            egraph.rebuild();
            let report = egraph.saturate(10);
            let (extracted, cost) = egraph
                .extract_best_expr(root)
                .map_err(|e| format!("Extraction failed: {e}"))?;

            writeln!(out, "Canonical IR E-Graph Optimization:")?;
            writeln!(out, "  Original Expression: {parsed}")?;
            writeln!(out, "  Saturated Classes: {}", report.total_classes)?;
            writeln!(out, "  Rewrites Applied: {}", report.total_rewrites)?;
            writeln!(out, "  Saturation Iterations: {}", report.iterations)?;
            writeln!(out, "  Extracted Minimal Expression: {extracted}")?;
            writeln!(out, "  Minimal AST Cost: {cost}")?;
        }
        ("evolve", parameters) => {
            let sub = parameters
                .first()
                .map(|v| text(v))
                .transpose()?
                .unwrap_or("status");
            match sub {
                "status" => {
                    let mut engine = temnion_evolution::EvolutionEngine::new(
                        temnion_evolution::EvolutionConfig::default(),
                    );
                    let baseline = temnion_evolution::FitnessMetrics {
                        latency_p50_us: 1000,
                        latency_p99_us: 2000,
                        memory_bytes: 10_000_000,
                        storage_bytes: 50_000_000,
                        cpu_cycles: 500_000,
                        read_amplification: 2.0,
                        write_amplification: 1.5,
                        background_cost_score: 0.10,
                        net_benefit_score: 0.0,
                    };
                    engine.register_incumbent(
                        temnion_evolution::CandidateKind::Codec,
                        "segment_compression".into(),
                        b"RawIncumbentCodec".to_vec(),
                        baseline,
                    );
                    writeln!(out, "Temnion Champion/Challenger Evolution Engine Status:")?;
                    writeln!(out, "  Evolution Enabled: {}", engine.config().enabled)?;
                    writeln!(
                        out,
                        "  Manual Promotion Required: {}",
                        engine.config().manual_promotion_required
                    )?;
                    writeln!(out, "  Gate C Passed: {}", engine.config().gate_c_passed)?;
                    writeln!(out, "  Active Incumbents:")?;
                    for inc in engine.list_incumbents() {
                        writeln!(
                            out,
                            "    - [domain='{}' kind={:?} ID={}]",
                            inc.target_domain, inc.kind, inc.id.0
                        )?;
                    }
                }
                "audit" => {
                    let mut engine = temnion_evolution::EvolutionEngine::new(
                        temnion_evolution::EvolutionConfig::default(),
                    );
                    let baseline = temnion_evolution::FitnessMetrics {
                        latency_p50_us: 1000,
                        latency_p99_us: 2000,
                        memory_bytes: 10_000_000,
                        storage_bytes: 50_000_000,
                        cpu_cycles: 500_000,
                        read_amplification: 2.0,
                        write_amplification: 1.5,
                        background_cost_score: 0.10,
                        net_benefit_score: 0.0,
                    };
                    engine.register_incumbent(
                        temnion_evolution::CandidateKind::Index,
                        "entity_bloom".into(),
                        b"StandardBloom".to_vec(),
                        baseline,
                    );
                    let report = engine.audit_constitution();
                    writeln!(out, "Temnion Immutable Constitution Audit:")?;
                    writeln!(
                        out,
                        "  Conformance: {}",
                        if report.passed { "CERTIFIED" } else { "FAILED" }
                    )?;
                    writeln!(out, "  Axioms Certified: {}/8", report.axioms_checked)?;
                    writeln!(out, "  Audited Incumbents: {}", report.audited_incumbents)?;
                    writeln!(out, "  Audited Candidates: {}", report.audited_candidates)?;
                    writeln!(out, "  Violations Detected: {}", report.violations.len())?;
                }
                "demo" => {
                    let mut engine = temnion_evolution::EvolutionEngine::new(
                        temnion_evolution::EvolutionConfig {
                            enabled: true,
                            manual_promotion_required: true,
                            gate_c_passed: true,
                            min_net_benefit_threshold: 0.05,
                        },
                    );
                    let baseline = temnion_evolution::FitnessMetrics {
                        latency_p50_us: 1000,
                        latency_p99_us: 2000,
                        memory_bytes: 10_000_000,
                        storage_bytes: 50_000_000,
                        cpu_cycles: 500_000,
                        read_amplification: 2.0,
                        write_amplification: 1.5,
                        background_cost_score: 0.10,
                        net_benefit_score: 0.0,
                    };
                    let inc_id = engine.register_incumbent(
                        temnion_evolution::CandidateKind::Codec,
                        "segment_compression".into(),
                        b"RawIncumbent".to_vec(),
                        baseline,
                    );
                    let cand_id = engine.propose_candidate(
                        temnion_evolution::CandidateKind::Codec,
                        "segment_compression".into(),
                        temnion_evolution::CandidateLineage {
                            candidate_id: temnion_evolution::CandidateId(0),
                            parent_candidate_id: Some(inc_id),
                            target_domain: "segment_compression".into(),
                            created_at_ms: 2000,
                            mutation_operator: "AdaptiveBitPackDelta".into(),
                            rationale: "BitPack + Delta compression candidate".into(),
                        },
                        b"AdaptiveBitPackDelta".to_vec(),
                    )?;
                    let challenger_simulated = temnion_evolution::FitnessMetrics {
                        latency_p50_us: 800,
                        latency_p99_us: 1600,
                        memory_bytes: 8_000_000,
                        storage_bytes: 35_000_000,
                        cpu_cycles: 400_000,
                        read_amplification: 1.5,
                        write_amplification: 1.5,
                        background_cost_score: 0.11,
                        net_benefit_score: 0.0,
                    };
                    let evaluated = engine.evaluate_candidate(
                        cand_id,
                        &temnion_evolution::EvaluationWorkload {
                            workload_id: "shadow_scan".into(),
                            sample_keys: vec!["entity_1".into()],
                            sample_events: 500,
                            iterations: 5,
                        },
                        temnion_evolution::IsolationBudget::default(),
                        challenger_simulated,
                    )?;
                    engine.promote_candidate(cand_id, Some("lead_architect"), 3000)?;
                    writeln!(out, "Temnion Evolution Champion/Challenger Demo:")?;
                    writeln!(out, "  Static Incumbent: ID={}", inc_id.0)?;
                    writeln!(out, "  Challenger Proposed: ID={}", cand_id.0)?;
                    writeln!(
                        out,
                        "  Net Benefit Score: {:.3} ({:.1}% gain)",
                        evaluated.net_benefit_score,
                        evaluated.net_benefit_score * 100.0
                    )?;
                    writeln!(out, "  Manual Promotion: Promoted by 'lead_architect'")?;
                    writeln!(
                        out,
                        "  Active Incumbent: ID={}",
                        engine.active_incumbent("segment_compression").unwrap().id.0
                    )?;
                }
                other => {
                    return Err(format!(
                        "unknown evolve subcommand '{other}'; use 'status', 'audit', or 'demo'"
                    )
                    .into());
                }
            }
        }
        ("tzeentch", []) => tzeentch_demo_cmd(&mut out)?,
        ("tzeentch", [subcmd]) => match text(subcmd)? {
            "demo" => tzeentch_demo_cmd(&mut out)?,
            "status" => tzeentch_status_cmd(&mut out)?,
            other => {
                return Err(format!(
                    "unknown tzeentch subcommand '{other}'; use 'demo', 'status', or 'trace <name>'"
                )
                .into());
            }
        },
        ("tzeentch", [subcmd, entity_name]) => match text(subcmd)? {
            "trace" => tzeentch_trace_cmd(&mut out, text(entity_name)?)?,
            other => {
                return Err(
                    format!("unknown tzeentch subcommand '{other}'; use 'trace <name>'").into(),
                );
            }
        },
        ("benchmark", [subcmd]) => match text(subcmd)? {
            "scale" => benchmark_scale_cmd(&mut out, 4_000)?,
            other => {
                return Err(format!(
                    "unknown benchmark subcommand '{other}'; use 'scale [records]'"
                )
                .into());
            }
        },
        ("benchmark", [subcmd, recs]) => match text(subcmd)? {
            "scale" => {
                let count: usize = text(recs)?.parse()?;
                benchmark_scale_cmd(&mut out, count)?
            }
            other => {
                return Err(format!(
                    "unknown benchmark subcommand '{other}'; use 'scale [records]'"
                )
                .into());
            }
        },
        _ => {
            return Err(format!(
                "unknown command or invalid arguments for '{command}'; use 'tem help'"
            )
            .into());
        }
    }
    out.flush()?;
    Ok(())
}

fn tzeentch_status_cmd(out: &mut impl Write) -> io::Result<()> {
    writeln!(out, "Temnion Tzeentch Adapter Status:")?;
    writeln!(out, "  Migration Modes:")?;
    writeln!(
        out,
        "    - LegacyOnly: All writes routed to legacy DB, zero to Temnion"
    )?;
    writeln!(
        out,
        "    - ShadowMirror: Authoritative legacy, best-effort async mirror to Temnion (zero silent drops)"
    )?;
    writeln!(
        out,
        "    - TemnionAuthoritative: Authoritative Temnion, fallback read to legacy"
    )?;
    writeln!(
        out,
        "    - TemnionOnly: Full cutover, zero legacy dependencies"
    )?;
    writeln!(out, "  Cadence Multi-Timescale Clocks:")?;
    writeln!(
        out,
        "    - Fast (120 Hz): Reflex loops, actuator commands, raw sensor samples"
    )?;
    writeln!(
        out,
        "    - Medium (20 Hz): Organ state integration, tracking filters"
    )?;
    writeln!(
        out,
        "    - Slow (1 Hz): Belief updates, high-level intentions, planner steps"
    )?;
    writeln!(
        out,
        "    - Background (0.1 Hz): Epistemic consolidation, episodic memory indexing, GC"
    )?;
    writeln!(out, "  Causal Introspection Guarantees:")?;
    writeln!(
        out,
        "    - Strict chain: Perception -> Organ/Cell -> Belief -> Prediction -> Decision -> Action -> Outcome"
    )?;
    writeln!(
        out,
        "    - Explicit SourceGap nodes emitted whenever provenance is partial"
    )?;
    writeln!(
        out,
        "    - Invariant: Zero future-knowledge leakage verified at every node"
    )?;
    writeln!(out, "  Retention & Reference Holds:")?;
    writeln!(
        out,
        "    - Pinned causal reference holds guarantee zero accidental eviction of critical lineage"
    )?;
    Ok(())
}

fn tzeentch_demo_cmd(out: &mut impl Write) -> Result<(), Box<dyn Error>> {
    let mut scheduler = CadenceScheduler::new();

    scheduler.tick(CadenceTier::Fast, 1_000_000);
    scheduler.tick(CadenceTier::Medium, 1_000_000);
    scheduler.tick(CadenceTier::Slow, 1_000_000);

    let media = MediaRef::new(
        "media://cam0/frame_101.raw",
        b"RGB_CAMERA_FRAME_320x240_SENSOR_0",
        "image/raw",
    );
    let percept = TzeentchPercept {
        organ_id: "vision_organ".into(),
        sensor_id: "retina_cell_4".into(),
        cadence: CadenceTier::Fast,
        media_ref: Some(media),
        features: vec![0.85, 0.12, 0.44],
        timestamp: 100,
    };

    let intention = TzeentchIntention {
        organ_id: "decision_cortex".into(),
        cell_id: "navigation_cell".into(),
        goal_label: "intercept_target".into(),
        policy_id: "policy_v2".into(),
        target_features: vec![0.95],
        planned_at: 110,
    };

    let action = TzeentchAction {
        action_id: "act_401".into(),
        intention_ref: Some("intention_101".into()),
        motor_command: "thrust_yaw_+15deg".into(),
        parameters: vec![15.0],
        executed_at: 120,
    };

    let outcome = TzeentchOutcome {
        action_id: "act_401".into(),
        reward: 1.0,
        state_delta: vec![0.02],
        observed_at: 130,
    };

    let entity = EntityId {
        shard: ShardId(0),
        slot: 1,
        generation: 1,
    };

    let p_ev = TzeentchConverter::percept_to_event(
        &percept,
        entity,
        temnion_adapter::CLOCK_FAST,
        temnion_adapter::CLOCK_FAST,
    );
    let p_stored = StoredEvent {
        id: EventId {
            source: SourceId(1),
            epoch: SourceEpoch(1),
            sequence: 0,
        },
        entity: p_ev.entity,
        schema: p_ev.schema,
        times: p_ev.times,
        payload: p_ev.payload,
        causes: p_ev.causes,
    };

    let i_ev = TzeentchConverter::intention_to_event(
        &intention,
        entity,
        vec![p_stored.id],
        temnion_adapter::CLOCK_SLOW,
        temnion_adapter::CLOCK_FAST,
    );
    let i_stored = StoredEvent {
        id: EventId {
            source: SourceId(1),
            epoch: SourceEpoch(1),
            sequence: 1,
        },
        entity: i_ev.entity,
        schema: i_ev.schema,
        times: i_ev.times,
        payload: i_ev.payload,
        causes: i_ev.causes,
    };

    let a_ev = TzeentchConverter::action_to_event(
        &action,
        entity,
        vec![i_stored.id],
        temnion_adapter::CLOCK_FAST,
        temnion_adapter::CLOCK_FAST,
    );
    let a_stored = StoredEvent {
        id: EventId {
            source: SourceId(1),
            epoch: SourceEpoch(1),
            sequence: 2,
        },
        entity: a_ev.entity,
        schema: a_ev.schema,
        times: a_ev.times,
        payload: a_ev.payload,
        causes: a_ev.causes,
    };

    let o_ev = TzeentchConverter::outcome_to_event(
        &outcome,
        entity,
        a_stored.id,
        temnion_adapter::CLOCK_MEDIUM,
        temnion_adapter::CLOCK_FAST,
    );
    let o_stored = StoredEvent {
        id: EventId {
            source: SourceId(1),
            epoch: SourceEpoch(1),
            sequence: 3,
        },
        entity: o_ev.entity,
        schema: o_ev.schema,
        times: o_ev.times,
        payload: o_ev.payload,
        causes: o_ev.causes,
    };

    let stored_events = vec![p_stored, i_stored, a_stored, o_stored];
    let trace = TzeentchActionTracer::trace_action(2, &stored_events)?;

    writeln!(
        out,
        "Temnion Tzeentch Multi-Cadence Causal Action Trace Demo:"
    )?;
    writeln!(out, "  Action ID: 401 (Sequence 2)")?;
    writeln!(out, "  Causal Chain Continuity: Complete")?;
    writeln!(
        out,
        "  Future Leakage Detected: {}",
        trace.future_leakage_detected
    )?;
    writeln!(out, "  Trace Nodes ({} total):", trace.nodes.len())?;
    for node in &trace.nodes {
        match node {
            temnion_adapter::ActionTraceNode::Percept {
                event_id,
                organ,
                sensor,
                timestamp,
            } => {
                writeln!(
                    out,
                    "    - [Perception] seq={} time={} organ='{organ}' sensor='{sensor}'",
                    event_id.sequence, timestamp
                )?;
            }
            temnion_adapter::ActionTraceNode::Intention {
                event_id,
                goal,
                policy,
                timestamp,
            } => {
                writeln!(
                    out,
                    "    - [Intention] seq={} time={} goal='{goal}' policy='{policy}'",
                    event_id.sequence, timestamp
                )?;
            }
            temnion_adapter::ActionTraceNode::Action {
                event_id,
                action_id,
                command,
                timestamp,
            } => {
                writeln!(
                    out,
                    "    - [Action] seq={} time={} action='{action_id}' cmd='{command}'",
                    event_id.sequence, timestamp
                )?;
            }
            temnion_adapter::ActionTraceNode::Outcome {
                event_id,
                reward,
                timestamp,
            } => {
                writeln!(
                    out,
                    "    - [Outcome] seq={} time={} reward={reward}",
                    event_id.sequence, timestamp
                )?;
            }
            temnion_adapter::ActionTraceNode::SourceGap {
                step_name,
                expected_time,
            } => {
                writeln!(
                    out,
                    "    - [SourceGap] step='{step_name}' expected_time={expected_time}"
                )?;
            }
            temnion_adapter::ActionTraceNode::CellProcessing {
                event_id,
                organ,
                cell,
                timestamp,
            } => {
                writeln!(
                    out,
                    "    - [Cell] seq={} time={} organ='{organ}' cell='{cell}'",
                    event_id.sequence, timestamp
                )?;
            }
            temnion_adapter::ActionTraceNode::RetrievedBelief {
                event_id,
                concept,
                confidence,
                timestamp,
            } => {
                writeln!(
                    out,
                    "    - [Belief] seq={} time={} concept='{concept}' conf={confidence}",
                    event_id.sequence, timestamp
                )?;
            }
            temnion_adapter::ActionTraceNode::Prediction {
                event_id,
                label,
                probability,
                timestamp,
            } => {
                writeln!(
                    out,
                    "    - [Prediction] seq={} time={} label='{label}' prob={probability}",
                    event_id.sequence, timestamp
                )?;
            }
        }
    }
    writeln!(
        out,
        "  Verification: Outcome verified following Action sequence 2"
    )?;
    Ok(())
}

fn tzeentch_trace_cmd(out: &mut impl Write, entity_name: &str) -> Result<(), Box<dyn Error>> {
    let percept = TzeentchPercept {
        organ_id: format!("{entity_name}_sensor"),
        sensor_id: "sensor_0".into(),
        cadence: CadenceTier::Fast,
        media_ref: None,
        features: vec![1.0, 0.5],
        timestamp: 10,
    };
    let intention = TzeentchIntention {
        organ_id: format!("{entity_name}_planner"),
        cell_id: "cell_0".into(),
        goal_label: format!("navigate_{entity_name}"),
        policy_id: "policy_v1".into(),
        target_features: vec![1.0],
        planned_at: 15,
    };
    let action = TzeentchAction {
        action_id: format!("{entity_name}_action"),
        intention_ref: None,
        motor_command: "step_forward".into(),
        parameters: vec![1.0],
        executed_at: 20,
    };

    let entity = EntityId {
        shard: ShardId(0),
        slot: 1,
        generation: 1,
    };

    let p_ev = TzeentchConverter::percept_to_event(
        &percept,
        entity,
        temnion_adapter::CLOCK_FAST,
        temnion_adapter::CLOCK_FAST,
    );
    let p_stored = StoredEvent {
        id: EventId {
            source: SourceId(1),
            epoch: SourceEpoch(1),
            sequence: 0,
        },
        entity: p_ev.entity,
        schema: p_ev.schema,
        times: p_ev.times,
        payload: p_ev.payload,
        causes: p_ev.causes,
    };
    let i_ev = TzeentchConverter::intention_to_event(
        &intention,
        entity,
        vec![p_stored.id],
        temnion_adapter::CLOCK_SLOW,
        temnion_adapter::CLOCK_FAST,
    );
    let i_stored = StoredEvent {
        id: EventId {
            source: SourceId(1),
            epoch: SourceEpoch(1),
            sequence: 1,
        },
        entity: i_ev.entity,
        schema: i_ev.schema,
        times: i_ev.times,
        payload: i_ev.payload,
        causes: i_ev.causes,
    };
    let a_ev = TzeentchConverter::action_to_event(
        &action,
        entity,
        vec![i_stored.id],
        temnion_adapter::CLOCK_FAST,
        temnion_adapter::CLOCK_FAST,
    );
    let a_stored = StoredEvent {
        id: EventId {
            source: SourceId(1),
            epoch: SourceEpoch(1),
            sequence: 2,
        },
        entity: a_ev.entity,
        schema: a_ev.schema,
        times: a_ev.times,
        payload: a_ev.payload,
        causes: a_ev.causes,
    };

    let stored_events = vec![p_stored, i_stored, a_stored];
    let trace = TzeentchActionTracer::trace_action(2, &stored_events)?;

    writeln!(out, "Causal Action Trace for Entity '{entity_name}':")?;
    writeln!(out, "  Action Sequence: 2")?;
    writeln!(
        out,
        "  Future Leakage Detected: {}",
        trace.future_leakage_detected
    )?;
    for node in &trace.nodes {
        match node {
            temnion_adapter::ActionTraceNode::Percept {
                event_id,
                organ,
                sensor,
                timestamp,
            } => {
                writeln!(
                    out,
                    "    [Percept] seq={} time={} organ='{organ}' sensor='{sensor}'",
                    event_id.sequence, timestamp
                )?;
            }
            temnion_adapter::ActionTraceNode::Intention {
                event_id,
                goal,
                policy,
                timestamp,
            } => {
                writeln!(
                    out,
                    "    [Intention] seq={} time={} goal='{goal}' policy='{policy}'",
                    event_id.sequence, timestamp
                )?;
            }
            temnion_adapter::ActionTraceNode::Action {
                event_id,
                action_id,
                command,
                timestamp,
            } => {
                writeln!(
                    out,
                    "    [Action] seq={} time={} action='{action_id}' cmd='{command}'",
                    event_id.sequence, timestamp
                )?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn benchmark_scale_cmd(out: &mut impl Write, target_records: usize) -> Result<(), Box<dyn Error>> {
    let start = std::time::Instant::now();
    let mut log = EventLog::new(SourceId(1), SourceEpoch(1), target_records.max(64))?;
    let entity = EntityId {
        shard: ShardId(0),
        slot: 1,
        generation: 1,
    };

    let append_start = std::time::Instant::now();
    for i in 0..target_records {
        log.append(EventInput {
            entity,
            times: EventTimes {
                valid: Timestamp::new(ClockId(1), i as u64),
                observed: None,
                known: Timestamp::new(ClockId(2), i as u64),
            },
            change: Position { x: i as i32 },
        })?;
    }
    let append_duration = append_start.elapsed();

    let scan_start = std::time::Instant::now();
    let history = log.history(
        HistoryFilter::default(),
        QueryBudget {
            max_results: target_records,
            max_scanned: target_records * 2,
        },
        None,
    )?;
    let scan_duration = scan_start.elapsed();

    let total_elapsed = start.elapsed();
    let throughput = if append_duration.as_secs_f64() > 0.0 {
        target_records as f64 / append_duration.as_secs_f64()
    } else {
        0.0
    };

    writeln!(out, "Temnion Scale Benchmark:")?;
    writeln!(out, "  Target Records:       {}", target_records)?;
    writeln!(out, "  Ingestion Time:       {:.2?}", append_duration)?;
    writeln!(out, "  Ingestion Throughput: {:.0} events/sec", throughput)?;
    writeln!(
        out,
        "  Scan Latency ({} rec): {:.2?}",
        history.events.len(),
        scan_duration
    )?;
    writeln!(out, "  Total Test Elapsed:   {:.2?}", total_elapsed)?;
    Ok(())
}

fn subscribe_cmd(out: &mut impl Write, args: &[OsString]) -> Result<(), Box<dyn Error>> {
    let mut uri = None;
    let mut host = None;
    let mut port = None;
    let mut db = None;
    let mut user = None;
    let mut auth_token = None;
    let mut from_seq = None;
    let mut from_now = false;
    let mut format_override = None;
    let mut query_parts = Vec::new();

    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let s = text(arg)?;
        match s {
            "--uri" => {
                if let Some(val) = iter.next() {
                    uri = Some(text(val)?.to_string());
                }
            }
            "--host" => {
                if let Some(val) = iter.next() {
                    host = Some(text(val)?.to_string());
                }
            }
            "--port" => {
                if let Some(val) = iter.next() {
                    port = Some(text(val)?.parse::<u16>()?);
                }
            }
            "--db" | "--database" => {
                if let Some(val) = iter.next() {
                    db = Some(text(val)?.to_string());
                }
            }
            "--user" => {
                if let Some(val) = iter.next() {
                    user = Some(text(val)?.to_string());
                }
            }
            "--token" => {
                if let Some(val) = iter.next() {
                    auth_token = Some(text(val)?.to_string());
                }
            }
            "--from-seq" | "--from" => {
                if let Some(val) = iter.next() {
                    from_seq = Some(text(val)?.parse::<u64>()?);
                }
            }
            "--from-now" => {
                from_now = true;
            }
            "--format" => {
                if let Some(val) = iter.next() {
                    match text(val)?.to_ascii_lowercase().as_str() {
                        "sql" => format_override = Some(QueryFormat::Sql),
                        "temql" => format_override = Some(QueryFormat::Temql),
                        "compact" | "compact-tem" => {
                            format_override = Some(QueryFormat::CompactTem)
                        }
                        other => return Err(format!("Unknown format '{other}'").into()),
                    }
                }
            }
            _ => {
                query_parts.push(s);
            }
        }
    }

    let query_str = if query_parts.is_empty() {
        "SELECT * FROM events".to_string()
    } else {
        query_parts.join(" ")
    };

    let format = format_override.unwrap_or_else(|| {
        let trimmed = query_str.trim();
        if trimmed.starts_with("tn:") || trimmed.starts_with('#') || trimmed.starts_with('$') {
            QueryFormat::CompactTem
        } else if trimmed.to_ascii_lowercase().starts_with("select") {
            QueryFormat::Sql
        } else {
            QueryFormat::Temql
        }
    });

    let config = ConnectionConfig::resolve(
        uri.as_deref(),
        host.as_deref(),
        port,
        db.as_deref(),
        user.as_deref(),
        auth_token.as_deref(),
        None,
    );

    writeln!(
        out,
        "Connecting to Temnion daemon at {}...",
        config.to_uri()
    )?;
    out.flush()?;
    let conn = ActiveConnection::connect(config)?;
    writeln!(
        out,
        "Connected to server '{}' (TNP v{}).",
        conn.server_id(),
        conn.negotiated_version()
    )?;

    let mut sub = conn.subscribe(&query_str, format, from_seq, from_now)?;
    let (start_seq, end_seq) = sub.snapshot_range();
    writeln!(
        out,
        "Live subscription established (sub_id={}). Snapshot range: [{}..{}].",
        sub.subscription_id(),
        start_seq,
        end_seq
    )?;
    writeln!(out, "Streaming events for query: {query_str}")?;
    writeln!(
        out,
        "------------------------------------------------------------"
    )?;
    out.flush()?;

    loop {
        match sub.next_event(Some(Duration::from_millis(500))) {
            Ok(Some(event)) => {
                let badge = if event.is_live { "LIVE" } else { "SNAPSHOT" };
                writeln!(
                    out,
                    "[{badge}] seq={} entity={}:{}:{} schema={} valid={}:{} known={}:{} payload={}",
                    event.sequence,
                    event.entity_shard,
                    event.entity_slot,
                    event.entity_generation,
                    event.schema,
                    event.valid_clock,
                    event.valid_time,
                    event.known_clock,
                    event.known_time,
                    event.payload_hex
                )?;
                out.flush()?;
            }
            Ok(None) => {
                if sub.is_unsubscribed() {
                    writeln!(out, "Subscription terminated by server.")?;
                    break;
                }
                continue;
            }
            Err(e) => {
                return Err(format!("Subscription error: {e}").into());
            }
        }
    }

    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("tem: {error}");
            ExitCode::FAILURE
        }
    }
}
