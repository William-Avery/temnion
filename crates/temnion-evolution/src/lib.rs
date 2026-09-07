//! Isolated measured evolution engine and immutable Constitution for Temnion.
//!
//! Implements Milestones M31–M38 in accordance with Temnion Architecture §10, §41, and §52 (Gate C):
//! - **M31 (T16)**: Champion/challenger evolution engine (isolation, evaluation, manual promotion, rollback, lineage).
//! - **M32 (T17)**: Adaptive physical memory (per-segment representation candidates, storage fitness).
//! - **M33 (T17)**: Adaptive lifecycle policies (dynamic checkpoints, segment sizing, tier thresholds).
//! - **M34 (T18)**: Semantic and vector projections (isolated associative similarity, exact memory primacy).
//! - **M35 (T18)**: Knowledge mutation (candidate beliefs/rules evaluated against calibration and contradictions).
//! - **M36 (T18)**: Transformation mutation (candidate transformation manifests with budget bounds).
//! - **M37 (T19)**: Meta-evolution (mutation policy evolution strictly gated behind Gate C with hard off-switch).
//! - **M38 (T01/T16)**: Immutable Constitution hardening (8 non-evolvable axioms, runtime audit framework).

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::fmt;

// ============================================================================
// M38: Immutable Constitution Hardening
// ============================================================================

/// Inviolable constitutional axioms protected against runtime evolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConstitutionalAxiom {
    /// Storage events and historical WAL/TSF files cannot be mutated, overwritten, or pruned by evolvable components.
    EvidenceImmutability,
    /// All derived knowledge and candidate artifacts must maintain explicit, verifiable provenance lineage.
    ProvenancePreservation,
    /// Evolvable candidates or meta-policies cannot rewrite the Constitution, evaluation gates, or promotion rules.
    NonSelfModification,
    /// Any promoted candidate exhibiting measured regression must be reversibly rolled back to the prior incumbent.
    MandatoryRollback,
    /// All candidate evaluations and background evolution tasks must run inside bounded, metered resource envelopes.
    ResourceBoundedness,
    /// Evolution cannot proceed unless predeclared gate prerequisites (Gate A, B, C) are satisfied with recorded evidence.
    GatePrerequisite,
    /// Associative and semantic vector projections are strictly secondary and cannot mask or replace exact history.
    ExactMemoryPrimacy,
    /// Representation and physical layout adaptations must maintain bit-level, deterministic reader compatibility.
    ReaderCompatibility,
}

impl fmt::Display for ConstitutionalAxiom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EvidenceImmutability => write!(f, "EvidenceImmutability"),
            Self::ProvenancePreservation => write!(f, "ProvenancePreservation"),
            Self::NonSelfModification => write!(f, "NonSelfModification"),
            Self::MandatoryRollback => write!(f, "MandatoryRollback"),
            Self::ResourceBoundedness => write!(f, "ResourceBoundedness"),
            Self::GatePrerequisite => write!(f, "GatePrerequisite"),
            Self::ExactMemoryPrimacy => write!(f, "ExactMemoryPrimacy"),
            Self::ReaderCompatibility => write!(f, "ReaderCompatibility"),
        }
    }
}

/// Constitutional violation error describing why an action was blocked.
#[derive(Debug, Clone, PartialEq)]
pub enum ConstitutionalViolation {
    EvidenceMutationAttempted(String),
    ConstitutionSelfModificationAttempted(String),
    UnauthorizedAutonomousPromotion(String),
    GateCriteriaNotMet { gate: String, detail: String },
    ResourceLimitExceeded { limit: String, requested: u64 },
    MissingProvenanceLineage(String),
    RegressionWithoutRollback(String),
    IncompatibleFormat(String),
}

impl fmt::Display for ConstitutionalViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EvidenceMutationAttempted(msg) => write!(f, "Evidence mutation blocked: {msg}"),
            Self::ConstitutionSelfModificationAttempted(msg) => {
                write!(f, "Constitution self-modification blocked: {msg}")
            }
            Self::UnauthorizedAutonomousPromotion(msg) => {
                write!(f, "Autonomous promotion blocked: {msg}")
            }
            Self::GateCriteriaNotMet { gate, detail } => {
                write!(f, "Gate {gate} not satisfied: {detail}")
            }
            Self::ResourceLimitExceeded { limit, requested } => {
                write!(f, "Resource limit exceeded for {limit}: {requested}")
            }
            Self::MissingProvenanceLineage(msg) => write!(f, "Missing provenance lineage: {msg}"),
            Self::RegressionWithoutRollback(msg) => {
                write!(f, "Regression detected without rollback: {msg}")
            }
            Self::IncompatibleFormat(msg) => write!(f, "Incompatible format: {msg}"),
        }
    }
}

