// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]

//! Epistemic Knowledge Store (EKS), truth maintenance, provenance, and predictive calibration.
//!
//! # Architecture
//! Following Temnion Architecture §9:
//! - **Knowledge Primitives (M25)**: Distinct derived representations (`Observation`, `Claim`,
//!   `Belief`, `Concept`, `Rule`, `ModelManifest`, `Skill`) that reference exact `EventId`
//!   evidence without overwriting ground truth.
//! - **Provenance & Truth Maintenance (M26)**: Non-destructive justification network tracking
//!   dependencies, assumptions, and contradictions. `WHY` queries traverse justifications back
//!   to exact evidence.
//! - **Predictive Knowledge & Calibration (M27)**: Predictions linked to outcomes, Brier score
//!   calibration, and strict known-as-of cutoff isolation (zero future leakage).
//! - **Knowledge Consolidation (M28)**: Tiered memory (`Active`, `Reference`, `Archive`)
//!   consolidating episodes into patterns, concepts, and rules.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::ops::Range;

use temnion_core::{EventId, Timestamp};

// ---------------------------------------------------------------------------
// Knowledge Primitives (M25)
// ---------------------------------------------------------------------------

/// Unique identifier for an epistemic knowledge record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KnowledgeId(pub u64);

impl fmt::Display for KnowledgeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "k:{}", self.0)
    }
}

/// Epistemic confidence score bounded in [0.0, 1.0].
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Confidence(f64);

impl Confidence {
    pub const CERTAIN: Self = Self(1.0);
    pub const UNCERTAIN: Self = Self(0.5);
    pub const IMPOSSIBLE: Self = Self(0.0);

    pub fn new(value: f64) -> Result<Self, String> {
        if value.is_nan() || !(0.0..=1.0).contains(&value) {
            Err(format!("Confidence must be within [0.0, 1.0], got {value}"))
        } else {
            Ok(Self(value))
        }
    }

    pub fn value(self) -> f64 {
        self.0
    }
}

impl Default for Confidence {
    fn default() -> Self {
        Self::CERTAIN
    }
}

/// Lifecycle status of an epistemic knowledge item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KnowledgeStatus {
    /// Active, currently justified belief.
    Active,
    /// Explicitly contradicted by opposing evidence or claims.
    Contradicted,
    /// Retracted due to revoked premise or assumption.
    Retracted,
    /// Superseded by a refined, newer belief or consolidated concept.
    Superseded,
}

/// Raw empirical perception grounded in an authoritative storage event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    pub id: KnowledgeId,
    pub event_ref: EventId,
    pub valid_time: Timestamp,
    pub known_time: Timestamp,
    pub feature: String,
    pub value_repr: String,
}

/// Asserted proposition from an external source or agent.
#[derive(Debug, Clone, PartialEq)]
pub struct Claim {
    pub id: KnowledgeId,
    pub source: String,
    pub proposition: String,
    pub confidence: Confidence,
    pub valid_range: Range<Timestamp>,
    pub asserted_at: Timestamp,
}

/// Inferred state of affairs supported by explicit justifications.
#[derive(Debug, Clone, PartialEq)]
pub struct Belief {
    pub id: KnowledgeId,
    pub proposition: String,
    pub confidence: Confidence,
    pub status: KnowledgeStatus,
    pub valid_time: Timestamp,
    pub learned_time: Timestamp,
    pub justification: Justification,
}

/// Abstract entity or category derived from recurring patterns.
#[derive(Debug, Clone, PartialEq)]
pub struct Concept {
    pub id: KnowledgeId,
    pub name: String,
    pub definition: String,
    pub generalization_level: u32,
    pub exemplar_evidence: Vec<EventId>,
    pub created_at: Timestamp,
}

/// Inferred implication (`IF <antecedent> THEN <consequent>`).
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub id: KnowledgeId,
    pub name: String,
    pub antecedent: String,
    pub consequent: String,
    pub confidence: Confidence,
    pub support_count: u64,
    pub version: u32,
}

/// Declarative metadata for an external or isolated reasoning model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelManifest {
    pub id: KnowledgeId,
    pub model_name: String,
    pub runtime: String,
    pub version: String,
    pub is_stateful: bool,
    pub max_memory_bytes: u64,
}

/// Reusable procedural capability or recipe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    pub id: KnowledgeId,
    pub name: String,
    pub intent: String,
    pub steps: Vec<String>,
    pub version: u32,
}

// ---------------------------------------------------------------------------
// Truth Maintenance & Provenance Traversal (M26)
// ---------------------------------------------------------------------------

