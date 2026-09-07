// SPDX-License-Identifier: AGPL-3.0-only
use std::error::Error;
use std::io::{self, Write};
use std::process::ExitCode;

use temnion_core::{ClockId, EventTimes, ShardId, SourceEpoch, SourceId, Timestamp};
use temnion_events::{EventInput, EventLog, HistoryFilter, QueryBudget};
use temnion_state::StateSlab;

const HELP: &str = "\
tem - Temnion foundation CLI

Usage: tem [help | version | describe | demo]

  help       Show this help
  version    Show the foundation version
  describe   Print implemented capabilities as JSON
  demo       Run an in-memory state/history example; writes no files

This milestone is volatile only. Durable storage, temniond, TemQL, TNP,
MCP, and Temnion Studio are not implemented yet.";

const CAPABILITIES: &str = concat!(
    "{\n",
    "  \"name\": \"Temnion\",\n",
    "  \"version\": \"",
    env!("CARGO_PKG_VERSION"),
    "\",\n",
    "  \"maturity\": \"foundation\",\n",
    "  \"storage\": \"volatile-memory-only\",\n",
    "  \"implemented\": [\"packed-state\", \"generational-entities\", ",
    "\"typed-events\", \"atomic-batch-admission\", \"entity-history\", ",
    "\"time-range-filter\", \"known-as-of\", \"bounded-snapshot-pagination\"],\n",
    "  \"durable\": false,\n",
    "  \"server\": false,\n",
    "  \"temql\": false,\n",
    "  \"tnp\": false,\n",
    "  \"tsf\": false,\n",
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

fn run() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args_os().skip(1);
    let command = args
        .next()
        .map(|value| value.into_string())
        .transpose()
        .map_err(|_| "command is not valid Unicode")?;
    if args.next().is_some() {
        return Err("expected one command; use 'tem help'".into());
    }
    let mut out = io::stdout().lock();
    match command.as_deref().unwrap_or("help") {
        "help" | "--help" | "-h" => writeln!(out, "{HELP}")?,
        "version" | "--version" | "-V" => writeln!(out, "tem {}", env!("CARGO_PKG_VERSION"))?,
        "describe" => writeln!(out, "{CAPABILITIES}")?,
        "demo" => demo(&mut out)?,
        other => return Err(format!("unknown command '{other}'; use 'tem help'").into()),
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
