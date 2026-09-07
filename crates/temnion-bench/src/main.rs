// SPDX-License-Identifier: AGPL-3.0-only
//! Small standard-library baseline runner, not a durable-database benchmark.

use std::collections::HashMap;
use std::error::Error;
use std::hint::black_box;
use std::process::ExitCode;
use std::time::Instant;

use temnion_core::{ClockId, EntityId, EventTimes, ShardId, SourceEpoch, SourceId, Timestamp};
use temnion_events::{EventInput, EventLog, HistoryFilter, QueryBudget};
use temnion_state::StateSlab;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Config {
    entities: usize,
    events: usize,
    iterations: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            entities: 4096,
            events: 100_000,
            iterations: 100_000,
        }
    }
}

fn config(args: impl Iterator<Item = String>) -> Result<Config, Box<dyn Error>> {
    let mut config = Config::default();
    let mut args = args.peekable();
    let mut seen = [false; 3];
    while let Some(flag) = args.next() {
        let (index, max) = match flag.as_str() {
            "--entities" => (0, 10_000_000),
            "--events" => (1, 100_000_000),
            "--iterations" => (2, 100_000_000),
            _ => return Err(format!("unknown option '{flag}'").into()),
        };
        if seen[index] {
            return Err(format!("duplicate option '{flag}'").into());
        }
        seen[index] = true;
        let value: usize = args
            .next()
            .ok_or_else(|| format!("missing value for '{flag}'"))?
            .parse()?;
        if value == 0 || value > max {
            return Err(format!("{flag} must be between 1 and {max}").into());
        }
        match index {
            0 => config.entities = value,
            1 => config.events = value,
            _ => config.iterations = value,
        }
    }
    Ok(config)
}

fn report(
    workload: &str,
    implementation: &str,
    operations: usize,
    started: Instant,
    checksum: u64,
) {
    let ns = started.elapsed().as_nanos();
    println!(
        "{workload},{implementation},{operations},{ns},{:.3},{checksum}",
        ns as f64 / operations as f64
    );
}

fn input(entity: EntityId, tick: u64) -> EventInput<u64> {
    EventInput {
        entity,
        times: EventTimes {
            valid: Timestamp::new(ClockId(1), tick),
            observed: None,
            known: Timestamp::new(ClockId(2), tick),
        },
        change: tick,
    }
}