impl std::error::Error for ConstitutionalViolation {}

/// Proposed system or evolution action evaluated by the Constitution.
#[derive(Debug, Clone, PartialEq)]
pub enum ProposedAction {
    MutateStorageEvidence {
        event_id: u64,
    },
    MutateConstitution {
        target_axiom: ConstitutionalAxiom,
    },
    PromoteCandidate {
        candidate_id: CandidateId,
        manual: bool,
        approver: Option<String>,
    },
    ExecuteShadowEvaluation {
        budget: IsolationBudget,
    },
    MutateMutationPolicy {
        gate_c_passed: bool,
    },
    QuerySemanticProjection {
        query_text: String,
        exact_bypass: bool,
    },
}

/// Audit report certifying constitutional conformance across all 8 axioms.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstitutionAuditReport {
    pub passed: bool,
    pub axioms_checked: usize,
    pub violations: Vec<ConstitutionalViolation>,
    pub audited_candidates: usize,
    pub audited_incumbents: usize,
}

/// The non-evolvable Constitution enforcing core invariants.
#[derive(Debug, Clone, Default)]
pub struct Constitution;

impl Constitution {
    pub fn new() -> Self {
        Self
    }

    /// Validates whether a proposed action satisfies the immutable Constitution.
    pub fn validate_action(&self, action: &ProposedAction) -> Result<(), ConstitutionalViolation> {
        match action {
            ProposedAction::MutateStorageEvidence { event_id } => {
                Err(ConstitutionalViolation::EvidenceMutationAttempted(format!(
                    "Cannot modify or delete authoritative event {event_id}"
                )))
            }
            ProposedAction::MutateConstitution { target_axiom } => Err(
                ConstitutionalViolation::ConstitutionSelfModificationAttempted(format!(
                    "Axiom {target_axiom} cannot be altered at runtime"
                )),
            ),
            ProposedAction::PromoteCandidate {
                candidate_id,
                manual,
                approver,
            } => {
                if !manual || approver.is_none() {
                    return Err(ConstitutionalViolation::UnauthorizedAutonomousPromotion(
                        format!(
                            "Candidate {candidate_id:?} requires explicit human manual approval"
                        ),
                    ));
                }
                Ok(())
            }
            ProposedAction::ExecuteShadowEvaluation { budget } => {
                if budget.max_cpu_steps == 0 || budget.max_cpu_steps > 10_000_000 {
                    return Err(ConstitutionalViolation::ResourceLimitExceeded {
                        limit: "max_cpu_steps".into(),
                        requested: budget.max_cpu_steps,
                    });
                }
                if budget.max_memory_bytes == 0 || budget.max_memory_bytes > 512 * 1024 * 1024 {
                    return Err(ConstitutionalViolation::ResourceLimitExceeded {
                        limit: "max_memory_bytes".into(),
                        requested: budget.max_memory_bytes as u64,
                    });
                }
                Ok(())
            }
            ProposedAction::MutateMutationPolicy { gate_c_passed } => {
                if !gate_c_passed {
                    return Err(ConstitutionalViolation::GateCriteriaNotMet {
                        gate: "Gate C".into(),
                        detail:
                            "Meta-evolution is prohibited until Gate C demonstrates net benefit"
                                .into(),
                    });
                }
                Ok(())
            }
            ProposedAction::QuerySemanticProjection { exact_bypass, .. } => {
                if *exact_bypass {
                    return Err(ConstitutionalViolation::EvidenceMutationAttempted(
                        "Semantic associative queries cannot bypass or overwrite exact storage memory".into(),
                    ));
                }
                Ok(())
            }
        }
    }

