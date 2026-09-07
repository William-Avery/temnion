// SPDX-License-Identifier: AGPL-3.0-only
//! Explicit on-disk workload: synchronized Store batches versus a framed-file lower bound.
use std::error::Error;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use temnion_core::{
    ClockId, EntityId, EventId, EventTimes, SchemaId, ShardId, SourceEpoch, SourceId, Timestamp,
};
use temnion_format::{Limits, StoredEvent, encode_batch, encode_wal_header};
use temnion_storage::{RecoveryMode, Store, WriteEvent};

#[derive(Debug, PartialEq, Eq)]
struct Config {
    directory: PathBuf,
    events: usize,
    batch_size: usize,
}

fn parse(args: impl IntoIterator<Item = String>) -> Result<Config, Box<dyn Error>> {
    let mut args = args.into_iter();
    let mut directory = None;
    let mut events = 4096;
    let mut batch_size = 256;
    let mut seen = [false; 3];
    while let Some(flag) = args.next() {
        let index = match flag.as_str() {
            "--directory" => 0,
            "--events" => 1,
            "--batch-size" => 2,
            _ => return Err(format!("unknown option '{flag}'").into()),
        };
        if seen[index] {
            return Err(format!("duplicate option '{flag}'").into());
        }
        seen[index] = true;
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for '{flag}'"))?;
        match index {
            0 => directory = Some(PathBuf::from(value)),
            1 => events = value.parse()?,
            _ => batch_size = value.parse()?,
        }
    }
    if events == 0 || events > 1_000_000 || batch_size == 0 || batch_size > 4096 {
        return Err("events must be 1..=1000000 and batch-size 1..=4096".into());
    }
    Ok(Config {
        directory: directory.ok_or("required: --directory <new benchmark directory>")?,
        events,
        batch_size,
    })
}

fn input(sequence: u64) -> WriteEvent {
    WriteEvent {
        entity: EntityId {
            shard: ShardId(0),
            slot: (sequence % 4096) as u32,
            generation: 0,
        },
        times: EventTimes {
            valid: Timestamp::new(ClockId(1), sequence),
            observed: None,
            known: Timestamp::new(ClockId(2), sequence),
        },
        schema: SchemaId(1),
        payload: sequence.to_le_bytes().to_vec(),
        causes: Vec::new(),
    }
}

fn report(label: &str, events: usize, bytes: u64, elapsed_ns: u128, batches: &mut [u128]) {
    batches.sort_unstable();
    let median = batches[batches.len() / 2];
    let p95 = batches[((batches.len() * 95).div_ceil(100) - 1).min(batches.len() - 1)];
    println!(
        "{label},{events},{},{bytes},{elapsed_ns},{median},{p95}",
        batches.len()
    );
}

fn same_bytes(left: &Path, right: &Path) -> Result<bool, Box<dyn Error>> {
    let mut left = fs::File::open(left)?;
    let mut right = fs::File::open(right)?;
    let mut remaining = left.metadata()?.len();
    if remaining != right.metadata()?.len() {
        return Ok(false);
    }
    let mut a = [0u8; 64 * 1024];
    let mut b = [0u8; 64 * 1024];
    while remaining != 0 {
        let count = remaining.min(a.len() as u64) as usize;
        left.read_exact(&mut a[..count])?;
        right.read_exact(&mut b[..count])?;
        if a[..count] != b[..count] {
            return Ok(false);
        }
        remaining -= count as u64;
    }
    Ok(true)
}

