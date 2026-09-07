// SPDX-License-Identifier: AGPL-3.0-only
use std::fs;
use std::path::PathBuf;

use temnion_branch::{BranchLifecycle, BranchManager};
use temnion_core::{
    BranchId, ClockId, EntityId, EventTimes, SchemaId, ShardId, SourceEpoch, SourceId, Timestamp,
};
use temnion_format::Limits;
use temnion_replay::{RawEntityReducer, decode_raw_entity_map};
use temnion_storage::{Store, WriteEvent};

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let mut bytes = [0u8; 8];
        getrandom::getrandom(&mut bytes).unwrap();
        let name = format!("temnion-branch-test-{:016x}", u64::from_le_bytes(bytes));
        let path = std::env::temp_dir().join(name);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn sample_write(shard: u32, slot: u32, generation: u32, payload: Vec<u8>) -> WriteEvent {
    WriteEvent {
        entity: EntityId {
            shard: ShardId(shard),
            slot,
            generation,
        },
        schema: SchemaId(1),
        times: EventTimes {
            valid: Timestamp::new(ClockId(1), 100),
            observed: None,
            known: Timestamp::new(ClockId(2), 200),
        },
        payload,
        causes: Vec::new(),
    }
}

#[test]
fn branch_manager_initializes_main_branch() {
    let dir = TestDir::new();
    let limits = Limits::default();
    Store::create(&dir.0, SourceId(1), SourceEpoch(1), limits).unwrap();

    let mgr = BranchManager::open(&dir.0).unwrap();
    assert_eq!(mgr.manifest().branches.len(), 1);
    let main = mgr.manifest().branches.get(&BranchId(0)).unwrap();
    assert_eq!(main.name, "main");
    assert_eq!(main.parent, None);
    assert_eq!(main.lifecycle, BranchLifecycle::Active);
}

#[test]
fn branch_forking_and_structural_sharing() {
    let dir = TestDir::new();
    let limits = Limits::default();
    let mut store = Store::create(&dir.0, SourceId(1), SourceEpoch(1), limits).unwrap();

    // Populate main branch with 5 events (seq 0..4)
    for i in 0..5 {
        store
            .append(vec![sample_write(0, i, 1, vec![i as u8])])
            .unwrap();
    }
    drop(store);

    let mut mgr = BranchManager::open(&dir.0).unwrap();

    // Fork branch 1 from main at sequence 2
    let b1 = mgr
        .create_fork(
            BranchId(0),
            "counterfactual-alpha".into(),
            2,
            BranchLifecycle::Candidate,
        )
        .unwrap();
    assert_eq!(b1, BranchId(1));

    // Fork branch 2 from branch 1 at sequence 2
    let b2 = mgr
        .create_fork(
            b1,
            "nested-experiment".into(),
            2,
            BranchLifecycle::Temporary,
        )
        .unwrap();
    assert_eq!(b2, BranchId(2));

    // Verify timeline segments
    let t_main = mgr.resolve_timeline(BranchId(0)).unwrap();
    assert_eq!(t_main.len(), 1);
    assert_eq!(t_main[0].branch_id, BranchId(0));
    assert_eq!(t_main[0].start_seq, 0);
    assert_eq!(t_main[0].end_seq, None);

    let t_b1 = mgr.resolve_timeline(b1).unwrap();
    assert_eq!(t_b1.len(), 2);
    assert_eq!(t_b1[0].branch_id, BranchId(0));
    assert_eq!(t_b1[0].start_seq, 0);
    assert_eq!(t_b1[0].end_seq, Some(2));
    assert_eq!(t_b1[1].branch_id, b1);
    assert_eq!(t_b1[1].start_seq, 3);
    assert_eq!(t_b1[1].end_seq, None);

    let t_b2 = mgr.resolve_timeline(b2).unwrap();
    assert_eq!(t_b2.len(), 3);
    assert_eq!(t_b2[0].branch_id, BranchId(0));
    assert_eq!(t_b2[0].start_seq, 0);
    assert_eq!(t_b2[0].end_seq, Some(2));
    assert_eq!(t_b2[1].branch_id, b1);
    assert_eq!(t_b2[1].start_seq, 3);
    assert_eq!(t_b2[1].end_seq, Some(2));
    assert_eq!(t_b2[2].branch_id, b2);
    assert_eq!(t_b2[2].start_seq, 3);
    assert_eq!(t_b2[2].end_seq, None);
}

