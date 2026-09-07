// SPDX-License-Identifier: AGPL-3.0-only
use std::fs::{self, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::process::Command;

use temnion_core::{
    ClockId, EntityId, EventId, EventTimes, SchemaId, ShardId, SourceEpoch, SourceId, Timestamp,
};
use temnion_events::HistoryFilter;
use temnion_format::{Limits, StoredEvent, decode_segment, encode_batch};
use temnion_storage::{
    RecoveryMode, StorageError, StorageQueryBudget, Store, WriteEvent, read_bounded_file,
};

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let mut id = [0u8; 16];
        getrandom::getrandom(&mut id).unwrap();
        let token = temnion_core::DatabaseId(id).to_string();
        let path = std::env::temp_dir().join(format!("temnion-storage-test-{token}"));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove only the test-owned temporary directory");
    }
}

fn input(valid: u64, known: u64) -> WriteEvent {
    WriteEvent {
        entity: EntityId {
            shard: ShardId(7),
            slot: 1,
            generation: 0,
        },
        times: EventTimes {
            valid: Timestamp::new(ClockId(1), valid),
            observed: None,
            known: Timestamp::new(ClockId(2), known),
        },
        schema: SchemaId(1),
        payload: valid.to_le_bytes().to_vec(),
        causes: Vec::new(),
    }
}

fn create(directory: &TestDirectory) -> Store {
    Store::create(&directory.0, SourceId(1), SourceEpoch(1), Limits::default()).unwrap()
}

fn open(directory: &TestDirectory) -> Store {
    Store::open(
        &directory.0,
        Limits::default(),
        RecoveryMode::RejectIncompleteTail,
    )
    .unwrap()
    .0
}

#[test]
fn durable_batches_survive_reopen_with_stable_identity_and_sequence() {
    let directory = TestDirectory::new();
    let mut store = create(&directory);
    let header = store.header();
    let receipt = store.append(vec![input(10, 1), input(5, 2)]).unwrap();
    assert_eq!(receipt.first.sequence, 0);
    assert_eq!(receipt.last.sequence, 1);
    assert_eq!(receipt.database, header.database);
    drop(store);
    let (mut reopened, report) = Store::open(
        &directory.0,
        Limits::default(),
        RecoveryMode::RejectIncompleteTail,
    )
    .unwrap();
    assert_eq!(reopened.header(), header);
    assert_eq!(report.recovered_events, 2);
    assert_eq!(report.discarded_tail_bytes, 0);
    assert_eq!(
        reopened.get(receipt.last).unwrap().payload,
        5u64.to_le_bytes()
    );
    assert_eq!(
        reopened.append(vec![input(11, 3)]).unwrap().first.sequence,
        2
    );
}

#[test]
fn invalid_batch_does_not_change_bytes_or_consume_ids() {
    let directory = TestDirectory::new();
    let mut store = create(&directory);
    store.append(vec![input(1, 5)]).unwrap();
    let before = fs::read(directory.0.join("events.wal")).unwrap();
    assert!(matches!(
        store.append(vec![input(2, 6), input(3, 4)]),
        Err(StorageError::KnownTimeRegression { .. })
    ));
    assert!(matches!(
        store.append(vec![]),
        Err(StorageError::EmptyBatch)
    ));
    let mut wrong_clock = input(3, 8);
    wrong_clock.times.known.clock = ClockId(9);
    assert!(matches!(
        store.append(vec![wrong_clock]),
        Err(StorageError::KnownClockMismatch { .. })
    ));
    let mut future_cause = input(4, 9);
    future_cause.causes.push(EventId {
        source: SourceId(1),
        epoch: SourceEpoch(1),
        sequence: 1,
    });
    assert!(matches!(
        store.append(vec![future_cause]),
        Err(StorageError::InvalidRecord(_))
    ));
    assert_eq!(fs::read(directory.0.join("events.wal")).unwrap(), before);
    assert_eq!(store.append(vec![input(0, 6)]).unwrap().first.sequence, 1);
}

#[test]
fn only_one_writer_can_open_and_existing_databases_are_not_overwritten() {
    let directory = TestDirectory::new();
    let store = create(&directory);
    assert!(matches!(
        Store::open(
            &directory.0,
            Limits::default(),
            RecoveryMode::RejectIncompleteTail
        ),
        Err(StorageError::WriterBusy)
    ));
    drop(store);
    assert!(matches!(
        Store::create(&directory.0, SourceId(2), SourceEpoch(2), Limits::default()),
        Err(StorageError::AlreadyExists)
    ));
    let reopened = open(&directory);
    assert_eq!(reopened.header().source, SourceId(1));
}

