// SPDX-License-Identifier: AGPL-3.0-only
use std::fs;
use std::path::PathBuf;

use temnion_core::{
    ClockId, DatabaseId, EntityId, EventTimes, FieldId, SchemaId, ShardId, SourceEpoch, SourceId,
    Timestamp,
};
use temnion_format::Limits;
use temnion_replay::{
    Checkpoint, CheckpointHeader, DeterministicRng, RawEntityMap, RawEntityReducer, ReplayEngine,
    ReplayError, RngSnapshot, SchemaStateReducer, decode_checkpoint, decode_entity_map,
    decode_raw_entity_map, encode_checkpoint, encode_entity_map, encode_raw_entity_map,
};
use temnion_schema::{Field, FieldType, FieldUpdate, Mutation, Schema, Value, encode_mutation};
use temnion_storage::{Store, WriteEvent};

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let mut bytes = [0u8; 8];
        getrandom::getrandom(&mut bytes).unwrap();
        let name = format!("temnion-replay-test-{:016x}", u64::from_le_bytes(bytes));
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

fn sample_schema() -> Schema {
    Schema::new(
        SchemaId(1),
        1,
        vec![
            Field {
                id: FieldId(0),
                name: "health".into(),
                kind: FieldType::U64,
                nullable: false,
                unit: Some("points".into()),
            },
            Field {
                id: FieldId(1),
                name: "active".into(),
                kind: FieldType::Bool,
                nullable: false,
                unit: None,
            },
        ],
    )
    .unwrap()
}

fn sample_write_event(entity: EntityId, seq: u64, health: u64, active: bool) -> WriteEvent {
    let mutation = Mutation::Upsert(vec![
        FieldUpdate {
            field: FieldId(0),
            value: Value::U64(health),
        },
        FieldUpdate {
            field: FieldId(1),
            value: Value::Bool(active),
        },
    ]);
    let payload = encode_mutation(&mutation, 1024).unwrap();
    WriteEvent {
        entity,
        times: EventTimes {
            valid: Timestamp::new(ClockId(1), seq * 10),
            observed: None,
            known: Timestamp::new(ClockId(2), seq * 10),
        },
        schema: SchemaId(1),
        payload,
        causes: Vec::new(),
    }
}

#[test]
fn deterministic_rng_produces_reproducible_sequences() {
    let mut rng1 = DeterministicRng::new(42);
    let mut rng2 = DeterministicRng::new(42);

    let vals1: Vec<u64> = (0..100).map(|_| rng1.next_u64()).collect();
    let vals2: Vec<u64> = (0..100).map(|_| rng2.next_u64()).collect();
    assert_eq!(vals1, vals2);

    // Test snapshot and restoration
    let snap = rng1.snapshot();
    let next_val1 = rng1.next_u64();

    rng2.restore(snap);
    let next_val2 = rng2.next_u64();
    assert_eq!(next_val1, next_val2);
}

#[test]
fn checkpoint_roundtrips_and_detects_tampering() {
    let header = CheckpointHeader {
        database: DatabaseId([1; 16]),
        source: SourceId(1),
        epoch: SourceEpoch(1),
        sequence: 42,
        valid_time: Timestamp::new(ClockId(1), 100),
        known_time: Timestamp::new(ClockId(2), 200),
        rng: RngSnapshot {
            seed: 7,
            state: 12345,
            steps: 99,
        },
        state_bytes_len: 4,
    };
    let checkpoint = Checkpoint {
        header,
        state_payload: vec![10, 20, 30, 40],
    };

    let encoded = encode_checkpoint(&checkpoint).unwrap();
    let decoded = decode_checkpoint(&encoded).unwrap();
    assert_eq!(decoded, checkpoint);

    // Bit flip in payload
    let mut tampered = encoded.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 0x01;
    assert!(matches!(
        decode_checkpoint(&tampered),
        Err(ReplayError::ChecksumMismatch)
    ));
}

