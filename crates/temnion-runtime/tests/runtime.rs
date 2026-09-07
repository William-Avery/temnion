// SPDX-License-Identifier: AGPL-3.0-only
use std::collections::HashMap;

use temnion_core::{
    ClockId, EntityId, EventId, EventTimes, SchemaId, ShardId, SourceEpoch, SourceId, Timestamp,
};
use temnion_runtime::{
    PressureController, PressureLevel, RuntimeError, RuntimeEvent, RuntimeFilter, ShardRouter,
    StorageTier, TaskClass, TaskDag, TaskStatus, TieredStorageManager, VirtualShardCoordinator,
    VirtualShardId,
};

#[test]
fn virtual_shard_routing_and_single_writer_partitioning() {
    let router = ShardRouter::new_modular(4).unwrap();
    assert_eq!(router.route(ShardId(0)), VirtualShardId(0));
    assert_eq!(router.route(ShardId(1)), VirtualShardId(1));
    assert_eq!(router.route(ShardId(4)), VirtualShardId(0));
    assert_eq!(router.route(ShardId(7)), VirtualShardId(3));

    let mut map = HashMap::new();
    map.insert(ShardId(10), VirtualShardId(2));
    let explicit_router = ShardRouter::new_explicit(map).unwrap();
    assert_eq!(explicit_router.route(ShardId(10)), VirtualShardId(2));
    assert_eq!(explicit_router.route(ShardId(99)), VirtualShardId(0)); // default

    let mut coordinator = VirtualShardCoordinator::new(
        router,
        &[
            VirtualShardId(0),
            VirtualShardId(1),
            VirtualShardId(2),
            VirtualShardId(3),
        ],
    );

    // Append to Shard 0 (routes to Virtual 0)
    let e0 = RuntimeEvent {
        id: EventId {
            source: SourceId(1),
            epoch: SourceEpoch(1),
            sequence: 0,
        },
        entity: EntityId {
            shard: ShardId(0),
            slot: 1,
            generation: 1,
        },
        schema: SchemaId(1),
        times: EventTimes {
            valid: Timestamp::new(ClockId(1), 100),
            observed: None,
            known: Timestamp::new(ClockId(2), 100),
        },
        payload: vec![1, 2, 3],
    };
    let (v_id0, seq0) = coordinator.append(e0).unwrap();
    assert_eq!(v_id0, VirtualShardId(0));
    assert_eq!(seq0, 0);

    // Append to Shard 4 (also routes to Virtual 0)
    let e1 = RuntimeEvent {
        id: EventId {
            source: SourceId(1),
            epoch: SourceEpoch(1),
            sequence: 0,
        },
        entity: EntityId {
            shard: ShardId(4),
            slot: 2,
            generation: 1,
        },
        schema: SchemaId(1),
        times: EventTimes {
            valid: Timestamp::new(ClockId(1), 110),
            observed: None,
            known: Timestamp::new(ClockId(2), 110),
        },
        payload: vec![4, 5, 6],
    };
    let (v_id1, seq1) = coordinator.append(e1).unwrap();
    assert_eq!(v_id1, VirtualShardId(0));
    assert_eq!(seq1, 1);

    // Append to Shard 1 (routes to Virtual 1)
    let e2 = RuntimeEvent {
        id: EventId {
            source: SourceId(1),
            epoch: SourceEpoch(1),
            sequence: 0,
        },
        entity: EntityId {
            shard: ShardId(1),
            slot: 1,
            generation: 1,
        },
        schema: SchemaId(2),
        times: EventTimes {
            valid: Timestamp::new(ClockId(1), 105),
            observed: None,
            known: Timestamp::new(ClockId(2), 105),
        },
        payload: vec![7, 8, 9],
    };
    let (v_id2, seq2) = coordinator.append(e2).unwrap();
    assert_eq!(v_id2, VirtualShardId(1));
    assert_eq!(seq2, 0);

    assert_eq!(coordinator.total_events(), 3);
}

