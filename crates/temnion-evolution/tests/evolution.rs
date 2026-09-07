use temnion_evolution::*;

#[test]
fn test_constitution_blocks_evidence_and_self_modification() {
    let constitution = Constitution::new();

    // 1. Attempt to mutate storage evidence must fail
    let action_mutate_evidence = ProposedAction::MutateStorageEvidence { event_id: 42 };
    let result = constitution.validate_action(&action_mutate_evidence);
    assert!(result.is_err());
    match result.unwrap_err() {
        ConstitutionalViolation::EvidenceMutationAttempted(msg) => {
            assert!(msg.contains("42"));
        }
        other => panic!("Expected EvidenceMutationAttempted, got: {other:?}"),
    }

    // 2. Attempt to mutate Constitution must fail
    let action_mutate_constitution = ProposedAction::MutateConstitution {
        target_axiom: ConstitutionalAxiom::NonSelfModification,
    };
    let result = constitution.validate_action(&action_mutate_constitution);
    assert!(result.is_err());
    match result.unwrap_err() {
        ConstitutionalViolation::ConstitutionSelfModificationAttempted(msg) => {
            assert!(msg.contains("NonSelfModification"));
        }
        other => panic!("Expected ConstitutionSelfModificationAttempted, got: {other:?}"),
    }

    // 3. Attempt to bypass exact storage via semantic query must fail
    let action_bypass = ProposedAction::QuerySemanticProjection {
        query_text: "find anomaly".into(),
        exact_bypass: true,
    };
    let result = constitution.validate_action(&action_bypass);
    assert!(result.is_err());
    match result.unwrap_err() {
        ConstitutionalViolation::EvidenceMutationAttempted(_) => {}
        other => panic!("Expected EvidenceMutationAttempted, got: {other:?}"),
    }
}

#[test]
fn test_champion_challenger_lifecycle_and_net_benefit() {
    let mut engine = EvolutionEngine::new(EvolutionConfig {
        enabled: true,
        manual_promotion_required: true,
        gate_c_passed: true,
        min_net_benefit_threshold: 0.05, // 5% minimum net benefit
    });

    // 1. Register static incumbent
    let baseline_fitness = FitnessMetrics {
        latency_p50_us: 1000,
        latency_p99_us: 2500,
        memory_bytes: 10_000_000,
        storage_bytes: 50_000_000,
        cpu_cycles: 500_000,
        read_amplification: 2.0,
        write_amplification: 1.5,
        background_cost_score: 0.10,
        net_benefit_score: 0.0,
    };

    let inc_id = engine.register_incumbent(
        CandidateKind::Codec,
        "segment_compression".into(),
        b"RawIncumbentCodec".to_vec(),
        baseline_fitness.clone(),
    );
    assert_eq!(inc_id, CandidateId(1));

    // 2. Propose challenger
    let lineage = CandidateLineage {
        candidate_id: CandidateId(0),
        parent_candidate_id: Some(inc_id),
        target_domain: "segment_compression".into(),
        created_at_ms: 2000,
        mutation_operator: "AdaptiveBitPackDelta".into(),
        rationale: "BitPack + Delta reduces storage footprint while preserving latency".into(),
    };

    let candidate_id = engine
        .propose_candidate(
            CandidateKind::Codec,
            "segment_compression".into(),
            lineage,
            b"BitPackDeltaCodec".to_vec(),
        )
        .expect("Propose candidate should succeed");

    assert_eq!(candidate_id, CandidateId(2));

    // 3. Shadow-evaluate candidate
    let workload = EvaluationWorkload {
        workload_id: "bench_scan_segment".into(),
        sample_keys: vec!["entity_1".into(), "entity_2".into()],
        sample_events: 1000,
        iterations: 10,
    };

    let challenger_simulated = FitnessMetrics {
        latency_p50_us: 800,       // 20% faster
        latency_p99_us: 2000,      // 20% faster
        memory_bytes: 8_000_000,   // 20% less RAM
        storage_bytes: 35_000_000, // 30% smaller storage
        cpu_cycles: 450_000,
        read_amplification: 1.5,
        write_amplification: 1.5,
        background_cost_score: 0.12, // slightly higher background cost
        net_benefit_score: 0.0,      // computed by evaluate_candidate
    };

    let evaluated_metrics = engine
        .evaluate_candidate(
            candidate_id,
            &workload,
            IsolationBudget::default(),
            challenger_simulated,
        )
        .expect("Shadow evaluation should succeed");

    assert!(
        evaluated_metrics.net_benefit_score > 0.05,
        "Net benefit score should exceed 5%: {}",
        evaluated_metrics.net_benefit_score
    );

    // 4. Autonomous promotion without manual approver must fail
    let auto_promote_result = engine.promote_candidate(candidate_id, None, 3000);
    assert!(auto_promote_result.is_err());
    match auto_promote_result.unwrap_err() {
        ConstitutionalViolation::UnauthorizedAutonomousPromotion(_) => {}
        other => panic!("Expected UnauthorizedAutonomousPromotion, got: {other:?}"),
    }

    // 5. Manual promotion succeeds
    engine
        .promote_candidate(candidate_id, Some("lead_architect"), 3000)
        .expect("Manual promotion with net benefit must succeed");

    let active = engine
        .active_incumbent("segment_compression")
        .expect("Active incumbent should exist");
    assert_eq!(active.id, candidate_id);
    assert_eq!(active.status, CandidateStatus::Promoted);
}