#[test]
fn incomplete_tail_requires_explicit_recovery_and_reports_discarded_bytes() {
    let directory = TestDirectory::new();
    let mut store = create(&directory);
    store.append(vec![input(1, 1)]).unwrap();
    let valid_length = store.wal_bytes();
    drop(store);
    OpenOptions::new()
        .append(true)
        .open(directory.0.join("events.wal"))
        .unwrap()
        .write_all(b"TNW")
        .unwrap();
    assert!(matches!(
        Store::open(
            &directory.0,
            Limits::default(),
            RecoveryMode::RejectIncompleteTail
        ),
        Err(StorageError::IncompleteTail { bytes: 3, .. })
    ));
    assert_eq!(
        fs::metadata(directory.0.join("events.wal")).unwrap().len(),
        valid_length + 3
    );
    let (mut store, report) = Store::open(
        &directory.0,
        Limits::default(),
        RecoveryMode::TruncateIncompleteTail,
    )
    .unwrap();
    assert_eq!(report.discarded_tail_bytes, 3);
    assert_eq!(report.recovered_events, 1);
    assert_eq!(store.append(vec![input(2, 2)]).unwrap().first.sequence, 1);
}

#[test]
fn complete_corruption_is_not_silently_truncated_even_in_recovery_mode() {
    let directory = TestDirectory::new();
    let mut store = create(&directory);
    store.append(vec![input(1, 1)]).unwrap();
    drop(store);
    let path = directory.0.join("events.wal");
    let mut bytes = fs::read(&path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0x80;
    fs::write(&path, &bytes).unwrap();
    assert!(matches!(
        Store::open(
            &directory.0,
            Limits::default(),
            RecoveryMode::TruncateIncompleteTail
        ),
        Err(StorageError::Format(_))
    ));
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

#[test]
fn every_partial_batch_boundary_preserves_the_previously_acknowledged_prefix() {
    let directory = TestDirectory::new();
    let mut store = create(&directory);
    store.append(vec![input(1, 1)]).unwrap();
    drop(store);
    let path = directory.0.join("events.wal");
    let prefix = fs::read(&path).unwrap();
    let source = input(2, 2);
    let event = StoredEvent {
        id: EventId {
            source: SourceId(1),
            epoch: SourceEpoch(1),
            sequence: 1,
        },
        entity: source.entity,
        times: source.times,
        schema: source.schema,
        payload: source.payload,
        causes: source.causes,
    };
    let frame = encode_batch(&[event], &Limits::default()).unwrap();
    for cut in 1..frame.len() {
        let mut interrupted = prefix.clone();
        interrupted.extend_from_slice(&frame[..cut]);
        fs::write(&path, &interrupted).unwrap();
        assert!(
            matches!(
                Store::open(
                    &directory.0,
                    Limits::default(),
                    RecoveryMode::RejectIncompleteTail
                ),
                Err(StorageError::IncompleteTail { .. })
            ),
            "cut={cut}"
        );
        let (recovered, report) = Store::open(
            &directory.0,
            Limits::default(),
            RecoveryMode::TruncateIncompleteTail,
        )
        .unwrap();
        assert_eq!(report.recovered_events, 1, "cut={cut}");
        assert_eq!(report.discarded_tail_bytes, cut as u64, "cut={cut}");
        drop(recovered);
        assert_eq!(fs::read(&path).unwrap(), prefix, "cut={cut}");
    }
}

#[test]
fn checksummed_but_noncontiguous_sequences_are_rejected() {
    let directory = TestDirectory::new();
    let store = create(&directory);
    drop(store);
    let source = input(1, 1);
    let record = StoredEvent {
        id: EventId {
            source: SourceId(1),
            epoch: SourceEpoch(1),
            sequence: 5,
        },
        entity: source.entity,
        times: source.times,
        schema: source.schema,
        payload: source.payload,
        causes: source.causes,
    };
    let bytes = encode_batch(&[record], &Limits::default()).unwrap();
    OpenOptions::new()
        .append(true)
        .open(directory.0.join("events.wal"))
        .unwrap()
        .write_all(&bytes)
        .unwrap();
    assert!(matches!(
        Store::open(
            &directory.0,
            Limits::default(),
            RecoveryMode::TruncateIncompleteTail
        ),
        Err(StorageError::InvalidRecord(_))
    ));
}

#[test]
fn storage_pagination_pins_snapshot_and_excludes_future_knowledge() {
    let directory = TestDirectory::new();
    let mut store = create(&directory);
    store
        .append(vec![input(10, 1), input(2, 2), input(1, 30)])
        .unwrap();
    let filter = HistoryFilter {
        known_as_of: Some(Timestamp::new(ClockId(2), 5)),
        ..HistoryFilter::default()
    };
    let budget = StorageQueryBudget {
        max_results: 1,
        ..StorageQueryBudget::default()
    };
    let first = store.history(filter, budget, None).unwrap();
    assert_eq!(first.events.len(), 1);
    assert_eq!(first.scanned, 3);
    let cursor = first.continuation;
    store.append(vec![input(11, 31)]).unwrap();
    let second = store.history(filter, budget, cursor).unwrap();
    assert_eq!(second.events[0].payload, 2u64.to_le_bytes());
    let third = store.history(filter, budget, second.continuation).unwrap();
    assert!(third.events.is_empty());
    assert!(third.continuation.is_none());
    assert!(matches!(
        store.history(HistoryFilter::default(), budget, cursor),
        Err(StorageError::InvalidCursor)
    ));
    assert!(matches!(
        store.history(
            filter,
            StorageQueryBudget {
                max_scanned: 1,
                ..budget
            },
            None
        ),
        Err(StorageError::BudgetTooSmall { .. })
    ));
}

#[test]
fn cursor_cannot_be_used_on_another_database() {
    let first_dir = TestDirectory::new();
    let second_dir = TestDirectory::new();
    let mut first = create(&first_dir);
    let mut second = create(&second_dir);
    first.append(vec![input(1, 1), input(2, 2)]).unwrap();
    second.append(vec![input(1, 1), input(2, 2)]).unwrap();
    let budget = StorageQueryBudget {
        max_results: 1,
        ..StorageQueryBudget::default()
    };
    let page = first
        .history(HistoryFilter::default(), budget, None)
        .unwrap();
    assert!(matches!(
        second.history(HistoryFilter::default(), budget, page.continuation),
        Err(StorageError::InvalidCursor)
    ));
}

#[test]
fn explicit_sequence_cursors_skip_old_batches_and_pin_the_durable_prefix() {
    let directory = TestDirectory::new();
    let mut store = create(&directory);
    store.append(vec![input(1, 1), input(2, 2)]).unwrap();
    store.append(vec![input(3, 3), input(4, 4)]).unwrap();
    let filter = HistoryFilter::default();
    let cursor = store.cursor_from_sequence(2, filter).unwrap();
    store.append(vec![input(5, 5)]).unwrap();
    let page = store
        .history(filter, StorageQueryBudget::default(), Some(cursor))
        .unwrap();
    assert_eq!(page.scanned, 2);
    assert_eq!(
        page.events
            .iter()
            .map(|event| event.id.sequence)
            .collect::<Vec<_>>(),
        vec![2, 3]
    );
    assert!(page.continuation.is_none());
    assert!(matches!(
        store.cursor_from_sequence(6, filter),
        Err(StorageError::InvalidCursor)
    ));
}

#[test]
fn seal_produces_idempotent_independent_segments_without_retiring_the_wal() {
    let directory = TestDirectory::new();
    let mut store = create(&directory);
    store.append(vec![input(1, 1), input(2, 2)]).unwrap();
    store.append(vec![input(3, 3)]).unwrap();
    let wal_before = fs::read(directory.0.join("events.wal")).unwrap();
    let report = store.seal().unwrap();
    assert_eq!(report.segments_created, 2);
    assert_eq!(report.events, 3);
    assert_eq!(
        fs::read(directory.0.join("events.wal")).unwrap(),
        wal_before
    );
    let path = directory
        .0
        .join("segments")
        .join(format!("{:020}-{:020}.tsf", 0, 1));
    let segment = decode_segment(
        &read_bounded_file(&path, Limits::default().max_segment_bytes).unwrap(),
        &Limits::default(),
    )
    .unwrap();
    assert_eq!(segment.header, store.header());
    assert_eq!(segment.records.len(), 2);
    let again = store.seal().unwrap();
    assert_eq!(again.segments_created, 0);
    assert_eq!(again.segments_existing, 2);
    drop(store);
    assert_eq!(open(&directory).len(), 3);
}

#[test]
fn an_existing_segment_is_never_overwritten() {
    let directory = TestDirectory::new();
    let mut store = create(&directory);
    store.append(vec![input(1, 1)]).unwrap();
    store.seal().unwrap();
    let path = directory
        .0
        .join("segments")
        .join(format!("{:020}-{:020}.tsf", 0, 0));
    fs::write(&path, b"not the immutable segment").unwrap();
    assert!(matches!(store.seal(), Err(StorageError::InvalidRecord(_))));
    assert_eq!(fs::read(path).unwrap(), b"not the immutable segment");
}

#[test]
fn truncated_headers_and_oversized_payloads_are_errors() {
    let directory = TestDirectory::new();
    let limits = Limits {
        max_payload_bytes: 2,
        ..Limits::default()
    };
    let mut store = Store::create(&directory.0, SourceId(1), SourceEpoch(1), limits).unwrap();
    assert!(matches!(
        store.append(vec![input(1, 1)]),
        Err(StorageError::Format(_))
    ));
    assert!(store.is_empty());
    drop(store);
    let file = OpenOptions::new()
        .write(true)
        .open(directory.0.join("events.wal"))
        .unwrap();
    file.set_len(3).unwrap();
    drop(file);
    assert!(matches!(
        Store::open(
            &directory.0,
            Limits::default(),
            RecoveryMode::TruncateIncompleteTail
        ),
        Err(StorageError::Format(_))
    ));
}

#[test]
fn external_file_changes_poison_the_writer_instead_of_reusing_ids() {
    let directory = TestDirectory::new();
    let mut store = create(&directory);
    let mut file = OpenOptions::new()
        .write(true)
        .open(directory.0.join("events.wal"))
        .unwrap();
    file.seek(SeekFrom::End(0)).unwrap();
    file.write_all(b"external bytes").unwrap();
    drop(file);
    assert!(matches!(
        store.append(vec![input(1, 1)]),
        Err(StorageError::CommitOutcomeUnknown(_))
    ));
    assert!(matches!(
        store.append(vec![input(1, 1)]),
        Err(StorageError::Poisoned)
    ));
}

#[test]
fn acknowledged_records_survive_process_exit_without_destructors() {
    let directory = TestDirectory::new();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "crash_process_helper",
            "--ignored",
            "--nocapture",
        ])
        .env("TEMNION_STORAGE_CRASH_DIR", &directory.0)
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(86),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut store = open(&directory);
    assert_eq!(store.len(), 1);
    assert_eq!(store.append(vec![input(2, 2)]).unwrap().first.sequence, 1);
}