#[test]
fn deterministic_replay_equivalence_matches_continuous_execution() {
    let dir = TestDir::new();
    let schema = sample_schema();
    let mut store = Store::create(&dir.0, SourceId(1), SourceEpoch(1), Limits::default()).unwrap();

    let entity_a = EntityId {
        shard: ShardId(0),
        slot: 0,
        generation: 0,
    };
    let entity_b = EntityId {
        shard: ShardId(0),
        slot: 1,
        generation: 0,
    };

    // Append 10 events
    for i in 0..10 {
        let event = if i % 2 == 0 {
            sample_write_event(entity_a, i, i * 100, true)
        } else {
            sample_write_event(entity_b, i, i * 200, i > 5)
        };
        store.append(vec![event]).unwrap();
    }
    drop(store);

    // Initialize replay engine
    let mut engine = ReplayEngine::new(&dir.0, 999).unwrap();
    let mut reducer = SchemaStateReducer::new(vec![schema.clone()]);

    // 1. Full reconstruction from sequence 0 to sequence 5
    let (state_at_5, header_at_5) = engine
        .reconstruct_at_sequence(5, &mut reducer, decode_entity_map)
        .unwrap();

    // Take a checkpoint at sequence 5
    let cp = Checkpoint {
        header: header_at_5,
        state_payload: encode_entity_map(&state_at_5),
    };
    engine.checkpoints().save(&cp).unwrap();

    // 2. Continuous playback from sequence 0 up to 9
    let mut fresh_engine = ReplayEngine::new(&dir.0, 999).unwrap();
    let mut fresh_reducer = SchemaStateReducer::new(vec![schema.clone()]);
    let (continuous_state_at_9, continuous_header_at_9) = fresh_engine
        .reconstruct_at_sequence(9, &mut fresh_reducer, decode_entity_map)
        .unwrap();

    // 3. Reconstruct up to 9 using the checkpoint at 5
    let mut resumed_engine = ReplayEngine::new(&dir.0, 999).unwrap();
    let mut resumed_reducer = SchemaStateReducer::new(vec![schema]);
    let (resumed_state_at_9, resumed_header_at_9) = resumed_engine
        .reconstruct_at_sequence(9, &mut resumed_reducer, decode_entity_map)
        .unwrap();

    // BIT-FOR-BIT EQUIVALENCE ASSERTIONS:
    assert_eq!(
        resumed_state_at_9, continuous_state_at_9,
        "Reconstruction from checkpoint must be bit-for-bit identical to full playback!"
    );
    assert_eq!(
        resumed_header_at_9.sequence, continuous_header_at_9.sequence,
        "Final sequence must match exactly"
    );
    assert_eq!(
        resumed_header_at_9.valid_time, continuous_header_at_9.valid_time,
        "Final valid timestamp must match exactly"
    );
    assert_eq!(
        resumed_header_at_9.rng, continuous_header_at_9.rng,
        "RNG state must match exactly after replay"
    );

    // Verify actual reconstructed entity fields
    let fields_a = &resumed_state_at_9.get(&entity_a).unwrap().1;
    assert_eq!(fields_a.get(&FieldId(0)), Some(&Value::U64(800)));
    assert_eq!(fields_a.get(&FieldId(1)), Some(&Value::Bool(true)));

    let fields_b = &resumed_state_at_9.get(&entity_b).unwrap().1;
    assert_eq!(fields_b.get(&FieldId(0)), Some(&Value::U64(1800)));
    assert_eq!(fields_b.get(&FieldId(1)), Some(&Value::Bool(true)));
}

#[test]
fn raw_entity_map_roundtrip_and_reconstruction() {
    let mut map = RawEntityMap::new();
    let e1 = EntityId {
        shard: ShardId(1),
        slot: 10,
        generation: 1,
    };
    let e2 = EntityId {
        shard: ShardId(2),
        slot: 20,
        generation: 2,
    };
    map.insert(e1, (SchemaId(100), vec![1, 2, 3, 4]));
    map.insert(e2, (SchemaId(200), vec![5, 6, 7, 8, 9]));

    let encoded = encode_raw_entity_map(&map);
    let decoded = decode_raw_entity_map(&encoded).unwrap();
    assert_eq!(map, decoded);

    let mut reducer = RawEntityReducer;
    let mut rng = DeterministicRng::new(42);
    let event = temnion_format::StoredEvent {
        id: temnion_core::EventId {
            source: SourceId(1),
            epoch: SourceEpoch(1),
            sequence: 0,
        },
        entity: e1,
        schema: SchemaId(100),
        times: EventTimes {
            valid: Timestamp::new(ClockId(1), 100),
            observed: None,
            known: Timestamp::new(ClockId(2), 200),
        },
        payload: vec![9, 9, 9],
        causes: Vec::new(),
    };
    temnion_replay::StateReducer::apply(&mut reducer, &mut map, &event, &mut rng).unwrap();
    assert_eq!(map.get(&e1).unwrap().1, vec![9, 9, 9]);
}