/// Non-destructive justification structure linking a belief to its premises.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Justification {
    /// Direct supporting event IDs from the authoritative WAL/TSF.
    pub evidence_events: Vec<EventId>,
    /// Intermediate premises (other beliefs/claims).
    pub premises: Vec<KnowledgeId>,
    /// Applied inferential rules.
    pub applied_rules: Vec<KnowledgeId>,
    /// Explicit defeasible assumptions.
    pub assumptions: Vec<String>,
}

/// First-class record of contradiction between conflicting claims or beliefs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contradiction {
    pub item_a: KnowledgeId,
    pub item_b: KnowledgeId,
    pub detected_at: Timestamp,
    pub reason: String,
}

/// Provenance tree returned by a `WHY` query, tracing a belief to its origins.
#[derive(Debug, Clone, PartialEq)]
pub struct WhyTrace {
    pub target_id: KnowledgeId,
    pub proposition: String,
    pub confidence: Confidence,
    pub status: KnowledgeStatus,
    pub direct_evidence: Vec<EventId>,
    pub applied_rules: Vec<Rule>,
    pub assumptions: Vec<String>,
    pub premise_traces: Vec<WhyTrace>,
    pub contradictions: Vec<Contradiction>,
}

/// Epistemic Truth Maintenance System (TMS) tracking justifications and contradictions.
#[derive(Debug, Default)]
pub struct TruthMaintenanceSystem {
    next_id: u64,
    observations: BTreeMap<KnowledgeId, Observation>,
    claims: BTreeMap<KnowledgeId, Claim>,
    beliefs: BTreeMap<KnowledgeId, Belief>,
    concepts: BTreeMap<KnowledgeId, Concept>,
    rules: BTreeMap<KnowledgeId, Rule>,
    models: BTreeMap<KnowledgeId, ModelManifest>,
    skills: BTreeMap<KnowledgeId, Skill>,
    contradictions: Vec<Contradiction>,
}

