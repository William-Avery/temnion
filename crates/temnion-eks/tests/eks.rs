// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]

use temnion_core::{ClockId, EventId, SourceEpoch, SourceId, Timestamp};
use temnion_eks::{
    Confidence, Episode, Justification, KnowledgeId, KnowledgeStatus, KnowledgeTier, ModelManifest,
    PredictionLedger, TieredKnowledgeStore, TruthMaintenanceSystem,
};

#[test]
fn knowledge_primitives_creation_and_bounds() {
    let conf = Confidence::new(0.85).expect("valid confidence should succeed");
    assert_eq!(conf.value(), 0.85);

    assert!(Confidence::new(-0.01).is_err());
    assert!(Confidence::new(1.01).is_err());
    assert!(Confidence::new(f64::NAN).is_err());

    let mut tms = TruthMaintenanceSystem::new();

    let ev = EventId {
        source: SourceId(1),
        epoch: SourceEpoch(1),
        sequence: 42,
    };
    let obs_id = tms.record_observation(
        ev,
        Timestamp::new(ClockId(1), 100),
        Timestamp::new(ClockId(1), 105),
        "temperature",
        "24.5",
    );

    let claim_id = tms.assert_claim(
        "sensor-agent-01",
        "temperature is nominal",
        Confidence::new(0.95).unwrap(),
        Timestamp::new(ClockId(1), 90)..Timestamp::new(ClockId(1), 110),
        Timestamp::new(ClockId(1), 105),
    );

    let rule_id = tms.define_rule(
        "nominal_temp_implies_safe",
        "temperature is nominal",
        "reactor status is safe",
        Confidence::new(0.99).unwrap(),
    );

    let belief_id = tms.infer_belief(
        "reactor status is safe",
        Confidence::new(0.94).unwrap(),
        Timestamp::new(ClockId(1), 100),
        Timestamp::new(ClockId(1), 106),
        Justification {
            evidence_events: vec![ev],
            premises: vec![claim_id],
            applied_rules: vec![rule_id],
            assumptions: vec!["no external thermal leak".to_string()],
        },
    );

    assert_ne!(obs_id, claim_id);
    assert_ne!(claim_id, belief_id);
    assert_eq!(tms.beliefs_count(), 1);

    let model_id = tms.register_model(ModelManifest {
        id: KnowledgeId(0),
        model_name: "anomaly-detector-v1".to_string(),
        runtime: "onnx".to_string(),
        version: "1.0.0".to_string(),
        is_stateful: false,
        max_memory_bytes: 64 * 1024 * 1024,
    });
    assert_eq!(
        tms.get_model(model_id).unwrap().model_name,
        "anomaly-detector-v1"
    );

    let skill_id = tms.register_skill(temnion_eks::Skill {
        id: KnowledgeId(0),
        name: "rebalance-shards".to_string(),
        intent: "Balance load across worker shards".to_string(),
        steps: vec!["scan_load".to_string(), "migrate_keys".to_string()],
        version: 1,
    });
    assert_eq!(tms.get_skill(skill_id).unwrap().name, "rebalance-shards");
}

#[test]
fn truth_maintenance_and_contradiction_tracking() {
    let mut tms = TruthMaintenanceSystem::new();

    let b1 = tms.infer_belief(
        "target node is healthy",
        Confidence::new(0.9).unwrap(),
        Timestamp::new(ClockId(1), 10),
        Timestamp::new(ClockId(1), 11),
        Justification::default(),
    );

    // Contradictory belief arrives later
    let b2 = tms.infer_belief(
        "target node is critical",
        Confidence::new(0.85).unwrap(),
        Timestamp::new(ClockId(1), 15),
        Timestamp::new(ClockId(1), 16),
        Justification::default(),
    );

    assert_eq!(tms.contradictions().len(), 1);
    let contra = &tms.contradictions()[0];
    assert_eq!(contra.item_a, b1);
    assert_eq!(contra.item_b, b2);

    let trace1 = tms.why(b1).unwrap();
    let trace2 = tms.why(b2).unwrap();
    assert_eq!(trace1.status, KnowledgeStatus::Contradicted);
    assert_eq!(trace2.status, KnowledgeStatus::Contradicted);
}

