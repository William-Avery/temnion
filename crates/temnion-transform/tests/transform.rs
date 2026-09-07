// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]

//! Integration tests for the deterministic transformation engine (M29) and canonical IR e-graph optimizer (M30).

use std::collections::HashMap;

use temnion_core::{ClockId, EventId, SourceEpoch, SourceId, Timestamp};
use temnion_eks::{Episode, KnowledgeId, TieredKnowledgeStore};
use temnion_query::{BinaryOp, Expr, Literal, LogicalPlan};
use temnion_transform::{
    EGraph, TransformationBudget, TransformationEngine, TransformationId, TransformationKind,
    TransformationManifest, VersionTag,
};

#[test]
fn test_transformation_engine_event_to_observation_and_metering() {
    let mut engine = TransformationEngine::new();

    let manifest = TransformationManifest {
        id: TransformationId(1),
        name: "sensor-telemetry-extractor".to_string(),
        version: VersionTag::new(1, 0, 0),
        kind: TransformationKind::EventToObservation,
        description: "Extracts scalar telemetry temperature from raw sensor event bytes"
            .to_string(),
        budget: TransformationBudget {
            max_cpu_steps: 1_000,
            max_memory_bytes: 4_096,
        },
    };
    engine.register_manifest(manifest);

    let event_id = EventId {
        source: SourceId(1),
        epoch: SourceEpoch(1),
        sequence: 42,
    };
    let valid_time = Timestamp::new(ClockId(1), 100);
    let known_time = Timestamp::new(ClockId(1), 110);
    let payload = b"23.5 C";

    let (obs, lineage) = engine
        .execute_event_to_observation(
            TransformationId(1),
            event_id,
            valid_time,
            known_time,
            "temperature",
            payload,
            KnowledgeId(101),
        )
        .expect("Extraction should succeed");

    assert_eq!(obs.id, KnowledgeId(101));
    assert_eq!(obs.event_ref, event_id);
    assert_eq!(obs.feature, "temperature");
    assert_eq!(obs.value_repr, "23.5 C");

    assert_eq!(lineage.transformation_id, TransformationId(1));
    assert_eq!(lineage.input_events, vec![event_id]);
    assert_eq!(lineage.output_knowledge, vec![KnowledgeId(101)]);
    assert!(lineage.receipt.success);
    assert!(lineage.receipt.steps_consumed > 0);
    assert!(lineage.receipt.memory_allocated_bytes > 0);

    // Verify lineage log in engine
    assert_eq!(engine.lineage_records().len(), 1);

    // Test precondition failure on empty payload
    let err = engine.execute_event_to_observation(
        TransformationId(1),
        EventId {
            source: SourceId(1),
            epoch: SourceEpoch(1),
            sequence: 43,
        },
        valid_time,
        known_time,
        "temperature",
        b"",
        KnowledgeId(102),
    );
    assert!(err.is_err());
    assert_eq!(engine.lineage_records().len(), 2);
    assert!(!engine.lineage_records()[1].receipt.success);
}

#[test]
fn test_transformation_engine_episode_consolidation() {
    let mut engine = TransformationEngine::new();
    let mut store = TieredKnowledgeStore::new(10);

    let manifest = TransformationManifest {
        id: TransformationId(2),
        name: "episode-pattern-miner".to_string(),
        version: VersionTag::new(1, 0, 0),
        kind: TransformationKind::EpisodeToPattern,
        description: "Consolidates temporal sequence of observations into generalized pattern"
            .to_string(),
        budget: TransformationBudget::default(),
    };
    engine.register_manifest(manifest);

    let episode = Episode {
        id: 1,
        label: "high_temp_spike".to_string(),
        observations: vec![KnowledgeId(10), KnowledgeId(11), KnowledgeId(12)],
        start_time: Timestamp::new(ClockId(1), 100),
        end_time: Timestamp::new(ClockId(1), 150),
    };

    let (pattern_id, lineage) = engine
        .execute_episode_consolidation(
            TransformationId(2),
            &mut store,
            episode,
            "signature:temp_spike>80",
            Timestamp::new(ClockId(1), 200),
        )
        .expect("Episode consolidation should succeed");

    assert_eq!(pattern_id, 1);
    assert_eq!(store.patterns().len(), 1);
    assert_eq!(store.patterns()[0].signature, "signature:temp_spike>80");
    assert_eq!(lineage.input_knowledge.len(), 3);
    assert_eq!(lineage.output_knowledge, vec![KnowledgeId(pattern_id)]);
    assert!(lineage.receipt.success);
}