impl TruthMaintenanceSystem {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            ..Default::default()
        }
    }

    fn alloc_id(&mut self) -> KnowledgeId {
        let id = KnowledgeId(self.next_id);
        self.next_id += 1;
        id
    }

    /// Records an empirical observation grounded in an authoritative storage event.
    pub fn record_observation(
        &mut self,
        event_ref: EventId,
        valid_time: Timestamp,
        known_time: Timestamp,
        feature: impl Into<String>,
        value_repr: impl Into<String>,
    ) -> KnowledgeId {
        let id = self.alloc_id();
        self.observations.insert(
            id,
            Observation {
                id,
                event_ref,
                valid_time,
                known_time,
                feature: feature.into(),
                value_repr: value_repr.into(),
            },
        );
        id
    }

    /// Asserts a claim with source attribution and validity interval.
    pub fn assert_claim(
        &mut self,
        source: impl Into<String>,
        proposition: impl Into<String>,
        confidence: Confidence,
        valid_range: Range<Timestamp>,
        asserted_at: Timestamp,
    ) -> KnowledgeId {
        let id = self.alloc_id();
        self.claims.insert(
            id,
            Claim {
                id,
                source: source.into(),
                proposition: proposition.into(),
                confidence,
                valid_range,
                asserted_at,
            },
        );
        id
    }

    /// Infers a new belief with explicit justifications.
    pub fn infer_belief(
        &mut self,
        proposition: impl Into<String>,
        confidence: Confidence,
        valid_time: Timestamp,
        learned_time: Timestamp,
        justification: Justification,
    ) -> KnowledgeId {
        let id = self.alloc_id();
        let prop_str = proposition.into();

        // Check if any existing active belief contradicts this proposition
        let mut conflicting = Vec::new();
        for (other_id, existing) in &self.beliefs {
            if existing.status == KnowledgeStatus::Active
                && is_contradictory(&existing.proposition, &prop_str)
            {
                conflicting.push(*other_id);
            }
        }

        let status = if conflicting.is_empty() {
            KnowledgeStatus::Active
        } else {
            KnowledgeStatus::Contradicted
        };

        for other_id in &conflicting {
            self.contradictions.push(Contradiction {
                item_a: *other_id,
                item_b: id,
                detected_at: learned_time,
                reason: format!(
                    "Proposition '{}' contradicts '{}'",
                    prop_str, self.beliefs[other_id].proposition
                ),
            });
            // Mark existing belief as contradicted as well
            if let Some(b) = self.beliefs.get_mut(other_id) {
                b.status = KnowledgeStatus::Contradicted;
            }
        }

        self.beliefs.insert(
            id,
            Belief {
                id,
                proposition: prop_str,
                confidence,
                status,
                valid_time,
                learned_time,
                justification,
            },
        );

        id
    }

    /// Defines an inferential rule.
    pub fn define_rule(
        &mut self,
        name: impl Into<String>,
        antecedent: impl Into<String>,
        consequent: impl Into<String>,
        confidence: Confidence,
    ) -> KnowledgeId {
        let id = self.alloc_id();
        self.rules.insert(
            id,
            Rule {
                id,
                name: name.into(),
                antecedent: antecedent.into(),
                consequent: consequent.into(),
                confidence,
                support_count: 0,
                version: 1,
            },
        );
        id
    }

    /// Registers a consolidated concept.
    pub fn register_concept(&mut self, mut concept: Concept) -> KnowledgeId {
        let id = self.alloc_id();
        concept.id = id;
        self.concepts.insert(id, concept);
        id
    }

    /// Looks up a concept by ID.
    pub fn get_concept(&self, id: KnowledgeId) -> Option<&Concept> {
        self.concepts.get(&id)
    }

    /// Registers an isolated worker model manifest.
    pub fn register_model(&mut self, mut model: ModelManifest) -> KnowledgeId {
        let id = self.alloc_id();
        model.id = id;
        self.models.insert(id, model);
        id
    }

    /// Looks up a model manifest by ID.
    pub fn get_model(&self, id: KnowledgeId) -> Option<&ModelManifest> {
        self.models.get(&id)
    }

    /// Registers an executable agent skill.
    pub fn register_skill(&mut self, mut skill: Skill) -> KnowledgeId {
        let id = self.alloc_id();
        skill.id = id;
        self.skills.insert(id, skill);
        id
    }

    /// Looks up a skill by ID.
    pub fn get_skill(&self, id: KnowledgeId) -> Option<&Skill> {
        self.skills.get(&id)
    }

    /// Looks up an inferential rule by ID.
    pub fn get_rule(&self, id: KnowledgeId) -> Option<&Rule> {
        self.rules.get(&id)
    }

    /// Retracts a belief non-destructively, propagating status to dependent beliefs.
    pub fn retract_belief(&mut self, id: KnowledgeId) -> Result<(), String> {
        let belief = self
            .beliefs
            .get_mut(&id)
            .ok_or_else(|| format!("Belief {id} not found"))?;
        belief.status = KnowledgeStatus::Retracted;

        // Cascade retraction to all beliefs having `id` as a premise
        let dependents: Vec<KnowledgeId> = self
            .beliefs
            .iter()
            .filter(|(_, b)| b.justification.premises.contains(&id))
            .map(|(k, _)| *k)
            .collect();

        for dep_id in dependents {
            let _ = self.retract_belief(dep_id);
        }

        Ok(())
    }

    /// Evaluates a `WHY` query, recursively building the full justification provenance tree.
    pub fn why(&self, target_id: KnowledgeId) -> Result<WhyTrace, String> {
        let belief = self
            .beliefs
            .get(&target_id)
            .ok_or_else(|| format!("Belief {target_id} not found"))?;

        let mut applied_rules = Vec::new();
        for r_id in &belief.justification.applied_rules {
            if let Some(r) = self.rules.get(r_id) {
                applied_rules.push(r.clone());
            }
        }

        let mut premise_traces = Vec::new();
        for p_id in &belief.justification.premises {
            if self.beliefs.contains_key(p_id) {
                premise_traces.push(self.why(*p_id)?);
            }
        }

        let relevant_contradictions: Vec<Contradiction> = self
            .contradictions
            .iter()
            .filter(|c| c.item_a == target_id || c.item_b == target_id)
            .cloned()
            .collect();

        Ok(WhyTrace {
            target_id,
            proposition: belief.proposition.clone(),
            confidence: belief.confidence,
            status: belief.status,
            direct_evidence: belief.justification.evidence_events.clone(),
            applied_rules,
            assumptions: belief.justification.assumptions.clone(),
            premise_traces,
            contradictions: relevant_contradictions,
        })
    }

    /// Returns active beliefs filtered by a known-as-of temporal cutoff.
    /// Guarantees zero future-leakage: only beliefs learned at or before `cutoff` are returned.
    pub fn active_beliefs_known_as_of(&self, cutoff: Timestamp) -> Vec<&Belief> {
        self.beliefs
            .values()
            .filter(|b| b.status == KnowledgeStatus::Active && b.learned_time.ticks <= cutoff.ticks)
            .collect()
    }

    pub fn beliefs_count(&self) -> usize {
        self.beliefs.len()
    }

    pub fn contradictions(&self) -> &[Contradiction] {
        &self.contradictions
    }
}