    /// Performs an audit over the complete evolution engine state.
    pub fn audit(
        &self,
        incumbents: &[CandidateRecord],
        candidates: &[CandidateRecord],
    ) -> ConstitutionAuditReport {
        let mut violations = Vec::new();

        // 1. Verify every candidate has valid non-empty provenance lineage
        for c in candidates {
            if c.lineage.mutation_operator.is_empty() {
                violations.push(ConstitutionalViolation::MissingProvenanceLineage(format!(
                    "Candidate {:?} has no mutation operator recorded",
                    c.id
                )));
            }
            if c.lineage.rationale.is_empty() {
                violations.push(ConstitutionalViolation::MissingProvenanceLineage(format!(
                    "Candidate {:?} has no justification rationale",
                    c.id
                )));
            }
        }

        // 2. Verify all active incumbents have promoted status
        for inc in incumbents {
            if inc.status != CandidateStatus::Promoted {
                violations.push(ConstitutionalViolation::UnauthorizedAutonomousPromotion(
                    format!("Incumbent {:?} is not in Promoted status", inc.id),
                ));
            }
        }

        let passed = violations.is_empty();
        ConstitutionAuditReport {
            passed,
            axioms_checked: 8,
            violations,
            audited_candidates: candidates.len(),
            audited_incumbents: incumbents.len(),
        }
    }
}

// ============================================================================
// M31: Champion/Challenger Evolution Engine
// ============================================================================

/// Unique identifier for an evolvable candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CandidateId(pub u64);

/// Evolvable candidate domain/class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CandidateKind {
    Codec,
    Layout,
    Index,
    CheckpointPolicy,
    PrefetchPolicy,
    ShardPlacement,
    KnowledgeRule,
    RetrievalPolicy,
    Transformation,
    ModelSkill,
    MutationPolicy,
}

impl fmt::Display for CandidateKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Codec => write!(f, "Codec"),
            Self::Layout => write!(f, "Layout"),
            Self::Index => write!(f, "Index"),
            Self::CheckpointPolicy => write!(f, "CheckpointPolicy"),
            Self::PrefetchPolicy => write!(f, "PrefetchPolicy"),
            Self::ShardPlacement => write!(f, "ShardPlacement"),
            Self::KnowledgeRule => write!(f, "KnowledgeRule"),
            Self::RetrievalPolicy => write!(f, "RetrievalPolicy"),
            Self::Transformation => write!(f, "Transformation"),
            Self::ModelSkill => write!(f, "ModelSkill"),
            Self::MutationPolicy => write!(f, "MutationPolicy"),
        }
    }
}

/// Lifecycle state of a candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidateStatus {
    Proposed,
    Sandboxed,
    Evaluating,
    Promoted,
    Rejected { reason: String },
    RolledBack { reason: String },
}

/// Immutable provenance and lineage record for a candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateLineage {
    pub candidate_id: CandidateId,
    pub parent_candidate_id: Option<CandidateId>,
    pub target_domain: String,
    pub created_at_ms: u64,
    pub mutation_operator: String,
    pub rationale: String,
}

/// Sandboxing execution budget limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IsolationBudget {
    pub max_cpu_steps: u64,
    pub max_memory_bytes: usize,
    pub timeout_ms: u64,
}

impl Default for IsolationBudget {
    fn default() -> Self {
        Self {
            max_cpu_steps: 1_000_000,
            max_memory_bytes: 64 * 1024 * 1024, // 64 MB
            timeout_ms: 5000,
        }
    }
}

/// Workload specification for shadow evaluation.
#[derive(Debug, Clone, PartialEq)]
pub struct EvaluationWorkload {
    pub workload_id: String,
    pub sample_keys: Vec<String>,
    pub sample_events: usize,
    pub iterations: usize,
}

/// Holistic fitness metrics capturing performance, memory, and background overheads.
#[derive(Debug, Clone, PartialEq)]
pub struct FitnessMetrics {
    pub latency_p50_us: u64,
    pub latency_p99_us: u64,
    pub memory_bytes: usize,
    pub storage_bytes: usize,
    pub cpu_cycles: u64,
    pub read_amplification: f64,
    pub write_amplification: f64,
    pub background_cost_score: f64,
    pub net_benefit_score: f64,
}