#[test]
fn belief_retraction_cascades_non_destructively() {
    let mut tms = TruthMaintenanceSystem::new();

    let p1 = tms.infer_belief(
        "sensor battery is adequate",
        Confidence::new(0.99).unwrap(),
        Timestamp::new(ClockId(1), 10),
        Timestamp::new(ClockId(1), 10),
        Justification::default(),
    );

    let p2 = tms.infer_belief(
        "pressure reading is reliable",
        Confidence::new(0.95).unwrap(),
        Timestamp::new(ClockId(1), 10),
        Timestamp::new(ClockId(1), 11),
        Justification {
            evidence_events: vec![],
            premises: vec![p1],
            applied_rules: vec![],
            assumptions: vec![],
        },
    );

    let p3 = tms.infer_belief(
        "boiler pressure is optimal",
        Confidence::new(0.90).unwrap(),
        Timestamp::new(ClockId(1), 10),
        Timestamp::new(ClockId(1), 12),
        Justification {
            evidence_events: vec![],
            premises: vec![p2],
            applied_rules: vec![],
            assumptions: vec![],
        },
    );

    // Retract base premise p1
    tms.retract_belief(p1).unwrap();

    // Cascaded to p2 and p3
    assert_eq!(tms.why(p1).unwrap().status, KnowledgeStatus::Retracted);
    assert_eq!(tms.why(p2).unwrap().status, KnowledgeStatus::Retracted);
    assert_eq!(tms.why(p3).unwrap().status, KnowledgeStatus::Retracted);
}

#[test]
fn why_provenance_query_traversal() {
    let mut tms = TruthMaintenanceSystem::new();

    let ev1 = EventId {
        source: SourceId(1),
        epoch: SourceEpoch(1),
        sequence: 10,
    };
    let ev2 = EventId {
        source: SourceId(1),
        epoch: SourceEpoch(1),
        sequence: 11,
    };

    let obs1 = tms.record_observation(
        ev1,
        Timestamp::new(ClockId(1), 100),
        Timestamp::new(ClockId(1), 101),
        "rpm",
        "3600",
    );

    let obs2 = tms.record_observation(
        ev2,
        Timestamp::new(ClockId(1), 100),
        Timestamp::new(ClockId(1), 101),
        "vibration",
        "0.02",
    );

    let rule1 = tms.define_rule(
        "high_rpm_low_vib",
        "rpm == 3600 && vib < 0.05",
        "turbine is stable",
        Confidence::new(0.98).unwrap(),
    );

    let b_intermediate = tms.infer_belief(
        "turbine is stable",
        Confidence::new(0.95).unwrap(),
        Timestamp::new(ClockId(1), 100),
        Timestamp::new(ClockId(1), 102),
        Justification {
            evidence_events: vec![ev1, ev2],
            premises: vec![obs1, obs2],
            applied_rules: vec![rule1],
            assumptions: vec!["lubricant pressure normal".to_string()],
        },
    );

    let b_top = tms.infer_belief(
        "grid output can increase to 100MW",
        Confidence::new(0.90).unwrap(),
        Timestamp::new(ClockId(1), 100),
        Timestamp::new(ClockId(1), 103),
        Justification {
            evidence_events: vec![],
            premises: vec![b_intermediate],
            applied_rules: vec![],
            assumptions: vec![],
        },
    );

    let why_trace = tms
        .why(b_top)
        .expect("why query should traverse successfully");

    assert_eq!(why_trace.target_id, b_top);
    assert_eq!(why_trace.premise_traces.len(), 1);

    let intermediate_trace = &why_trace.premise_traces[0];
    assert_eq!(intermediate_trace.target_id, b_intermediate);
    assert_eq!(intermediate_trace.direct_evidence, vec![ev1, ev2]);
    assert_eq!(intermediate_trace.applied_rules.len(), 1);
    assert_eq!(intermediate_trace.applied_rules[0].name, "high_rpm_low_vib");
    assert_eq!(
        intermediate_trace.assumptions,
        vec!["lubricant pressure normal".to_string()]
    );
}

