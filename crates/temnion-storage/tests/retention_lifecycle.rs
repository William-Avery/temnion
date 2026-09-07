// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use temnion_core::{
    ClockId, EntityId, EventTimes, SchemaId, ShardId, SourceEpoch, SourceId, Timestamp,
};
use temnion_format::Limits;
use temnion_storage::lifecycle::{BackupManager, ReferenceHold, RetentionPolicy};
use temnion_storage::{Store, WriteEvent};

fn temp_paths() -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let pid = std::process::id();
    let base = std::env::temp_dir();
    let store_dir = base.join(format!("temnion-store-test-{pid}-{nonce}"));
    let backup_dir = base.join(format!("temnion-backup-test-{pid}-{nonce}"));
    let restore_dir = base.join(format!("temnion-restore-test-{pid}-{nonce}"));
    (store_dir, backup_dir, restore_dir)
}

#[test]
fn reference_holds_strictly_prevent_retention_reclamation() {
    let (store_dir, _, _) = temp_paths();
    let mut store =
        Store::create(&store_dir, SourceId(1), SourceEpoch(1), Limits::default()).unwrap();

    let mut events = Vec::new();
    for i in 1..=20 {
        events.push(WriteEvent {
            entity: EntityId {
                shard: ShardId(0),
                slot: i as u32,
                generation: 0,
            },
            times: EventTimes {
                valid: Timestamp::new(ClockId(1), i * 10),
                observed: None,
                known: Timestamp::new(ClockId(1), i * 10),
            },
            schema: SchemaId(1),
            payload: vec![i as u8],
            causes: Vec::new(),
        });
    }
    store.append(events).unwrap();

    let page = store
        .history(
            temnion_events::HistoryFilter::default(),
            temnion_storage::StorageQueryBudget {
                max_results: 100,
                max_scanned: 1000,
                max_read_bytes: 1024 * 1024,
            },
            None,
        )
        .unwrap();

    // Set retention policy: max age 50 ticks (older than 150 ticks at current time 200)
    let policy = RetentionPolicy {
        max_age_ticks: Some(50),
        max_retained_events: Some(5),
    };

    // Hold event sequences 2 and 3 explicitly
    let mut hold = ReferenceHold::new("audit-hold-01", "Preserve early evidence", 200);
    hold.hold_sequence(2);
    hold.hold_sequence(3);

    let reclaimable = policy.evaluate_reclaimable(&page.events, 200, &[hold]);

    // Sequences 2 and 3 must NOT be present in reclaimable list!
    assert!(!reclaimable.contains(&2));
    assert!(!reclaimable.contains(&3));
    // Sequence 1 should be eligible
    assert!(reclaimable.contains(&1));

    drop(store);
    let _ = fs::remove_dir_all(store_dir);
}

#[test]
fn backup_create_verify_and_restore_cycle() {
    let (store_dir, backup_dir, restore_dir) = temp_paths();

    // 1. Create source store and append events
    let mut store =
        Store::create(&store_dir, SourceId(42), SourceEpoch(7), Limits::default()).unwrap();
    let events = (1..=15)
        .map(|i| WriteEvent {
            entity: EntityId {
                shard: ShardId(0),
                slot: i,
                generation: 0,
            },
            times: EventTimes {
                valid: Timestamp::new(ClockId(1), i as u64),
                observed: None,
                known: Timestamp::new(ClockId(1), i as u64),
            },
            schema: SchemaId(1),
            payload: vec![0xAA, i as u8],
            causes: Vec::new(),
        })
        .collect();
    store.append(events).unwrap();
    drop(store);

    // 2. Create point-in-time backup
    let manifest = BackupManager::create_backup(&store_dir, &backup_dir).unwrap();
    assert!(!manifest.files.is_empty());

    // 3. Verify backup integrity
    let verified = BackupManager::verify_backup(&backup_dir).unwrap();
    assert_eq!(verified.files.len(), manifest.files.len());

    // 4. Restore backup into target directory
    let mut restored_store = BackupManager::restore_backup(&backup_dir, &restore_dir).unwrap();
    assert_eq!(restored_store.len(), 15);
    assert_eq!(restored_store.header().source.0, 42);
    assert_eq!(restored_store.header().epoch.0, 7);

    // 5. Append new event to restored store
    let receipt = restored_store
        .append(vec![WriteEvent {
            entity: EntityId {
                shard: ShardId(0),
                slot: 99,
                generation: 0,
            },
            times: EventTimes {
                valid: Timestamp::new(ClockId(1), 16),
                observed: None,
                known: Timestamp::new(ClockId(1), 16),
            },
            schema: SchemaId(1),
            payload: vec![0xFF],
            causes: Vec::new(),
        }])
        .unwrap();
    assert_eq!(receipt.count, 1);
    assert_eq!(restored_store.len(), 16);

    drop(restored_store);

    // 6. Test corruption detection: modify a byte in backup and verify it fails
    let wal_path = backup_dir.join("events.wal");
    let mut f = OpenOptions::new().write(true).open(&wal_path).unwrap();
    f.write_all(b"CORRUPT").unwrap();
    drop(f);

    assert!(BackupManager::verify_backup(&backup_dir).is_err());

    let _ = fs::remove_dir_all(store_dir);
    let _ = fs::remove_dir_all(backup_dir);
    let _ = fs::remove_dir_all(restore_dir);
}