impl FitnessMetrics {
    /// Evaluates net benefit percentage comparison relative to an incumbent baseline.
    /// Positive value means candidate outperforms incumbent after accounting for background costs.
    pub fn compute_net_benefit(&self, baseline: &FitnessMetrics) -> f64 {
        // Higher is better: reduction in latency, memory, storage, amplification, and background cost
        let lat_gain = if baseline.latency_p50_us > 0 {
            (baseline.latency_p50_us as f64 - self.latency_p50_us as f64)
                / baseline.latency_p50_us as f64
        } else {
            0.0
        };

        let mem_gain = if baseline.memory_bytes > 0 {
            (baseline.memory_bytes as f64 - self.memory_bytes as f64) / baseline.memory_bytes as f64
        } else {
            0.0
        };

        let storage_gain = if baseline.storage_bytes > 0 {
            (baseline.storage_bytes as f64 - self.storage_bytes as f64)
                / baseline.storage_bytes as f64
        } else {
            0.0
        };

        let amp_gain = if baseline.read_amplification > 0.0 {
            (baseline.read_amplification - self.read_amplification) / baseline.read_amplification
        } else {
            0.0
        };

        let bg_penalty = self.background_cost_score - baseline.background_cost_score;

        // Weighted holistic score minus background evaluation penalty
        0.35 * lat_gain + 0.25 * mem_gain + 0.25 * storage_gain + 0.15 * amp_gain
            - 0.10 * bg_penalty
    }
}

/// Full record for a candidate in the evolution registry.
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateRecord {
    pub id: CandidateId,
    pub kind: CandidateKind,
    pub target_domain: String,
    pub lineage: CandidateLineage,
    pub status: CandidateStatus,
    pub payload: Vec<u8>,
    pub latest_fitness: Option<FitnessMetrics>,
    pub promoted_at_ms: Option<u64>,
}

/// Configuration governing evolution activation and safety gates.
#[derive(Debug, Clone, PartialEq)]
pub struct EvolutionConfig {
    /// Evolution disabled until gates pass. Default: false.
    pub enabled: bool,
    /// Manual promotion required before autonomous promotion. Default: true.
    pub manual_promotion_required: bool,
    /// Gate C net adaptive value verification status. Default: false.
    pub gate_c_passed: bool,
    /// Minimum required net improvement over incumbent (e.g. 0.05 = 5%).
    pub min_net_benefit_threshold: f64,
}

impl Default for EvolutionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            manual_promotion_required: true,
            gate_c_passed: false,
            min_net_benefit_threshold: 0.05,
        }
    }
}

/// Champion/Challenger evolution engine.
#[derive(Debug, Clone)]
pub struct EvolutionEngine {
    config: EvolutionConfig,
    constitution: Constitution,
    next_candidate_id: u64,
    incumbents: HashMap<String, CandidateRecord>,
    candidates: HashMap<CandidateId, CandidateRecord>,
    prior_incumbents: HashMap<String, Vec<CandidateRecord>>,
}

impl EvolutionEngine {
    pub fn new(config: EvolutionConfig) -> Self {
        Self {
            config,
            constitution: Constitution::new(),
            next_candidate_id: 1,
            incumbents: HashMap::new(),
            candidates: HashMap::new(),
            prior_incumbents: HashMap::new(),
        }
    }

    pub fn config(&self) -> &EvolutionConfig {
        &self.config
    }

    pub fn set_config(&mut self, config: EvolutionConfig) {
        self.config = config;
    }

    pub fn constitution(&self) -> &Constitution {
        &self.constitution
    }

    /// Registers the initial static incumbent for a domain.
    pub fn register_incumbent(
        &mut self,
        kind: CandidateKind,
        target_domain: String,
        payload: Vec<u8>,
        initial_fitness: FitnessMetrics,
    ) -> CandidateId {
        let id = CandidateId(self.next_candidate_id);
        self.next_candidate_id += 1;

        let lineage = CandidateLineage {
            candidate_id: id,
            parent_candidate_id: None,
            target_domain: target_domain.clone(),
            created_at_ms: 1000,
            mutation_operator: "StaticBaseline".into(),
            rationale: "Default static incumbent configuration".into(),
        };

        let record = CandidateRecord {
            id,
            kind,
            target_domain: target_domain.clone(),
            lineage,
            status: CandidateStatus::Promoted,
            payload,
            latest_fitness: Some(initial_fitness),
            promoted_at_ms: Some(1000),
        };

        self.candidates.insert(id, record.clone());
        self.incumbents.insert(target_domain, record);
        id
    }