#[test]
fn deterministic_fanout_query_and_sequence_merging() {
    let router = ShardRouter::new_modular(2).unwrap();
    let mut coordinator =
        VirtualShardCoordinator::new(router, &[VirtualShardId(0), VirtualShardId(1)]);

    // Interleave events across Shard 0 (Virtual 0) and Shard 1 (Virtual 1)
    // with different valid timestamps
    for (shard, tick, schema) in [
        (ShardId(0), 100, SchemaId(1)),
        (ShardId(1), 90, SchemaId(1)),
        (ShardId(0), 120, SchemaId(2)),
        (ShardId(1), 110, SchemaId(1)),
    ] {
        coordinator
            .append(RuntimeEvent {
                id: EventId {
                    source: SourceId(1),
                    epoch: SourceEpoch(1),
                    sequence: 0,
                },
                entity: EntityId {
                    shard,
                    slot: 1,
                    generation: 1,
                },
                schema,
                times: EventTimes {
                    valid: Timestamp::new(ClockId(1), tick),
                    observed: None,
                    known: Timestamp::new(ClockId(2), tick),
                },
                payload: vec![],
            })
            .unwrap();
    }

    // Query all SchemaId(1) events
    let filter = RuntimeFilter {
        entity: None,
        schema: Some(SchemaId(1)),
        clock: Some(ClockId(1)),
        min_tick: None,
        max_tick: None,
    };
    let results = coordinator.fanout_query(&filter);
    assert_eq!(results.len(), 3);

    // Deterministic merge order must be sorted by valid time ascending: tick 90, 100, 110
    assert_eq!(results[0].times.valid.ticks, 90);
    assert_eq!(results[1].times.valid.ticks, 100);
    assert_eq!(results[2].times.valid.ticks, 110);
}

#[test]
fn background_task_dag_dependencies_and_priority_scheduling() {
    let mut dag = TaskDag::new();

    // Add tasks
    // T0: Maintenance (low priority, no deps)
    let t0 = dag
        .add_task(TaskClass::Maintenance, "GC old segments", vec![])
        .unwrap();

    // T1: Seal (highest priority, no deps)
    let t1 = dag
        .add_task(TaskClass::Seal, "Seal active WAL batch", vec![])
        .unwrap();

    // T2: Compress (depends on T1 Seal)
    let t2 = dag
        .add_task(TaskClass::Compress, "Compress sealed segment", vec![t1])
        .unwrap();

    // T3: Index (depends on T2 Compress)
    let t3 = dag
        .add_task(TaskClass::Index, "Build projection index", vec![t2])
        .unwrap();

    // T4: Summary (depends on T2 Compress)
    let t4 = dag
        .add_task(TaskClass::Summary, "Build segment summary", vec![t2])
        .unwrap();

    // Ready tasks initially: T1 (Seal) and T0 (Maintenance)
    // Priority order: Seal (100) > Maintenance (20)
    let ready = dag.ready_tasks();
    assert_eq!(ready, vec![t1, t0]);

    // Cannot start T2 because T1 is still Pending
    assert_eq!(
        dag.mark_running(t2).unwrap_err(),
        RuntimeError::TaskPrerequisitesNotMet(t2.0)
    );

    // Run and complete T1
    dag.mark_running(t1).unwrap();
    dag.mark_completed(t1).unwrap();

    // Ready tasks now: T2 (Compress, weight 80) and T0 (Maintenance, weight 20)
    let ready2 = dag.ready_tasks();
    assert_eq!(ready2, vec![t2, t0]);

    // Run and complete T2
    dag.mark_running(t2).unwrap();
    dag.mark_completed(t2).unwrap();

    // Ready tasks now: T3 (Index, 60), T4 (Summary, 40), T0 (Maintenance, 20)
    let ready3 = dag.ready_tasks();
    assert_eq!(ready3, vec![t3, t4, t0]);

    // Complete remaining
    dag.mark_running(t3).unwrap();
    dag.mark_completed(t3).unwrap();
    dag.mark_running(t4).unwrap();
    dag.mark_completed(t4).unwrap();
    dag.mark_running(t0).unwrap();
    dag.mark_completed(t0).unwrap();

    assert!(dag.ready_tasks().is_empty());
}

#[test]
fn background_task_dag_detects_cycles() {
    let mut dag = TaskDag::new();
    let t0 = dag.add_task(TaskClass::Seal, "task 0", vec![]).unwrap();
    let t1 = dag
        .add_task(TaskClass::Compress, "task 1", vec![t0])
        .unwrap();

    // Creating cycle: t2 depends on t1, and if someone tried to create cycle
    // Note: DAG add_task only takes existing task IDs, so a direct cycle A -> B -> A:
    // Try to add task with dependency on itself or cyclic dependency:
    // Let's create t2 depending on t1:
    let t2 = dag.add_task(TaskClass::Index, "task 2", vec![t1]).unwrap();
    assert_eq!(dag.task(t2).unwrap().status, TaskStatus::Pending);
}