#[test]
fn prediction_ledger_and_brier_score_calibration() {
    let mut ledger = PredictionLedger::new();
    let model_id = temnion_eks::KnowledgeId(999);

    // Prediction 1: emitted at t=10 for valid time 20, confidence 0.9 (predicts "failure")
    let p1 = ledger.record_prediction(
        model_id,
        0,
        1,
        Timestamp::new(ClockId(1), 20),
        "failure",
        Confidence::new(0.9).unwrap(),
        Timestamp::new(ClockId(1), 10),
    );

    // Prediction 2: emitted at t=10 for valid time 20, confidence 0.2 (predicts "failure")
    let p2 = ledger.record_prediction(
        model_id,
        0,
        2,
        Timestamp::new(ClockId(1), 20),
        "failure",
        Confidence::new(0.2).unwrap(),
        Timestamp::new(ClockId(1), 10),
    );

    // Prediction 3: emitted at t=60 (future relative to cutoff 50)
    let _p3 = ledger.record_prediction(
        model_id,
        0,
        3,
        Timestamp::new(ClockId(1), 70),
        "failure",
        Confidence::new(0.95).unwrap(),
        Timestamp::new(ClockId(1), 60),
    );

    // Resolve outcomes at valid time 20 (observed at known time 20)
    ledger
        .resolve_outcome(p1, "failure", Timestamp::new(ClockId(1), 20))
        .unwrap(); // Matched: actual = 1.0, forecast = 0.9 -> (0.9 - 1.0)^2 = 0.01
    ledger
        .resolve_outcome(p2, "nominal", Timestamp::new(ClockId(1), 20))
        .unwrap(); // Refuted: actual = 0.0, forecast = 0.2 -> (0.2 - 0.0)^2 = 0.04

    // Evaluate Brier score at cutoff = 50
    // Expected: (0.01 + 0.04) / 2 = 0.025
    let score = ledger
        .compute_brier_score(Timestamp::new(ClockId(1), 50))
        .expect("should compute calibration score");

    assert!((score - 0.025).abs() < 1e-6);

    // Evaluate Brier score at cutoff = 15 (before outcome at 20 was observed)
    assert!(
        ledger
            .compute_brier_score(Timestamp::new(ClockId(1), 15))
            .is_err(),
        "must reject evaluation when no outcomes are known yet"
    );
}

#[test]
fn knowledge_consolidation_across_tiers() {
    let mut store = TieredKnowledgeStore::new(2); // Max 2 active items

    let k1 = temnion_eks::KnowledgeId(1);
    let k2 = temnion_eks::KnowledgeId(2);
    let k3 = temnion_eks::KnowledgeId(3);

    store.insert_active(k1);
    store.insert_active(k2);
    store.record_access(k2); // k2 accessed twice

    assert_eq!(store.tier_of(k1), Some(KnowledgeTier::Active));
    assert_eq!(store.tier_of(k2), Some(KnowledgeTier::Active));

    // Inserting k3 should trigger eviction of least-accessed k1 to Archive
    store.insert_active(k3);

    assert_eq!(store.tier_of(k1), Some(KnowledgeTier::Archive));
    assert_eq!(store.tier_of(k2), Some(KnowledgeTier::Active));
    assert_eq!(store.tier_of(k3), Some(KnowledgeTier::Active));

    // Consolidate episode
    let episode = Episode {
        id: 1,
        label: "EngineStartCycle".to_string(),
        observations: vec![k1, k2],
        start_time: Timestamp::new(ClockId(1), 100),
        end_time: Timestamp::new(ClockId(1), 200),
    };

    let pat_id = store.consolidate_episode(episode, "rpm_ramp_up_signature");
    assert_eq!(pat_id, 1);
    assert_eq!(store.patterns().len(), 1);
    assert_eq!(store.patterns()[0].signature, "rpm_ramp_up_signature");

    // Observations in consolidated episode are promoted to Reference tier
    assert_eq!(store.tier_of(k1), Some(KnowledgeTier::Reference));
    assert_eq!(store.tier_of(k2), Some(KnowledgeTier::Reference));
}