fn run(config: Config) -> Result<(), Box<dyn Error>> {
    println!("# Temnion foundation benchmark v1; volatile only; no filesystem synchronization");
    println!(
        "# os={} arch={} version={}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        env!("CARGO_PKG_VERSION")
    );
    println!(
        "# entities={} events={} iterations={} seed=42",
        config.entities, config.events, config.iterations
    );
    println!("# Latencies are aggregate ns/op, not per-operation percentiles.");
    println!("workload,implementation,operations,elapsed_ns,ns_per_op,checksum");

    let mut slab = StateSlab::new(ShardId(0), config.entities)?;
    let mut ids = Vec::new();
    let mut values = Vec::new();
    let mut map = HashMap::new();
    ids.try_reserve_exact(config.entities)?;
    values.try_reserve_exact(config.entities)?;
    map.try_reserve(config.entities)?;
    for i in 0..config.entities {
        let id = slab.insert(i as u64)?;
        ids.push(id);
        values.push(i as u64);
        map.insert(id, i as u64);
    }
    let mut indices = Vec::new();
    indices.try_reserve_exact(config.iterations)?;
    let mut rng = 42u64;
    for _ in 0..config.iterations {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
        indices.push((rng % config.entities as u64) as usize);
    }
    let mut checksum = 0u64;
    let started = Instant::now();
    for &index in &indices {
        checksum = checksum.wrapping_add(*black_box(&slab).get(black_box(ids[index]))?);
    }
    report(
        "A-current",
        "StateSlab",
        config.iterations,
        started,
        black_box(checksum),
    );
    let expected = checksum;

    let started = Instant::now();
    checksum = 0;
    for &index in &indices {
        checksum = checksum.wrapping_add(black_box(&values)[black_box(index)]);
    }
    report(
        "A-current",
        "Vec-direct-lower-bound",
        config.iterations,
        started,
        black_box(checksum),
    );
    if checksum != expected {
        return Err("Vec lookup checksum diverged from StateSlab".into());
    }
    let started = Instant::now();
    checksum = 0;
    for &index in &indices {
        checksum = checksum.wrapping_add(black_box(&map)[&black_box(ids[index])]);
    }
    report(
        "A-current",
        "HashMap",
        config.iterations,
        started,
        black_box(checksum),
    );
    if checksum != expected {
        return Err("HashMap lookup checksum diverged from StateSlab".into());
    }

    let mut log = EventLog::new(SourceId(1), SourceEpoch(1), config.events)?;
    let started = Instant::now();
    for i in 0..config.events {
        let entity = ids[i % ids.len()];
        black_box(log.append(input(entity, i as u64))?);
    }
    report(
        "B-append",
        "EventLog-validated",
        config.events,
        started,
        log.len() as u64,
    );

    // Raw Vec is a lower bound: it lacks event-ID assignment and append validation.
    let mut reference = Vec::new();
    reference.try_reserve_exact(config.events)?;
    let started = Instant::now();
    for i in 0..config.events {
        reference.push(black_box(input(ids[i % ids.len()], i as u64)));
    }
    report(
        "B-append",
        "Vec-unvalidated-lower-bound",
        config.events,
        started,
        reference.len() as u64,
    );

    let filter = HistoryFilter {
        entity: Some(ids[0]),
        ..HistoryFilter::default()
    };
    let started = Instant::now();
    let mut cursor = None;
    let mut found = 0usize;
    checksum = 0;
    loop {
        let page = log.history(
            filter,
            QueryBudget {
                max_results: 256,
                max_scanned: 4096,
            },
            cursor,
        )?;
        for event in page.events {
            found += 1;
            checksum = checksum.wrapping_add(event.change);
        }
        cursor = page.continuation;
        if cursor.is_none() {
            break;
        }
    }
    report(
        "D-history-scanned",
        "EventLog-bounded-pages",
        config.events,
        started,
        black_box(checksum),
    );
    let expected = (found, checksum);

    let started = Instant::now();
    found = 0;
    checksum = 0;
    for event in &reference {
        if event.entity == ids[0] {
            found += 1;
            checksum = checksum.wrapping_add(event.change);
        }
    }
    report(
        "D-history-scanned",
        "Vec-linear-reference",
        config.events,
        started,
        black_box(checksum),
    );
    if (found, checksum) != expected {
        return Err("history query differs from linear reference".into());
    }
    println!("# History has no index yet; both scans inspect the full snapshot.");
    println!("# Vector baselines are labeled lower bounds, not equivalent database competitors.");
    Ok(())
}

fn main() -> ExitCode {
    let args = std::env::args_os()
        .skip(1)
        .map(|value| {
            value
                .into_string()
                .map_err(|_| "argument is not valid Unicode")
        })
        .collect::<Result<Vec<_>, _>>();
    let result = args
        .map_err(Box::<dyn Error>::from)
        .and_then(|args| config(args.into_iter()))
        .and_then(run);
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("temnion-bench: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Config, Box<dyn Error>> {
        config(args.iter().map(|arg| (*arg).to_owned()))
    }

    #[test]
    fn defaults_and_custom_sizes() {
        assert_eq!(parse(&[]).unwrap(), Config::default());
        assert_eq!(
            parse(&["--entities", "4", "--events", "20", "--iterations", "100"]).unwrap(),
            Config {
                entities: 4,
                events: 20,
                iterations: 100
            }
        );
    }

    #[test]
    fn malformed_and_unbounded_workloads_fail() {
        for args in [
            &["--unknown", "1"][..],
            &["--events"][..],
            &["--events", "0"][..],
            &["--events", "-1"][..],
            &["--entities", "10000001"][..],
            &["--events", "2", "--events", "3"][..],
        ] {
            assert!(parse(args).is_err(), "{args:?}");
        }
    }

    #[test]
    fn small_workload_matches_reference_results() {
        run(Config {
            entities: 4,
            events: 20,
            iterations: 100,
        })
        .unwrap();
    }
}
