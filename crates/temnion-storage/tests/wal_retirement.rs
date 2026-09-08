// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]

use std::fs;
use std::path::PathBuf;

use temnion_core::{
    ClockId, EntityId, EventId, EventTimes, SchemaId, ShardId, SourceEpoch, SourceId, Timestamp,
};
use temnion_events::HistoryFilter;
use temnion_format::Limits;
use temnion_storage::lifecycle::ReferenceHold;
use temnion_storage::manifest::SegmentManifest;
use temnion_storage::{RecoveryMode, StorageQueryBudget, Store, WriteEvent};

struct TestEnv {
    path: PathBuf,
}

impl TestEnv {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "temnion_test_wal_retire_{name}_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn create_write_event(slot: u32, ticks: u64, payload_tag: u8) -> WriteEvent {
    WriteEvent {
        entity: EntityId {
            shard: ShardId(0),
            slot,
            generation: 0,
        },
        schema: SchemaId(1),
        times: EventTimes {
            valid: Timestamp::new(ClockId(1), ticks),
            observed: None,
            known: Timestamp::new(ClockId(1), ticks),
        },
        causes: Vec::new(),
        payload: vec![payload_tag, (ticks & 0xFF) as u8],
    }
}

#[test]
fn test_manifest_creation_and_registration_on_seal() {
    let env = TestEnv::new("manifest_seal");
    let mut store =
        Store::create(&env.path, SourceId(1), SourceEpoch(1), Limits::default()).unwrap();

    let b1 = vec![
        create_write_event(10, 100, 1),
        create_write_event(10, 101, 2),
    ];
    let b2 = vec![
        create_write_event(10, 102, 3),
        create_write_event(10, 103, 4),
    ];

    store.append(b1).unwrap();
    store.append(b2).unwrap();

    let seal_report = store.seal().unwrap();
    assert_eq!(seal_report.segments_created, 2);

    let manifest_path = env.path.join("segments").join("manifest.bin");
    assert!(manifest_path.exists());

    let manifest = SegmentManifest::load_if_exists(&manifest_path)
        .unwrap()
        .expect("manifest exists");
    assert_eq!(manifest.sealed_segments.len(), 2);
    assert_eq!(manifest.sealed_segments[0].first_sequence, 0);
    assert_eq!(manifest.sealed_segments[0].last_sequence, 1);
    assert_eq!(manifest.sealed_segments[1].first_sequence, 2);
    assert_eq!(manifest.sealed_segments[1].last_sequence, 3);
    assert_eq!(manifest.retired_up_to_sequence, None);
}

#[test]
fn test_retire_wal_respects_reference_holds_and_enables_seamless_historical_queries() {
    let env = TestEnv::new("retire_holds");
    let mut store =
        Store::create(&env.path, SourceId(1), SourceEpoch(1), Limits::default()).unwrap();

    // 3 batches: [0..=1], [2..=3], [4..=5]
    let b1 = vec![
        create_write_event(10, 100, 10),
        create_write_event(10, 101, 11),
    ];
    let b2 = vec![
        create_write_event(10, 102, 12),
        create_write_event(10, 103, 13),
    ];
    let b3 = vec![
        create_write_event(10, 104, 14),
        create_write_event(10, 105, 15),
    ];

    store.append(b1).unwrap();
    store.append(b2).unwrap();
    store.append(b3).unwrap();

    // Seal into segments
    let seal_report = store.seal().unwrap();
    assert_eq!(seal_report.segments_created, 3);

    // Initial WAL size
    let wal_path = env.path.join("events.wal");
    let initial_wal_len = fs::metadata(&wal_path).unwrap().len();

    // 1. Apply reference hold covering sequence 3 (in batch 2)
    let mut hold = ReferenceHold::new("hold_seq_3", "critical audit checkpoint", 103);
    hold.hold_sequence(3);

    // Run retirement with active hold
    let r1 = store.retire_wal(&[hold.clone()]).unwrap();
    // Only batch 1 (seq 0..=1) should be retired, because batch 2 contains held seq 3!
    assert_eq!(r1.frames_retired, 1);
    assert_eq!(r1.events_retired, 2);
    assert_eq!(r1.retired_up_to, Some(1));

    let wal_len_after_r1 = fs::metadata(&wal_path).unwrap().len();
    assert!(wal_len_after_r1 < initial_wal_len);

    // Verify historical query for retired event (seq 1) still works seamlessly from sealed segment
    let ev1 = store
        .get(EventId {
            source: SourceId(1),
            epoch: SourceEpoch(1),
            sequence: 1,
        })
        .unwrap();
    assert_eq!(ev1.id.sequence, 1);
    assert_eq!(ev1.payload, vec![11, 101]);

    // Verify active WAL query for unretired event (seq 3) works
    let ev3 = store
        .get(EventId {
            source: SourceId(1),
            epoch: SourceEpoch(1),
            sequence: 3,
        })
        .unwrap();
    assert_eq!(ev3.id.sequence, 3);

    // Verify history pagination traverses across retired segment and active WAL
    let page = store
        .history(
            HistoryFilter::default(),
            StorageQueryBudget {
                max_results: 10,
                max_scanned: 10,
                max_read_bytes: 1_000_000,
            },
            None,
        )
        .unwrap();
    assert_eq!(page.events.len(), 6);
    for i in 0..6 {
        assert_eq!(page.events[i].id.sequence, i as u64);
    }

    // 2. Release hold and retire remaining batches
    let r2 = store.retire_wal(&[]).unwrap();
    assert_eq!(r2.frames_retired, 2);
    assert_eq!(r2.events_retired, 4);
    assert_eq!(r2.retired_up_to, Some(5));

    let wal_len_after_r2 = fs::metadata(&wal_path).unwrap().len();
    // WAL now contains only the header
    assert_eq!(wal_len_after_r2, temnion_format::WAL_HEADER_LEN as u64);

    // Drop store to release lock
    drop(store);

    // 3. Reopen store from disk and verify clean recovery
    let (mut reopened, recovery_report) = Store::open(
        &env.path,
        Limits::default(),
        RecoveryMode::RejectIncompleteTail,
    )
    .unwrap();
    assert_eq!(recovery_report.discarded_tail_bytes, 0);

    // Verify all 6 events still readable through history
    let page_reopened = reopened
        .history(
            HistoryFilter::default(),
            StorageQueryBudget {
                max_results: 10,
                max_scanned: 10,
                max_read_bytes: 1_000_000,
            },
            None,
        )
        .unwrap();
    assert_eq!(page_reopened.events.len(), 6);

    // 4. Append new batch to reopened store (should continue at sequence 6)
    let b4 = vec![
        create_write_event(10, 106, 16),
        create_write_event(10, 107, 17),
    ];
    reopened.append(b4).unwrap();

    let ev6 = reopened
        .get(EventId {
            source: SourceId(1),
            epoch: SourceEpoch(1),
            sequence: 6,
        })
        .unwrap();
    assert_eq!(ev6.id.sequence, 6);
    assert_eq!(ev6.payload, vec![16, 106]);
}
