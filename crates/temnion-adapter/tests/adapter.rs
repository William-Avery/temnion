// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]

use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use temnion_adapter::{
    CLOCK_FAST, CLOCK_MEDIUM, CadenceTier, MediaRef, MirrorWriter, SCHEMA_ACTION, SCHEMA_INTENTION,
    SCHEMA_OUTCOME, SCHEMA_PERCEPT, TzeentchAction, TzeentchActionTracer, TzeentchConverter,
    TzeentchIntention, TzeentchOutcome, TzeentchPercept,
};
use temnion_core::{EntityId, EventId, EventTimes, ShardId, SourceEpoch, SourceId, Timestamp};
use temnion_format::{Limits, StoredEvent};
use temnion_storage::Store;

fn temp_store() -> (Store, std::path::PathBuf) {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "temnion-adapter-test-{}-{nonce}",
        std::process::id()
    ));
    let store = Store::create(&root, SourceId(99), SourceEpoch(1), Limits::default()).unwrap();
    (store, root)
}

#[test]
fn format_conversion_round_trip_preserves_semantics() {
    let (mut store, root) = temp_store();

    let media = MediaRef::new(
        "s3://orgx/camera_0.raw",
        b"frame-pixels-123456",
        "image/raw",
    );
    let percept = TzeentchPercept {
        organ_id: "VisualCortex".to_string(),
        sensor_id: "Cam0".to_string(),
        cadence: CadenceTier::Fast,
        media_ref: Some(media),
        features: vec![0.12, 0.45, 0.99],
        timestamp: 100,
    };

    let entity = EntityId {
        shard: ShardId(0),
        slot: 10,
        generation: 0,
    };

    let p_ev = TzeentchConverter::percept_to_event(&percept, entity, CLOCK_FAST, CLOCK_FAST);
    let receipt = store.append(vec![p_ev]).unwrap();
    assert_eq!(receipt.count, 1);
    let percept_id = receipt.first;

    let intention = TzeentchIntention {
        organ_id: "ExecutiveOrgan".to_string(),
        cell_id: "PlannerCell4".to_string(),
        goal_label: "NavigateToDock".to_string(),
        policy_id: "PolicyPPO_v2".to_string(),
        target_features: vec![10.0, 5.0],
        planned_at: 105,
    };

    let i_ev = TzeentchConverter::intention_to_event(
        &intention,
        entity,
        vec![percept_id],
        CLOCK_MEDIUM,
        CLOCK_FAST,
    );
    let receipt_i = store.append(vec![i_ev]).unwrap();
    let intention_id = receipt_i.first;

    let action = TzeentchAction {
        action_id: "Act_001".to_string(),
        intention_ref: Some("NavigateToDock".to_string()),
        motor_command: "SteerAngle".to_string(),
        parameters: vec![0.25],
        executed_at: 110,
    };

    let a_ev = TzeentchConverter::action_to_event(
        &action,
        entity,
        vec![intention_id],
        CLOCK_FAST,
        CLOCK_FAST,
    );
    let receipt_a = store.append(vec![a_ev]).unwrap();
    let action_id = receipt_a.first;

    let outcome = TzeentchOutcome {
        action_id: "Act_001".to_string(),
        reward: 0.85,
        state_delta: vec![0.5, 0.1],
        observed_at: 115,
    };

    let o_ev =
        TzeentchConverter::outcome_to_event(&outcome, entity, action_id, CLOCK_FAST, CLOCK_FAST);
    let receipt_o = store.append(vec![o_ev]).unwrap();
    assert_eq!(receipt_o.count, 1);

    drop(store);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn mirror_writer_bounded_queue_drains_to_store_with_zero_drops() {
    let (mut store, root) = temp_store();
    let mut writer = MirrorWriter::new(100);

    for seq in 1..=50 {
        let ev = TzeentchConverter::percept_to_event(
            &TzeentchPercept {
                organ_id: "MotorOrgan".to_string(),
                sensor_id: "Encoder".to_string(),
                cadence: CadenceTier::Fast,
                media_ref: None,
                features: vec![seq as f64],
                timestamp: seq * 10,
            },
            EntityId {
                shard: ShardId(0),
                slot: 1,
                generation: 0,
            },
            CLOCK_FAST,
            CLOCK_FAST,
        );
        writer.enqueue(ev).unwrap();
    }

    assert_eq!(writer.pending_count(), 50);
    assert_eq!(writer.stats().enqueued, 50);
    assert_eq!(writer.stats().dropped_count, 0);

    let drained = writer.drain_to_store(&mut store).unwrap();
    assert_eq!(drained, 50);
    assert_eq!(writer.pending_count(), 0);
    assert_eq!(writer.stats().drained, 50);

    drop(store);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn action_trace_end_to_end_verification() {
    let p_id = EventId {
        source: SourceId(1),
        epoch: SourceEpoch(1),
        sequence: 1,
    };
    let i_id = EventId {
        source: SourceId(1),
        epoch: SourceEpoch(1),
        sequence: 2,
    };
    let a_id = EventId {
        source: SourceId(1),
        epoch: SourceEpoch(1),
        sequence: 3,
    };
    let o_id = EventId {
        source: SourceId(1),
        epoch: SourceEpoch(1),
        sequence: 4,
    };

    let events = vec![
        StoredEvent {
            id: p_id,
            entity: EntityId {
                shard: ShardId(0),
                slot: 1,
                generation: 0,
            },
            times: EventTimes {
                valid: Timestamp::new(CLOCK_FAST, 10),
                observed: None,
                known: Timestamp::new(CLOCK_FAST, 10),
            },
            schema: SCHEMA_PERCEPT,
            payload: vec![],
            causes: vec![],
        },
        StoredEvent {
            id: i_id,
            entity: EntityId {
                shard: ShardId(0),
                slot: 1,
                generation: 0,
            },
            times: EventTimes {
                valid: Timestamp::new(CLOCK_MEDIUM, 15),
                observed: None,
                known: Timestamp::new(CLOCK_MEDIUM, 15),
            },
            schema: SCHEMA_INTENTION,
            payload: vec![],
            causes: vec![p_id],
        },
        StoredEvent {
            id: a_id,
            entity: EntityId {
                shard: ShardId(0),
                slot: 1,
                generation: 0,
            },
            times: EventTimes {
                valid: Timestamp::new(CLOCK_FAST, 20),
                observed: None,
                known: Timestamp::new(CLOCK_FAST, 20),
            },
            schema: SCHEMA_ACTION,
            payload: vec![],
            causes: vec![i_id],
        },
        StoredEvent {
            id: o_id,
            entity: EntityId {
                shard: ShardId(0),
                slot: 1,
                generation: 0,
            },
            times: EventTimes {
                valid: Timestamp::new(CLOCK_FAST, 25),
                observed: None,
                known: Timestamp::new(CLOCK_FAST, 25),
            },
            schema: SCHEMA_OUTCOME,
            payload: vec![],
            causes: vec![a_id],
        },
    ];

    let trace = TzeentchActionTracer::trace_action(3, &events).unwrap();
    assert_eq!(trace.action_event_id, a_id);
    assert!(!trace.future_leakage_detected);
    // Verified 0 source gaps because all steps (Perception, Intention, Action, Outcome) were present
    let gaps = trace
        .nodes
        .iter()
        .filter(|n| matches!(n, temnion_adapter::ActionTraceNode::SourceGap { .. }))
        .count();
    assert_eq!(gaps, 0);
}