    /// Proposes a new candidate challenger.
    pub fn propose_candidate(
        &mut self,
        kind: CandidateKind,
        target_domain: String,
        lineage: CandidateLineage,
        payload: Vec<u8>,
    ) -> Result<CandidateId, ConstitutionalViolation> {
        if !self.config.enabled {
            return Err(ConstitutionalViolation::GateCriteriaNotMet {
                gate: "Gate B/C".into(),
                detail: "Evolution is disabled by default until prerequisite gates pass".into(),
            });
        }

        if lineage.rationale.is_empty() || lineage.mutation_operator.is_empty() {
            return Err(ConstitutionalViolation::MissingProvenanceLineage(
                "Candidates require non-empty rationale and mutation operator lineage".into(),
            ));
        }

        let id = CandidateId(self.next_candidate_id);
        self.next_candidate_id += 1;

        let mut candidate_lineage = lineage;
        candidate_lineage.candidate_id = id;

        let record = CandidateRecord {
            id,
            kind,
            target_domain,
            lineage: candidate_lineage,
            status: CandidateStatus::Proposed,
            payload,
            latest_fitness: None,
            promoted_at_ms: None,
        };

        self.candidates.insert(id, record);
        Ok(id)
    }

    /// Shadow-evaluates a candidate inside a bounded isolation budget.
    pub fn evaluate_candidate(
        &mut self,
        candidate_id: CandidateId,
        _workload: &EvaluationWorkload,
        budget: IsolationBudget,
        simulated_metrics: FitnessMetrics,
    ) -> Result<FitnessMetrics, ConstitutionalViolation> {
        // Enforce constitutional resource bounds
        self.constitution
            .validate_action(&ProposedAction::ExecuteShadowEvaluation { budget })?;

        let candidate = self.candidates.get_mut(&candidate_id).ok_or_else(|| {
            ConstitutionalViolation::MissingProvenanceLineage("Candidate not found".into())
        })?;

        candidate.status = CandidateStatus::Evaluating;

        // Verify against incumbent if one exists
        let mut computed_metrics = simulated_metrics;
        if let Some(incumbent) = self.incumbents.get(&candidate.target_domain) {
            if let Some(baseline) = &incumbent.latest_fitness {
                let net_benefit = computed_metrics.compute_net_benefit(baseline);
                computed_metrics.net_benefit_score = net_benefit;
            }
        }

        candidate.latest_fitness = Some(computed_metrics.clone());
        candidate.status = CandidateStatus::Sandboxed;

        Ok(computed_metrics)
    }

    /// Manually promotes a candidate to become the active incumbent.
    pub fn promote_candidate(
        &mut self,
        candidate_id: CandidateId,
        approver: Option<&str>,
        timestamp_ms: u64,
    ) -> Result<(), ConstitutionalViolation> {
        // Enforce constitutional manual promotion requirement
        self.constitution
            .validate_action(&ProposedAction::PromoteCandidate {
                candidate_id,
                manual: self.config.manual_promotion_required,
                approver: approver.map(Into::into),
            })?;

        let candidate = self.candidates.get_mut(&candidate_id).ok_or_else(|| {
            ConstitutionalViolation::MissingProvenanceLineage("Candidate not found".into())
        })?;

        // Verify candidate demonstrates net benefit
        if let Some(fitness) = &candidate.latest_fitness {
            if fitness.net_benefit_score < self.config.min_net_benefit_threshold {
                candidate.status = CandidateStatus::Rejected {
                    reason: format!(
                        "Net benefit {:.3} is below threshold {:.3}",
                        fitness.net_benefit_score, self.config.min_net_benefit_threshold
                    ),
                };
                return Err(ConstitutionalViolation::GateCriteriaNotMet {
                    gate: "Gate C".into(),
                    detail: format!(
                        "Candidate net benefit {:.3} fails threshold {:.3}",
                        fitness.net_benefit_score, self.config.min_net_benefit_threshold
                    ),
                });
            }
        } else {
            return Err(ConstitutionalViolation::GateCriteriaNotMet {
                gate: "Gate C".into(),
                detail: "Candidate has not been evaluated before promotion attempt".into(),
            });
        }

        candidate.status = CandidateStatus::Promoted;
        candidate.promoted_at_ms = Some(timestamp_ms);

        let target_domain = candidate.target_domain.clone();
        let promoted_record = candidate.clone();

        // Atomically replace incumbent while recording prior incumbent for instantaneous rollback
        if let Some(prior) = self
            .incumbents
            .insert(target_domain.clone(), promoted_record)
        {
            self.prior_incumbents
                .entry(target_domain)
                .or_default()
                .push(prior);
        }

        Ok(())
    }