fn run(config: Config) -> Result<(), Box<dyn Error>> {
    fs::create_dir(&config.directory)?;
    let limits = Limits::default();
    let database_path = config.directory.join("store");
    let mut store = Store::create(&database_path, SourceId(1), SourceEpoch(1), limits)?;
    let header = store.header();
    let mut samples = Vec::new();
    samples.try_reserve_exact(config.events.div_ceil(config.batch_size))?;
    let started = Instant::now();
    for start in (0..config.events).step_by(config.batch_size) {
        let end = (start + config.batch_size).min(config.events);
        let batch: Vec<_> = (start..end)
            .map(|sequence| input(sequence as u64))
            .collect();
        let batch_start = Instant::now();
        store.append(batch)?;
        samples.push(batch_start.elapsed().as_nanos());
    }
    let elapsed = started.elapsed().as_nanos();
    let store_bytes = store.wal_bytes();
    println!("# volatile=false; acknowledgment=File::sync_all per batch; no lossy encoding");
    println!(
        "# os={} arch={} version={}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        env!("CARGO_PKG_VERSION")
    );
    println!(
        "# times exclude database creation; per-batch times include encoding, writing and sync"
    );
    println!("implementation,events,batches,wal_bytes,elapsed_ns,batch_p50_ns,batch_p95_ns");
    report("Store", config.events, store_bytes, elapsed, &mut samples);
    drop(store);

    let started = Instant::now();
    let (mut reopened, recovered) = Store::open(
        &database_path,
        Limits::default(),
        RecoveryMode::RejectIncompleteTail,
    )?;
    let recovery_ns = started.elapsed().as_nanos();
    if recovered.recovered_events != config.events as u64 {
        return Err("recovery event count differs from admitted events".into());
    }
    let last = EventId {
        source: header.source,
        epoch: header.epoch,
        sequence: config.events as u64 - 1,
    };
    if reopened.get(last)?.payload != (config.events as u64 - 1).to_le_bytes() {
        return Err("recovered final payload differs from the reference".into());
    }
    println!(
        "# recovery_ns={recovery_ns} recovered_events={}",
        recovered.recovered_events
    );
    drop(reopened);

    let mut reference = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(config.directory.join("framed-reference.wal"))?;
    reference.write_all(&encode_wal_header(header))?;
    reference.sync_all()?;
    samples.clear();
    let started = Instant::now();
    for start in (0..config.events).step_by(config.batch_size) {
        let end = (start + config.batch_size).min(config.events);
        let records: Vec<_> = (start..end)
            .map(|sequence| {
                let input = input(sequence as u64);
                StoredEvent {
                    id: EventId {
                        source: header.source,
                        epoch: header.epoch,
                        sequence: sequence as u64,
                    },
                    entity: input.entity,
                    times: input.times,
                    schema: input.schema,
                    payload: input.payload,
                    causes: input.causes,
                }
            })
            .collect();
        let batch_start = Instant::now();
        reference.write_all(&encode_batch(&records, &Limits::default())?)?;
        reference.sync_all()?;
        samples.push(batch_start.elapsed().as_nanos());
    }
    let elapsed = started.elapsed().as_nanos();
    let reference_bytes = reference.metadata()?.len();
    drop(reference);
    if !same_bytes(
        &database_path.join("events.wal"),
        &config.directory.join("framed-reference.wal"),
    )? {
        return Err("durable Store bytes differ from identically framed reference batches".into());
    }
    report(
        "Framed-file-lower-bound",
        config.events,
        reference_bytes,
        elapsed,
        &mut samples,
    );
    println!("# The file reference lacks locking, admission validation, indexes and query APIs.");
    println!("# This is not Gate A or a production database competitor comparison.");
    Ok(())
}

fn main() -> ExitCode {
    let args = std::env::args_os()
        .skip(1)
        .map(|arg| arg.into_string().map_err(|_| "option is not valid Unicode"))
        .collect::<Result<Vec<_>, _>>();
    let result = args
        .map_err(Box::<dyn Error>::from)
        .and_then(parse)
        .and_then(run);
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("temnion durable benchmark: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(args: &[&str]) -> Result<Config, Box<dyn Error>> {
        parse(args.iter().map(|value| (*value).to_owned()))
    }

    #[test]
    fn output_directory_is_required_and_work_is_bounded() {
        assert!(config(&[]).is_err());
        assert_eq!(config(&["--directory", "fresh"]).unwrap().events, 4096);
        assert!(config(&["--directory", "fresh", "--events", "0"]).is_err());
        assert!(config(&["--directory", "fresh", "--batch-size", "4097"]).is_err());
        assert!(config(&["--directory", "fresh", "--events", "2", "--events", "3"]).is_err());
    }
}