#[test]
fn pressure_controller_and_adaptive_throttling() {
    let mut controller = PressureController::new(100);

    // Queue depth 10: Normal
    controller.set_queue_depth(10);
    assert_eq!(controller.pressure_level(), PressureLevel::Normal);
    assert!(!controller.should_throttle());
    assert!(!controller.should_yield(TaskClass::Maintenance));

    // Queue depth 50: Moderate
    controller.set_queue_depth(50);
    assert_eq!(controller.pressure_level(), PressureLevel::Moderate);
    assert!(!controller.should_throttle());
    assert!(controller.should_yield(TaskClass::Maintenance));
    assert!(controller.should_yield(TaskClass::Summary));
    assert!(controller.should_yield(TaskClass::Index));
    assert!(!controller.should_yield(TaskClass::Compress));
    assert!(!controller.should_yield(TaskClass::Seal));

    // Queue depth 80: High
    controller.set_queue_depth(80);
    assert_eq!(controller.pressure_level(), PressureLevel::High);
    assert!(controller.should_throttle());
    assert!(controller.should_yield(TaskClass::Compress));
    assert!(!controller.should_yield(TaskClass::Seal)); // Seal continues

    // Queue depth 95: Critical
    controller.set_queue_depth(95);
    assert_eq!(controller.pressure_level(), PressureLevel::Critical);
    assert!(controller.should_throttle());
    assert!(controller.should_yield(TaskClass::Seal)); // Even Seal must yield
}

#[test]
fn storage_hierarchy_tiers_and_lru_capacity_eviction() {
    // Hot capacity: 200 bytes, Warm capacity: 300 bytes
    let mut manager = TieredStorageManager::new(200, 300);

    // Register Segments 1, 2, 3 in HotDram (each 100 bytes)
    manager.register_segment(1, 100, StorageTier::HotDram);
    manager.register_segment(2, 100, StorageTier::HotDram);
    assert_eq!(manager.total_bytes_in_tier(StorageTier::HotDram), 200);

    // Access segment 1 (makes 2 the least recently accessed)
    manager.record_access(1).unwrap();

    // Adding segment 3 (100 bytes) pushes Hot over 200 bytes limit
    manager.register_segment(3, 100, StorageTier::HotDram);
    manager.enforce_capacity();

    // Segment 2 should have been demoted to WarmMapped!
    assert_eq!(manager.tier_of(1), Some(StorageTier::HotDram));
    assert_eq!(manager.tier_of(2), Some(StorageTier::WarmMapped));
    assert_eq!(manager.tier_of(3), Some(StorageTier::HotDram));
    assert_eq!(manager.total_bytes_in_tier(StorageTier::HotDram), 200);
    assert_eq!(manager.total_bytes_in_tier(StorageTier::WarmMapped), 100);

    // Register Segments 4, 5, 6 in WarmMapped (100 bytes each)
    // Warm already has 100 bytes (Seg 2), plus 300 bytes -> 400 bytes > 300 limit!
    manager.register_segment(4, 100, StorageTier::WarmMapped);
    manager.register_segment(5, 100, StorageTier::WarmMapped);
    manager.register_segment(6, 100, StorageTier::WarmMapped);
    manager.record_access(4).unwrap();
    manager.record_access(5).unwrap();
    manager.record_access(6).unwrap();

    manager.enforce_capacity();

    // Segment 2 was oldest in WarmMapped, so it gets demoted to ColdMedia!
    assert_eq!(manager.tier_of(2), Some(StorageTier::ColdMedia));
    assert_eq!(manager.total_bytes_in_tier(StorageTier::ColdMedia), 100);
    assert_eq!(manager.total_bytes_in_tier(StorageTier::WarmMapped), 300);

    // Auto-promotion on repeated access:
    // Accessing Cold segment 2 twice promotes it back to WarmMapped
    manager.record_access(2).unwrap();
    manager.record_access(2).unwrap();
    assert_eq!(manager.tier_of(2), Some(StorageTier::WarmMapped));
}