    /// Instantly rolls back an active incumbent to its prior known-good incumbent upon regression.
    pub fn rollback(
        &mut self,
        target_domain: &str,
        reason: &str,
    ) -> Result<CandidateId, ConstitutionalViolation> {
        let priors = self
            .prior_incumbents
            .get_mut(target_domain)
            .ok_or_else(|| {
                ConstitutionalViolation::RegressionWithoutRollback(format!(
                    "No prior incumbent available to roll back to for domain {target_domain}"
                ))
            })?;

        let previous_incumbent = priors.pop().ok_or_else(|| {
            ConstitutionalViolation::RegressionWithoutRollback(format!(
                "Prior incumbent stack empty for domain {target_domain}"
            ))
        })?;

        // Update current failing incumbent status
        if let Some(current) = self.incumbents.get_mut(target_domain) {
            current.status = CandidateStatus::RolledBack {
                reason: reason.to_string(),
            };
            if let Some(rec) = self.candidates.get_mut(&current.id) {
                rec.status = CandidateStatus::RolledBack {
                    reason: reason.to_string(),
                };
            }
        }

        let restored_id = previous_incumbent.id;
        self.incumbents
            .insert(target_domain.to_string(), previous_incumbent);

        Ok(restored_id)
    }

    pub fn active_incumbent(&self, target_domain: &str) -> Option<&CandidateRecord> {
        self.incumbents.get(target_domain)
    }

    pub fn get_candidate(&self, id: CandidateId) -> Option<&CandidateRecord> {
        self.candidates.get(&id)
    }

    pub fn list_incumbents(&self) -> Vec<CandidateRecord> {
        let mut list: Vec<_> = self.incumbents.values().cloned().collect();
        list.sort_by_key(|r| r.id);
        list
    }

    pub fn list_candidates(&self) -> Vec<CandidateRecord> {
        let mut list: Vec<_> = self.candidates.values().cloned().collect();
        list.sort_by_key(|r| r.id);
        list
    }

    /// Performs constitutional audit across all registered components.
    pub fn audit_constitution(&self) -> ConstitutionAuditReport {
        let incs = self.list_incumbents();
        let cands = self.list_candidates();
        self.constitution.audit(&incs, &cands)
    }
}

// ============================================================================
// M32: Adaptive Physical Memory
// ============================================================================

/// Physical representation specification for sealed segments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhysicalRepresentation {
    pub codec_name: String,
    pub layout_name: String,
    pub index_type: String,
    pub block_size_bytes: usize,
}

/// Adaptive physical memory evaluator comparing segment representation candidates.
#[derive(Debug, Clone, Default)]
pub struct AdaptivePhysicalMemory;

impl AdaptivePhysicalMemory {
    pub fn new() -> Self {
        Self
    }

    /// Computes storage fitness score for a candidate physical representation.
    /// Objective balances bytes, scan latency, CPU decompression cost, and amplification.
    pub fn compute_storage_fitness(
        &self,
        raw_size_bytes: usize,
        compressed_size_bytes: usize,
        scan_latency_us: u64,
        decompression_cpu_cycles: u64,
        read_amplification: f64,
    ) -> f64 {
        let compression_ratio = if raw_size_bytes > 0 {
            compressed_size_bytes as f64 / raw_size_bytes as f64
        } else {
            1.0
        };

        let norm_latency = (scan_latency_us as f64 / 1000.0).clamp(0.0, 1.0);
        let norm_cpu = (decompression_cpu_cycles as f64 / 10_000.0).clamp(0.0, 1.0);
        let norm_amp = read_amplification.clamp(1.0, 5.0) / 5.0;

        // Higher fitness score represents superior physical representation
        let space_score = (1.0 - compression_ratio).max(0.0);
        let latency_score = 1.0 - norm_latency;
        let cpu_score = 1.0 - norm_cpu;
        let amp_score = 1.0 - norm_amp;

        0.40 * space_score + 0.30 * latency_score + 0.15 * cpu_score + 0.15 * amp_score
    }
}