#[test]
fn test_egraph_boolean_simplification_and_extraction() {
    let mut egraph = EGraph::new();

    // Expression: NOT(NOT(active))
    // We expect this to simplify to just `active`.
    let original = Expr::Not(Box::new(Expr::Not(Box::new(Expr::Field(
        "active".to_string(),
    )))));

    let root_id = egraph.add_expr(&original);
    egraph.rebuild();

    let report = egraph.saturate(10);
    assert!(report.total_rewrites > 0);

    let (extracted, cost) = egraph
        .extract_best_expr(root_id)
        .expect("Should extract minimal expression");

    assert_eq!(extracted, Expr::Field("active".to_string()));
    assert_eq!(cost, 2); // Field cost = 2, whereas Not(Not(Field)) = 2 + 2 + 2 = 6
}

#[test]
fn test_egraph_and_or_identities_and_cost_reduction() {
    let mut egraph = EGraph::new();

    // Expression: (status AND true) OR (status AND false)
    // (status AND true) simplifies to `status`
    // (status AND false) simplifies to `false`
    // (status OR false) simplifies to `status`
    let original = Expr::Binary {
        op: BinaryOp::Or,
        left: Box::new(Expr::Binary {
            op: BinaryOp::And,
            left: Box::new(Expr::Field("status".to_string())),
            right: Box::new(Expr::Literal(Literal::Bool(true))),
        }),
        right: Box::new(Expr::Binary {
            op: BinaryOp::And,
            left: Box::new(Expr::Field("status".to_string())),
            right: Box::new(Expr::Literal(Literal::Bool(false))),
        }),
    };

    let root_id = egraph.add_expr(&original);
    egraph.rebuild();

    let report = egraph.saturate(10);
    assert!(report.total_rewrites >= 2);

    let (extracted, extracted_cost) = egraph
        .extract_best_expr(root_id)
        .expect("Should extract simplified form");

    assert_eq!(extracted, Expr::Field("status".to_string()));
    assert_eq!(extracted_cost, 2);

    // Verify semantic equivalence across truth table
    for status_val in [true, false] {
        let mut fields = HashMap::new();
        fields.insert("status".to_string(), Literal::Bool(status_val));

        let orig_res = original
            .evaluate(&fields)
            .expect("Original evaluation should succeed");
        let opt_res = extracted
            .evaluate(&fields)
            .expect("Optimized evaluation should succeed");

        assert_eq!(orig_res, opt_res);
    }
}

#[test]
fn test_egraph_constant_folding() {
    let mut egraph = EGraph::new();

    // Expression: (5 == 5) AND (10 < 20)
    // 5 == 5 folds to true
    // 10 < 20 folds to true
    // true AND true folds to true
    let expr = Expr::Binary {
        op: BinaryOp::And,
        left: Box::new(Expr::Binary {
            op: BinaryOp::Eq,
            left: Box::new(Expr::Literal(Literal::Int(5))),
            right: Box::new(Expr::Literal(Literal::Int(5))),
        }),
        right: Box::new(Expr::Binary {
            op: BinaryOp::Lt,
            left: Box::new(Expr::Literal(Literal::Int(10))),
            right: Box::new(Expr::Literal(Literal::Int(20))),
        }),
    };

    let root_id = egraph.add_expr(&expr);
    egraph.rebuild();

    let report = egraph.saturate(10);
    assert!(report.total_rewrites >= 2);

    let (extracted, cost) = egraph
        .extract_best_expr(root_id)
        .expect("Should extract folded expression");

    assert_eq!(extracted, Expr::Literal(Literal::Bool(true)));
    assert_eq!(cost, 1);
}

#[test]
fn test_egraph_scan_filter_simplification() {
    let mut egraph = EGraph::new();

    let plan = LogicalPlan::Scan {
        entity: None,
        schema: None,
        valid_range: None,
        known_as_of: None,
        filter: Some(Expr::Literal(Literal::Bool(true))),
        projection: None,
        limit: Some(50),
    };

    let _root_id = egraph
        .add_plan(&plan)
        .expect("Should add scan plan to e-graph");
    egraph.rebuild();

    let report = egraph.saturate(5);
    assert!(report.total_rewrites >= 1);
}