#[test]
fn test_instantaneous_rollback_on_regression() {
    let mut engine = EvolutionEngine::new(EvolutionConfig {
        enabled: true,
        manual_promotion_required: true,
        gate_c_passed: true,
        min_net_benefit_threshold: 0.05,
    });

    // Baseline
    let baseline = FitnessMetrics {
        latency_p50_us: 1000,
        latency_p99_us: 2000,
        memory_bytes: 10_000_000,
        storage_bytes: 50_000_000,
        cpu_cycles: 500_000,
        read_amplification: 2.0,
        write_amplification: 1.5,
        background_cost_score: 0.10,
        net_benefit_score: 0.0,
    };

    let inc1_id = engine.register_incumbent(
        CandidateKind::Layout,
        "segment_layout".into(),
        b"Morton2D".to_vec(),
        baseline.clone(),
    );

    // Candidate 2
    let cand2_id = engine
        .propose_candidate(
            CandidateKind::Layout,
            "segment_layout".into(),
            CandidateLineage {
                candidate_id: CandidateId(0),
                parent_candidate_id: Some(inc1_id),
                target_domain: "segment_layout".into(),
                created_at_ms: 2000,
                mutation_operator: "AdaptiveSFC".into(),
                rationale: "Adaptive Hilbert curve".into(),
            },
            b"AdaptiveSFC".to_vec(),
        )
        .unwrap();

    let superior_metrics = FitnessMetrics {
        latency_p50_us: 700,
        latency_p99_us: 1500,
        memory_bytes: 8_000_000,
        storage_bytes: 40_000_000,
        cpu_cycles: 400_000,
        read_amplification: 1.5,
        write_amplification: 1.5,
        background_cost_score: 0.10,
        net_benefit_score: 0.0,
    };

    engine
        .evaluate_candidate(
            cand2_id,
            &EvaluationWorkload {
                workload_id: "layout_eval".into(),
                sample_keys: vec![],
                sample_events: 100,
                iterations: 5,
            },
            IsolationBudget::default(),
            superior_metrics,
        )
        .unwrap();

    engine
        .promote_candidate(cand2_id, Some("operator"), 2500)
        .unwrap();

    assert_eq!(
        engine.active_incumbent("segment_layout").unwrap().id,
        cand2_id
    );

    // Regression detected in production: trigger instantaneous rollback
    let restored_id = engine
        .rollback(
            "segment_layout",
            "Tail latency p99 regression detected under write skew",
        )
        .expect("Rollback must succeed");

    assert_eq!(restored_id, inc1_id);
    assert_eq!(
        engine.active_incumbent("segment_layout").unwrap().id,
        inc1_id
    );

    // Verify cand2 is marked RolledBack
    let cand2_record = engine.get_candidate(cand2_id).unwrap();
    match &cand2_record.status {
        CandidateStatus::RolledBack { reason } => {
            assert!(reason.contains("Tail latency"));
        }
        other => panic!("Expected RolledBack status, got: {other:?}"),
    }
}

#[test]
fn test_adaptive_physical_memory_storage_fitness() {
    let memory_eval = AdaptivePhysicalMemory::new();

    // Baseline: 10MB uncompressed, 5MB compressed, 500us latency
    let fitness_incumbent =
        memory_eval.compute_storage_fitness(10_000_000, 5_000_000, 500, 2000, 2.0);

    // Challenger: 10MB uncompressed, 2.5MB compressed, 350us latency
    let fitness_challenger =
        memory_eval.compute_storage_fitness(10_000_000, 2_500_000, 350, 2500, 1.5);

    assert!(
        fitness_challenger > fitness_incumbent,
        "Challenger should have superior fitness score: challenger={}, incumbent={}",
        fitness_challenger,
        fitness_incumbent
    );
}