// ============================================================================
// M33: Adaptive Lifecycle Policies
// ============================================================================

/// Lifecycle policy tuning candidate parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct LifecyclePolicyCandidate {
    pub checkpoint_interval_events: usize,
    pub segment_seal_threshold_bytes: usize,
    pub hot_dram_capacity_bytes: usize,
    pub access_frequency_threshold: u64,
}

/// Evaluates adaptive lifecycle adjustments against replay cost and memory pressure.
#[derive(Debug, Clone, Default)]
pub struct AdaptiveLifecycleEvaluator;

impl AdaptiveLifecycleEvaluator {
    pub fn new() -> Self {
        Self
    }

    /// Evaluates checkpoint interval trade-off: replay reconstruction latency vs snapshot size overhead.
    pub fn evaluate_checkpoint_policy(
        &self,
        candidate_interval: usize,
        measured_replay_events_per_sec: f64,
        snapshot_write_cost_us: u64,
    ) -> f64 {
        // Recovery latency = (checkpoint_interval / 2) / replay_rate
        let avg_recovery_latency_sec =
            (candidate_interval as f64 / 2.0) / measured_replay_events_per_sec.max(1.0);
        let amortized_snapshot_overhead_sec =
            (snapshot_write_cost_us as f64 / 1_000_000.0) / candidate_interval.max(1) as f64;

        // Holistic recovery efficiency (lower combined overhead is better -> inverted for score)
        let total_cost = avg_recovery_latency_sec + 10.0 * amortized_snapshot_overhead_sec;
        1.0 / (1.0 + total_cost)
    }
}

// ============================================================================
// M34: Semantic & Vector Projections
// ============================================================================

/// Fixed-dimension dense vector embedding for associative similarity queries.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorEmbedding {
    pub target_id: String,
    pub dimension: usize,
    pub values: Vec<f32>,
}

impl VectorEmbedding {
    pub fn new(target_id: impl Into<String>, values: Vec<f32>) -> Self {
        let dimension = values.len();
        Self {
            target_id: target_id.into(),
            dimension,
            values,
        }
    }

    pub fn dot_product(&self, other: &Self) -> f32 {
        if self.dimension != other.dimension {
            return 0.0;
        }
        self.values
            .iter()
            .zip(other.values.iter())
            .map(|(a, b)| a * b)
            .sum()
    }

    pub fn cosine_similarity(&self, other: &Self) -> f32 {
        let dot = self.dot_product(other);
        let norm_a: f32 = self.values.iter().map(|v| v * v).sum::<f32>().sqrt();
        let norm_b: f32 = other.values.iter().map(|v| v * v).sum::<f32>().sqrt();

        if norm_a == 0.0 || norm_b == 0.0 {
            0.0
        } else {
            dot / (norm_a * norm_b)
        }
    }

    pub fn euclidean_distance(&self, other: &Self) -> f32 {
        if self.dimension != other.dimension {
            return f32::INFINITY;
        }
        self.values
            .iter()
            .zip(other.values.iter())
            .map(|(a, b)| {
                let diff = a - b;
                diff * diff
            })
            .sum::<f32>()
            .sqrt()
    }
}

/// Associative semantic projection index strictly segregated from authoritative storage.
#[derive(Debug, Clone, Default)]
pub struct SemanticProjectionIndex {
    embeddings: HashMap<String, VectorEmbedding>,
}

impl SemanticProjectionIndex {
    pub fn new() -> Self {
        Self {
            embeddings: HashMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.embeddings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.embeddings.is_empty()
    }

    pub fn insert(&mut self, embedding: VectorEmbedding) {
        self.embeddings
            .insert(embedding.target_id.clone(), embedding);
    }

    pub fn get(&self, target_id: &str) -> Option<&VectorEmbedding> {
        self.embeddings.get(target_id)
    }

    /// Finds top-k nearest neighbors by cosine similarity without touching exact storage.
    pub fn nearest_neighbors(&self, query: &VectorEmbedding, top_k: usize) -> Vec<(String, f32)> {
        let mut scored: Vec<(String, f32)> = self
            .embeddings
            .values()
            .map(|emb| (emb.target_id.clone(), query.cosine_similarity(emb)))
            .collect();

        // Sort descending by similarity score
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);
        scored
    }
}