#[test]
fn divergent_appends_and_reconstruction() {
    let dir = TestDir::new();
    let limits = Limits::default();
    let mut store = Store::create(&dir.0, SourceId(1), SourceEpoch(1), limits).unwrap();

    let e_common = EntityId {
        shard: ShardId(0),
        slot: 1,
        generation: 1,
    };
    store
        .append(vec![WriteEvent {
            entity: e_common,
            schema: SchemaId(1),
            times: EventTimes {
                valid: Timestamp::new(ClockId(1), 10),
                observed: None,
                known: Timestamp::new(ClockId(2), 20),
            },
            payload: vec![100],
            causes: Vec::new(),
        }])
        .unwrap();
    drop(store);

    let mut mgr = BranchManager::open(&dir.0).unwrap();

    // Fork branch 1 at seq 0
    let b1 = mgr
        .create_fork(
            BranchId(0),
            "divergent-path".into(),
            0,
            BranchLifecycle::Active,
        )
        .unwrap();

    // Reconstruct at sequence 0 on child branch: should read common event from parent
    let (state_at_0, header_at_0) = mgr
        .reconstruct_at(b1, 0, &mut RawEntityReducer, decode_raw_entity_map)
        .unwrap();
    assert_eq!(header_at_0.sequence, 0);
    assert_eq!(state_at_0.get(&e_common).unwrap().1, vec![100]);

    // Append divergent event to child branch
    let e_child = EntityId {
        shard: ShardId(0),
        slot: 2,
        generation: 1,
    };
    mgr.append_to_branch(
        b1,
        vec![WriteEvent {
            entity: e_child,
            schema: SchemaId(1),
            times: EventTimes {
                valid: Timestamp::new(ClockId(1), 30),
                observed: None,
                known: Timestamp::new(ClockId(2), 40),
            },
            payload: vec![200],
            causes: Vec::new(),
        }],
    )
    .unwrap();

    // Reconstruct at sequence 0 on child store
    let (state_child, _) = mgr
        .reconstruct_at(b1, 0, &mut RawEntityReducer, decode_raw_entity_map)
        .unwrap();
    assert_eq!(state_child.get(&e_common).unwrap().1, vec![100]);
}

#[test]
fn branch_lifecycle_transitions_and_persistence() {
    let dir = TestDir::new();
    let limits = Limits::default();
    let mut store = Store::create(&dir.0, SourceId(1), SourceEpoch(1), limits).unwrap();
    store.append(vec![sample_write(0, 1, 1, vec![42])]).unwrap();
    drop(store);

    let mut mgr = BranchManager::open(&dir.0).unwrap();
    let b1 = mgr
        .create_fork(
            BranchId(0),
            "candidate-run".into(),
            0,
            BranchLifecycle::Candidate,
        )
        .unwrap();

    // Promote branch
    mgr.set_lifecycle(b1, BranchLifecycle::Promoted).unwrap();
    assert_eq!(
        mgr.manifest().branches.get(&b1).unwrap().lifecycle,
        BranchLifecycle::Promoted
    );

    // Reopen manager and verify persistence
    drop(mgr);
    let mut reopened = BranchManager::open(&dir.0).unwrap();
    assert_eq!(
        reopened.manifest().branches.get(&b1).unwrap().lifecycle,
        BranchLifecycle::Promoted
    );

    // Retire branch
    reopened
        .set_lifecycle(b1, BranchLifecycle::Retired)
        .unwrap();
    assert_eq!(
        reopened.manifest().branches.get(&b1).unwrap().lifecycle,
        BranchLifecycle::Retired
    );

    // Attempting to fork from or append to a retired branch fails
    assert!(
        reopened
            .create_fork(b1, "should-fail".into(), 0, BranchLifecycle::Temporary)
            .is_err()
    );
    assert!(
        reopened
            .append_to_branch(b1, vec![sample_write(0, 2, 1, vec![1])])
            .is_err()
    );
}