fn is_contradictory(a: &str, b: &str) -> bool {
    let a_clean = a.trim().to_lowercase();
    let b_clean = b.trim().to_lowercase();

    // Direct negation detection
    if b_clean == format!("not {a_clean}") || a_clean == format!("not {b_clean}") {
        return true;
    }

    // Antonym pairs
    let antonyms = [
        ("true", "false"),
        ("alive", "dead"),
        ("active", "inactive"),
        ("healthy", "critical"),
        ("open", "closed"),
    ];

    for (x, y) in antonyms {
        if (a_clean.contains(x) && b_clean.contains(y))
            || (a_clean.contains(y) && b_clean.contains(x))
        {
            return true;
        }
    }

    false
}

// ---------------------------------------------------------------------------
// Predictive Knowledge & Calibration (M27)
// ---------------------------------------------------------------------------

/// Outcome assessment of an empirical prediction.
#[derive(Debug, Clone, PartialEq)]
pub enum PredictionOutcome {
    Pending,
    Matched {
        observed_at: Timestamp,
    },
    Refuted {
        observed_at: Timestamp,
        actual_value: String,
    },
    Inconclusive,
}

/// Anticipated future state emitted by a model or rule.
#[derive(Debug, Clone, PartialEq)]
pub struct Prediction {
    pub id: u64,
    pub model_ref: KnowledgeId,
    pub target_entity_shard: u32,
    pub target_entity_slot: u32,
    pub target_valid_time: Timestamp,
    pub predicted_value: String,
    pub confidence: Confidence,
    pub emitted_at_known_time: Timestamp,
    pub outcome: PredictionOutcome,
}

/// Ledger tracking predictions, outcomes, and empirical calibration scores.
#[derive(Debug, Default)]
pub struct PredictionLedger {
    next_id: u64,
    predictions: BTreeMap<u64, Prediction>,
}

impl PredictionLedger {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            predictions: BTreeMap::new(),
        }
    }

    /// Emits a new prediction with an explicit valid time horizon.
    #[allow(clippy::too_many_arguments)]
    pub fn record_prediction(
        &mut self,
        model_ref: KnowledgeId,
        shard: u32,
        slot: u32,
        target_valid_time: Timestamp,
        predicted_value: impl Into<String>,
        confidence: Confidence,
        emitted_at: Timestamp,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.predictions.insert(
            id,
            Prediction {
                id,
                model_ref,
                target_entity_shard: shard,
                target_entity_slot: slot,
                target_valid_time,
                predicted_value: predicted_value.into(),
                confidence,
                emitted_at_known_time: emitted_at,
                outcome: PredictionOutcome::Pending,
            },
        );
        id
    }

    /// Resolves an outcome when the actual event occurs.
    pub fn resolve_outcome(
        &mut self,
        prediction_id: u64,
        observed_value: &str,
        observed_at: Timestamp,
    ) -> Result<(), String> {
        let pred = self
            .predictions
            .get_mut(&prediction_id)
            .ok_or_else(|| format!("Prediction {prediction_id} not found"))?;

        if pred
            .predicted_value
            .eq_ignore_ascii_case(observed_value.trim())
        {
            pred.outcome = PredictionOutcome::Matched { observed_at };
        } else {
            pred.outcome = PredictionOutcome::Refuted {
                observed_at,
                actual_value: observed_value.to_string(),
            };
        }
        Ok(())
    }

    /// Computes Brier score calibration for resolved predictions known at `cutoff`.
    /// Brier score = (1/N) * sum((forecast - actual)^2).
    /// Lower is better: 0.0 is perfect calibration, 1.0 is complete miscalibration.
    /// Strictly guarantees zero future leakage.
    pub fn compute_brier_score(&self, cutoff: Timestamp) -> Result<f64, String> {
        let mut count = 0usize;
        let mut sum_squared_error = 0.0f64;

        for pred in self.predictions.values() {
            // Must have been emitted prior to cutoff
            if pred.emitted_at_known_time.ticks > cutoff.ticks {
                continue;
            }

            match &pred.outcome {
                PredictionOutcome::Matched { observed_at } if observed_at.ticks <= cutoff.ticks => {
                    let forecast = pred.confidence.value();
                    let actual = 1.0f64;
                    sum_squared_error += (forecast - actual).powi(2);
                    count += 1;
                }
                PredictionOutcome::Refuted { observed_at, .. }
                    if observed_at.ticks <= cutoff.ticks =>
                {
                    let forecast = pred.confidence.value();
                    let actual = 0.0f64;
                    sum_squared_error += (forecast - actual).powi(2);
                    count += 1;
                }
                _ => {
                    // Pending or observed after cutoff -> excluded from known-as-of score
                }
            }
        }

        if count == 0 {
            Err("No resolved predictions available at the requested cutoff time".to_string())
        } else {
            Ok(sum_squared_error / count as f64)
        }
    }

    pub fn count(&self) -> usize {
        self.predictions.len()
    }
}