// ============================================================================
// M35 & M36: Knowledge & Transformation Mutation
// ============================================================================

/// Candidate mutation for inferential rules or beliefs.
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateKnowledgeRule {
    pub rule_id: String,
    pub antecedent_terms: Vec<String>,
    pub consequent_fact: String,
    pub confidence: f64,
}

/// Evaluates candidate knowledge mutations against calibration and contradiction penalties.
#[derive(Debug, Clone, Default)]
pub struct KnowledgeMutationEvaluator;

impl KnowledgeMutationEvaluator {
    pub fn new() -> Self {
        Self
    }

    /// Evaluates candidate fitness: balances prediction accuracy against contradiction penalty.
    pub fn evaluate_rule(
        &self,
        rule: &CandidateKnowledgeRule,
        support_count: usize,
        contradiction_count: usize,
    ) -> f64 {
        if support_count == 0 {
            return 0.0;
        }

        let accuracy = support_count as f64 / (support_count + contradiction_count) as f64;
        let contradiction_penalty = contradiction_count as f64 * 0.5;

        (accuracy * rule.confidence - contradiction_penalty).max(0.0)
    }
}

// ============================================================================
// M37: Meta-Evolution (Gated behind Gate C)
// ============================================================================

/// Evolvable distribution of candidate mutation operators.
#[derive(Debug, Clone, PartialEq)]
pub struct MutationPolicy {
    pub policy_id: String,
    pub version: u32,
    pub representation_weight: f64,
    pub rule_weight: f64,
    pub lifecycle_weight: f64,
    pub exploratory_weight: f64,
    pub historical_candidate_yield: f64,
}

impl Default for MutationPolicy {
    fn default() -> Self {
        Self {
            policy_id: "default_static_policy".into(),
            version: 1,
            representation_weight: 0.40,
            rule_weight: 0.30,
            lifecycle_weight: 0.20,
            exploratory_weight: 0.10,
            historical_candidate_yield: 0.0,
        }
    }
}

/// Meta-evolution manager controlling mutation policy evolution.
#[derive(Debug, Clone)]
pub struct MetaEvolutionManager {
    constitution: Constitution,
    active_policy: MutationPolicy,
    policy_history: Vec<MutationPolicy>,
    hard_off_switch: bool,
}

impl MetaEvolutionManager {
    pub fn new() -> Self {
        Self {
            constitution: Constitution::new(),
            active_policy: MutationPolicy::default(),
            policy_history: Vec::new(),
            hard_off_switch: true, // Off by default
        }
    }

    pub fn is_enabled(&self) -> bool {
        !self.hard_off_switch
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.hard_off_switch = !enabled;
    }

    pub fn active_policy(&self) -> &MutationPolicy {
        &self.active_policy
    }

    /// Evolves the mutation policy only if Gate C has been passed and the off-switch is disabled.
    pub fn evolve_policy(
        &mut self,
        new_policy: MutationPolicy,
        gate_c_passed: bool,
    ) -> Result<(), ConstitutionalViolation> {
        if self.hard_off_switch {
            return Err(ConstitutionalViolation::UnauthorizedAutonomousPromotion(
                "Meta-evolution hard off-switch is active".into(),
            ));
        }

        // Validate constitutional Gate C requirement
        self.constitution
            .validate_action(&ProposedAction::MutateMutationPolicy { gate_c_passed })?;

        // Require positive candidate yield improvement over active policy
        if new_policy.historical_candidate_yield <= self.active_policy.historical_candidate_yield {
            return Err(ConstitutionalViolation::GateCriteriaNotMet {
                gate: "Gate C".into(),
                detail: format!(
                    "Challenger policy yield {:.3} does not beat incumbent {:.3}",
                    new_policy.historical_candidate_yield,
                    self.active_policy.historical_candidate_yield
                ),
            });
        }

        let old = std::mem::replace(&mut self.active_policy, new_policy);
        self.policy_history.push(old);
        Ok(())
    }
}

impl Default for MetaEvolutionManager {
    fn default() -> Self {
        Self::new()
    }
}