#[test]
fn test_adaptive_lifecycle_evaluator() {
    let lifecycle = AdaptiveLifecycleEvaluator::new();

    // Frequent checkpointing (interval = 500): lower recovery replay latency
    let score_frequent = lifecycle.evaluate_checkpoint_policy(500, 50_000.0, 50_000);

    // Sparse checkpointing (interval = 50,000): higher replay latency
    let score_sparse = lifecycle.evaluate_checkpoint_policy(50_000, 50_000.0, 50_000);

    assert!(score_frequent > 0.0);
    assert!(score_sparse > 0.0);
}

#[test]
fn test_semantic_vector_projections_and_exact_isolation() {
    let mut index = SemanticProjectionIndex::new();

    let emb_a = VectorEmbedding::new("concept_cache_eviction", vec![0.9, 0.1, 0.05]);
    let emb_b = VectorEmbedding::new("concept_lru_policy", vec![0.85, 0.15, 0.04]);
    let emb_c = VectorEmbedding::new("concept_wal_recovery", vec![0.05, 0.95, 0.8]);

    index.insert(emb_a.clone());
    index.insert(emb_b.clone());
    index.insert(emb_c);

    // High cosine similarity between cache eviction and lru policy
    let sim_ab = emb_a.cosine_similarity(&emb_b);
    assert!(sim_ab > 0.95);

    // Query nearest neighbors
    let query = VectorEmbedding::new("query", vec![0.88, 0.12, 0.06]);
    let neighbors = index.nearest_neighbors(&query, 2);

    assert_eq!(neighbors.len(), 2);
    assert_eq!(neighbors[0].0, "concept_cache_eviction");
    assert_eq!(neighbors[1].0, "concept_lru_policy");
}

#[test]
fn test_meta_evolution_gated_behind_gate_c_and_off_switch() {
    let mut manager = MetaEvolutionManager::new();

    let superior_policy = MutationPolicy {
        policy_id: "tuned_distribution_v2".into(),
        version: 2,
        representation_weight: 0.35,
        rule_weight: 0.35,
        lifecycle_weight: 0.20,
        exploratory_weight: 0.10,
        historical_candidate_yield: 0.25, // 25% promotion yield
    };

    // 1. Off-switch blocks meta-evolution
    assert!(!manager.is_enabled());
    let err = manager.evolve_policy(superior_policy.clone(), true);
    assert!(err.is_err());

    // 2. Disable off-switch, but Gate C not met: blocks meta-evolution
    manager.set_enabled(true);
    let err_gate_c = manager.evolve_policy(superior_policy.clone(), false);
    assert!(err_gate_c.is_err());
    match err_gate_c.unwrap_err() {
        ConstitutionalViolation::GateCriteriaNotMet { gate, .. } => {
            assert_eq!(gate, "Gate C");
        }
        other => panic!("Expected GateCriteriaNotMet, got: {other:?}"),
    }

    // 3. Gate C met and off-switch disabled: evolves policy
    manager
        .evolve_policy(superior_policy, true)
        .expect("Meta-evolution must succeed when Gate C is met and enabled");

    assert_eq!(manager.active_policy().version, 2);
}

#[test]
fn test_constitution_audit() {
    let mut engine = EvolutionEngine::new(EvolutionConfig::default());
    let baseline = FitnessMetrics {
        latency_p50_us: 1000,
        latency_p99_us: 2000,
        memory_bytes: 10_000_000,
        storage_bytes: 50_000_000,
        cpu_cycles: 500_000,
        read_amplification: 2.0,
        write_amplification: 1.5,
        background_cost_score: 0.10,
        net_benefit_score: 0.0,
    };

    engine.register_incumbent(
        CandidateKind::Index,
        "entity_bloom".into(),
        b"StandardBloom".to_vec(),
        baseline,
    );

    let report = engine.audit_constitution();
    assert!(report.passed, "Audit should pass on clean system state");
    assert_eq!(report.axioms_checked, 8);
    assert_eq!(report.audited_incumbents, 1);
    assert!(report.violations.is_empty());
}