#[test]
fn seal_produces_companion_tsm_summaries_and_prunes_queries() {
    use temnion_storage::read_segment_summary;

    let directory = TestDirectory::new();
    let mut store = create(&directory);

    // Append batch 0 with entity 10
    store
        .append(vec![WriteEvent {
            entity: EntityId {
                shard: ShardId(0),
                slot: 10,
                generation: 1,
            },
            times: EventTimes {
                valid: Timestamp::new(ClockId(1), 100),
                observed: None,
                known: Timestamp::new(ClockId(2), 100),
            },
            schema: SchemaId(1),
            payload: vec![1, 2, 3],
            causes: Vec::new(),
        }])
        .unwrap();

    // Append batch 1 with entity 20
    store
        .append(vec![WriteEvent {
            entity: EntityId {
                shard: ShardId(0),
                slot: 20,
                generation: 1,
            },
            times: EventTimes {
                valid: Timestamp::new(ClockId(1), 200),
                observed: None,
                known: Timestamp::new(ClockId(2), 200),
            },
            schema: SchemaId(1),
            payload: vec![4, 5, 6],
            causes: Vec::new(),
        }])
        .unwrap();

    // Seal both batches into segments
    let report = store.seal().unwrap();
    assert_eq!(report.segments_created, 2);

    // Verify .tsm companion summary files exist and can be read
    let summary0_path = directory
        .0
        .join("segments")
        .join(format!("{:020}-{:020}.tsm", 0, 0));
    let summary1_path = directory
        .0
        .join("segments")
        .join(format!("{:020}-{:020}.tsm", 1, 1));
    assert!(summary0_path.exists());
    assert!(summary1_path.exists());

    let summary0 = read_segment_summary(&summary0_path).unwrap();
    assert_eq!(summary0.sequence_range.min, 0);
    assert_eq!(summary0.sequence_range.max, 0);
    assert_eq!(summary0.blocks[0].entity_range.min.slot, 10);

    let summary1 = read_segment_summary(&summary1_path).unwrap();
    assert_eq!(summary1.sequence_range.min, 1);
    assert_eq!(summary1.sequence_range.max, 1);
    assert_eq!(summary1.blocks[0].entity_range.min.slot, 20);

    // History query filtering by entity 20 should skip batch 0 completely
    let filter_entity20 = HistoryFilter {
        entity: Some(EntityId {
            shard: ShardId(0),
            slot: 20,
            generation: 1,
        }),
        time: None,
        known_as_of: None,
    };
    let page = store
        .history(filter_entity20, StorageQueryBudget::default(), None)
        .unwrap();
    assert_eq!(page.events.len(), 1);
    assert_eq!(page.events[0].id.sequence, 1);
}

#[test]
#[ignore = "subprocess fixture; called by acknowledged_records_survive_process_exit_without_destructors"]
fn crash_process_helper() {
    let directory = std::env::var_os("TEMNION_STORAGE_CRASH_DIR").expect("subprocess directory");
    let mut store =
        Store::create(directory, SourceId(1), SourceEpoch(1), Limits::default()).unwrap();
    store.append(vec![input(1, 1)]).unwrap();
    std::process::exit(86);
}