// ---------------------------------------------------------------------------
// Knowledge Consolidation Tiers (M28)
// ---------------------------------------------------------------------------

/// Memory tiers for bound knowledge scaling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KnowledgeTier {
    /// Hot working memory for foreground prediction and reasoning.
    Active,
    /// Consolidated patterns, validated concepts, and generalized rules.
    Reference,
    /// Historical beliefs and superseded hypotheses retained for provenance.
    Archive,
}

/// Episode: temporal sequence of observations.
#[derive(Debug, Clone, PartialEq)]
pub struct Episode {
    pub id: u64,
    pub label: String,
    pub observations: Vec<KnowledgeId>,
    pub start_time: Timestamp,
    pub end_time: Timestamp,
}

/// Pattern: generalized frequent structure mined across episodes.
#[derive(Debug, Clone, PartialEq)]
pub struct Pattern {
    pub id: u64,
    pub signature: String,
    pub occurrence_count: u64,
    pub derived_concept: Option<String>,
}

/// Tiered knowledge manager organizing Active, Reference, and Archive tiers.
#[derive(Debug)]
pub struct TieredKnowledgeStore {
    max_active_items: usize,
    tier_map: HashMap<KnowledgeId, KnowledgeTier>,
    access_counts: HashMap<KnowledgeId, u64>,
    episodes: Vec<Episode>,
    patterns: Vec<Pattern>,
}

impl TieredKnowledgeStore {
    pub fn new(max_active_items: usize) -> Self {
        Self {
            max_active_items,
            tier_map: HashMap::new(),
            access_counts: HashMap::new(),
            episodes: Vec::new(),
            patterns: Vec::new(),
        }
    }

    /// Places a new knowledge item in the Active tier.
    pub fn insert_active(&mut self, id: KnowledgeId) {
        self.tier_map.insert(id, KnowledgeTier::Active);
        self.access_counts.insert(id, 1);
        self.evict_if_needed();
    }

    pub fn record_access(&mut self, id: KnowledgeId) {
        if let Some(c) = self.access_counts.get_mut(&id) {
            *c += 1;
        }
    }

    pub fn tier_of(&self, id: KnowledgeId) -> Option<KnowledgeTier> {
        self.tier_map.get(&id).copied()
    }

    /// Consolidates an episode into a pattern and promotes to Reference tier.
    pub fn consolidate_episode(&mut self, episode: Episode, signature: impl Into<String>) -> u64 {
        let pat_id = (self.patterns.len() + 1) as u64;
        let sig = signature.into();

        for obs_id in &episode.observations {
            // Observations in consolidated episodes transition to Reference tier
            self.tier_map.insert(*obs_id, KnowledgeTier::Reference);
        }

        self.patterns.push(Pattern {
            id: pat_id,
            signature: sig,
            occurrence_count: 1,
            derived_concept: Some(episode.label.clone()),
        });

        self.episodes.push(episode);
        pat_id
    }

    fn evict_if_needed(&mut self) {
        let active_count = self
            .tier_map
            .values()
            .filter(|t| **t == KnowledgeTier::Active)
            .count();

        if active_count > self.max_active_items {
            // Find lowest accessed active item and demote to Archive (tie-break by ID for determinism)
            let mut active_items: Vec<(KnowledgeId, u64)> = self
                .tier_map
                .iter()
                .filter(|(_, t)| **t == KnowledgeTier::Active)
                .map(|(&id, _)| (id, self.access_counts.get(&id).copied().unwrap_or(0)))
                .collect();

            active_items.sort_by_key(|(id, count)| (*count, id.0));

            if let Some((victim, _)) = active_items.first() {
                self.tier_map.insert(*victim, KnowledgeTier::Archive);
            }
        }
    }

    pub fn patterns(&self) -> &[Pattern] {
        &self.patterns
    }

    pub fn episodes(&self) -> &[Episode] {
        &self.episodes
    }
}
