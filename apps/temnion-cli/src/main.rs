// SPDX-License-Identifier: AGPL-3.0-only
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

use temnion_codec::CodecScorer;
use temnion_core::{
    ClockId, EntityId, EventTimes, SchemaId, ShardId, SourceEpoch, SourceId, Timestamp,
};
use temnion_events::{EventInput, EventLog, HistoryFilter, QueryBudget};
use temnion_format::{Limits, StoredEvent, decode_segment};
use temnion_replay::{
    Checkpoint, RawEntityReducer, ReplayEngine, decode_raw_entity_map, encode_raw_entity_map,
};
use temnion_state::StateSlab;
use temnion_storage::{RecoveryMode, StorageQueryBudget, Store, WriteEvent, read_bounded_file};

const HELP: &str = "\
tem - Temnion CLI

Usage: tem [help | version | describe | demo]
       tem init <database-directory>
       tem append <directory> <shard:slot:generation> <schema-id> <valid-clock:tick> <known-clock:tick> <hex-payload>
       tem history <directory> [row-limit]
       tem inspect <directory>
       tem recover <directory>
       tem seal <directory>
       tem verify-segment <segment-file>
       tem checkpoint <directory>
       tem reconstruct <directory> <sequence>
       tem evaluate-codecs

  help       Show this help
  version    Show the version
  describe   Print implemented capabilities as JSON
  demo       Run an in-memory state/history example; writes no files
  init       Create a durable source log with an OS-random database identity
  append     Persist one opaque typed payload; acknowledge only after OS sync
  history    Read one bounded history page (payload previews, default 100 rows)
  inspect    Validate the WAL and report durable records and bytes
  recover    Explicitly truncate an incomplete tail; never skip corrupt frames
  seal       Export immutable TSF segments while retaining the authoritative WAL
  verify-segment  Validate a standalone TSF segment
  checkpoint      Take an atomic checksummed state checkpoint pinned to current WAL sequence
  reconstruct     Deterministically replay WAL events to target sequence
  evaluate-codecs Evaluate candidate lossless codecs on representative streams

The append command is a low-level schema-ID/opaque-payload interface.
Ordinary open never silently truncates history. temniond, TemQL, TNP,
MCP, and Temnion Studio are not implemented yet.";

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
    "\"deterministic-reconstruction\", \"lossless-codecs\"],\n",
    "  \"durable\": true,\n",
    "  \"server\": false,\n",
    "  \"temql\": false,\n",
    "  \"tnp\": false,\n",
    "  \"tsf\": true,\n",
    "  \"mcp\": false,\n",
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
        ("append", [path, entity, schema, valid, known, payload]) => {
            let input = WriteEvent {
                entity: parse_entity(entity)?,
                schema: SchemaId(text(schema)?.parse()?),
                times: EventTimes {
                    valid: parse_time(valid)?,
                    observed: None,
                    known: parse_time(known)?,
                },
                payload: decode_hex(payload)?,
                causes: Vec::new(),
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

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("tem: {error}");
            ExitCode::FAILURE
        }
    }
}
