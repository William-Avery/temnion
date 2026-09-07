// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]

//! Deterministic transformation engine (M29) and canonical IR e-graph optimizer (M30).
//!
//! # Architecture
//! Following Temnion Architecture §9 and Tasks T14–T15:
//! - **Transformation Engine (M29)**: Deterministic, versioned transformations with declared
//!   input/output signatures, explicit preconditions, CPU/memory resource metering, and
//!   immutable provenance lineage records.
//! - **E-Graph Optimizer (M30)**: Canonical query IR equality saturation, algebraic rewrite
//!   rules (boolean identities, constant folding, redundant filter elimination), and
//!   cost-based plan extraction with verified semantic equivalence.

use std::collections::{HashMap, HashSet};
use std::fmt;

use temnion_core::{EntityId, EventId, Timestamp};
use temnion_eks::{Episode, KnowledgeId, Observation, TieredKnowledgeStore};
use temnion_query::{BinaryOp, Expr, Literal, LogicalPlan, QueryError};

// ---------------------------------------------------------------------------
// Transformation Engine (M29)
// ---------------------------------------------------------------------------

/// Unique identifier for a registered transformation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TransformationId(pub u64);

impl fmt::Display for TransformationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "tx:{}", self.0)
    }
}

/// Version tag for a transformation manifest.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VersionTag {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl VersionTag {
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

impl fmt::Display for VersionTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Declared input and output types for a transformation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransformationKind {
    /// Perceptual feature extraction (Event -> Observation).
    EventToObservation,
    /// Episode pattern consolidation (Episode -> Pattern).
    EpisodeToPattern,
    /// Deductive / inductive belief inference (Claims/Observations -> Belief).
    BeliefInference,
    /// Algebraic expression reduction / normalization.
    ExpressionOptimization,
}

/// Resource constraints and budget ceilings for transformation execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransformationBudget {
    pub max_cpu_steps: u64,
    pub max_memory_bytes: usize,
}

impl Default for TransformationBudget {
    fn default() -> Self {
        Self {
            max_cpu_steps: 100_000,
            max_memory_bytes: 10 * 1024 * 1024, // 10 MiB
        }
    }
}

/// Manifest describing a versioned, reproducible transformation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransformationManifest {
    pub id: TransformationId,
    pub name: String,
    pub version: VersionTag,
    pub kind: TransformationKind,
    pub description: String,
    pub budget: TransformationBudget,
}

/// Resource consumption receipt for an executed transformation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionReceipt {
    pub steps_consumed: u64,
    pub memory_allocated_bytes: usize,
    pub success: bool,
}

/// Immutable lineage record linking outputs to inputs and the executing transformation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineageRecord {
    pub transformation_id: TransformationId,
    pub transformation_version: VersionTag,
    pub input_events: Vec<EventId>,
    pub input_knowledge: Vec<KnowledgeId>,
    pub output_knowledge: Vec<KnowledgeId>,
    pub executed_at: Timestamp,
    pub receipt: ExecutionReceipt,
}

/// Resource meter tracking execution steps and allocated memory.
#[derive(Debug)]
pub struct ResourceMeter {
    budget: TransformationBudget,
    steps_used: u64,
    bytes_used: usize,
}

impl ResourceMeter {
    pub fn new(budget: TransformationBudget) -> Self {
        Self {
            budget,
            steps_used: 0,
            bytes_used: 0,
        }
    }

    pub fn consume_steps(&mut self, steps: u64) -> Result<(), String> {
        self.steps_used = self.steps_used.saturating_add(steps);
        if self.steps_used > self.budget.max_cpu_steps {
            Err(format!(
                "CPU step budget exceeded: used {}, limit {}",
                self.steps_used, self.budget.max_cpu_steps
            ))
        } else {
            Ok(())
        }
    }

    pub fn track_memory(&mut self, bytes: usize) -> Result<(), String> {
        self.bytes_used = self.bytes_used.saturating_add(bytes);
        if self.bytes_used > self.budget.max_memory_bytes {
            Err(format!(
                "Memory budget exceeded: allocated {} B, limit {} B",
                self.bytes_used, self.budget.max_memory_bytes
            ))
        } else {
            Ok(())
        }
    }

    pub fn receipt(&self, success: bool) -> ExecutionReceipt {
        ExecutionReceipt {
            steps_consumed: self.steps_used,
            memory_allocated_bytes: self.bytes_used,
            success,
        }
    }
}

/// Transformation engine orchestrating reproducible, metered pipeline executions.
pub struct TransformationEngine {
    manifests: HashMap<TransformationId, TransformationManifest>,
    lineage_log: Vec<LineageRecord>,
}

impl Default for TransformationEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl TransformationEngine {
    pub fn new() -> Self {
        Self {
            manifests: HashMap::new(),
            lineage_log: Vec::new(),
        }
    }

    pub fn register_manifest(&mut self, manifest: TransformationManifest) {
        self.manifests.insert(manifest.id, manifest);
    }

    pub fn get_manifest(&self, id: TransformationId) -> Option<&TransformationManifest> {
        self.manifests.get(&id)
    }

    pub fn lineage_records(&self) -> &[LineageRecord] {
        &self.lineage_log
    }

    /// Executes an Event-to-Observation feature extraction transformation.
    #[allow(clippy::too_many_arguments)]
    pub fn execute_event_to_observation(
        &mut self,
        tx_id: TransformationId,
        event_id: EventId,
        valid_time: Timestamp,
        known_time: Timestamp,
        feature_name: &str,
        payload_bytes: &[u8],
        next_knowledge_id: KnowledgeId,
    ) -> Result<(Observation, LineageRecord), String> {
        let manifest = self
            .manifests
            .get(&tx_id)
            .ok_or_else(|| format!("Transformation {tx_id} not registered"))?;

        if manifest.kind != TransformationKind::EventToObservation {
            return Err(format!(
                "Manifest {} is {:?}, expected EventToObservation",
                tx_id, manifest.kind
            ));
        }

        let mut meter = ResourceMeter::new(manifest.budget);
        // Meter basic step cost and input payload memory
        meter.consume_steps(10 + payload_bytes.len() as u64)?;
        meter.track_memory(payload_bytes.len() + 128)?;

        // Precondition: payload must not be empty
        if payload_bytes.is_empty() {
            let receipt = meter.receipt(false);
            let lineage = LineageRecord {
                transformation_id: tx_id,
                transformation_version: manifest.version.clone(),
                input_events: vec![event_id],
                input_knowledge: vec![],
                output_knowledge: vec![],
                executed_at: known_time,
                receipt,
            };
            self.lineage_log.push(lineage.clone());
            return Err("Precondition failed: empty event payload".to_string());
        }

        let value_repr = String::from_utf8_lossy(payload_bytes).to_string();
        let obs = Observation {
            id: next_knowledge_id,
            event_ref: event_id,
            valid_time,
            known_time,
            feature: feature_name.to_string(),
            value_repr,
        };

        let receipt = meter.receipt(true);
        let lineage = LineageRecord {
            transformation_id: tx_id,
            transformation_version: manifest.version.clone(),
            input_events: vec![event_id],
            input_knowledge: vec![],
            output_knowledge: vec![next_knowledge_id],
            executed_at: known_time,
            receipt,
        };

        self.lineage_log.push(lineage.clone());
        Ok((obs, lineage))
    }

    /// Executes an Episode-to-Pattern consolidation transformation into the tiered store.
    pub fn execute_episode_consolidation(
        &mut self,
        tx_id: TransformationId,
        store: &mut TieredKnowledgeStore,
        episode: Episode,
        signature: &str,
        executed_at: Timestamp,
    ) -> Result<(u64, LineageRecord), String> {
        let manifest = self
            .manifests
            .get(&tx_id)
            .ok_or_else(|| format!("Transformation {tx_id} not registered"))?;

        if manifest.kind != TransformationKind::EpisodeToPattern {
            return Err(format!(
                "Manifest {} is {:?}, expected EpisodeToPattern",
                tx_id, manifest.kind
            ));
        }

        let mut meter = ResourceMeter::new(manifest.budget);
        meter.consume_steps(50 + (episode.observations.len() as u64 * 5))?;
        meter.track_memory(signature.len() + 256)?;

        if episode.observations.is_empty() {
            return Err(
                "Precondition failed: cannot consolidate episode with zero observations"
                    .to_string(),
            );
        }

        let input_obs = episode.observations.clone();
        let pattern_id = store.consolidate_episode(episode, signature);

        let receipt = meter.receipt(true);
        let lineage = LineageRecord {
            transformation_id: tx_id,
            transformation_version: manifest.version.clone(),
            input_events: vec![],
            input_knowledge: input_obs,
            output_knowledge: vec![KnowledgeId(pattern_id)],
            executed_at,
            receipt,
        };

        self.lineage_log.push(lineage.clone());
        Ok((pattern_id, lineage))
    }
}

// ---------------------------------------------------------------------------
// Canonical IR E-Graphs & Rewrite Experiments (M30)
// ---------------------------------------------------------------------------

/// Unique equivalence class identifier in an e-graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EClassId(pub usize);

impl fmt::Display for EClassId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "e:{}", self.0)
    }
}

/// E-Node representation of expressions and query operators.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ENode {
    Literal(LiteralRepr),
    Field(String),
    Binary {
        op: BinaryOp,
        left: EClassId,
        right: EClassId,
    },
    Not(EClassId),
    Scan {
        entity: Option<EntityId>,
        filter: Option<EClassId>,
        limit: Option<usize>,
    },
    EmptyRelation,
}

/// Canonical hashable representation of a query literal.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LiteralRepr {
    Int(i64),
    Float(u64), // IEEE-754 bit-representation for exact deterministic hashing
    String(String),
    Bool(bool),
}

impl From<&Literal> for LiteralRepr {
    fn from(lit: &Literal) -> Self {
        match lit {
            Literal::Int(v) => Self::Int(*v),
            Literal::Float(v) => Self::Float(v.to_bits()),
            Literal::String(s) => Self::String(s.clone()),
            Literal::Bool(b) => Self::Bool(*b),
        }
    }
}

impl From<LiteralRepr> for Literal {
    fn from(repr: LiteralRepr) -> Self {
        match repr {
            LiteralRepr::Int(v) => Self::Int(v),
            LiteralRepr::Float(bits) => Self::Float(f64::from_bits(bits)),
            LiteralRepr::String(s) => Self::String(s),
            LiteralRepr::Bool(b) => Self::Bool(b),
        }
    }
}

/// E-Class containing structurally equivalent e-nodes.
#[derive(Debug, Clone)]
pub struct EClass {
    pub id: EClassId,
    pub nodes: HashSet<ENode>,
}

/// Equality saturation graph over canonical Query expressions and plans.
#[derive(Debug, Clone)]
pub struct EGraph {
    /// Union-find parent pointers for canonical class identity.
    parents: Vec<usize>,
    /// E-Classes indexed by class ID.
    classes: Vec<EClass>,
    /// Hashcons mapping canonical e-node to its assigned e-class.
    hashcons: HashMap<ENode, EClassId>,
}

impl Default for EGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl EGraph {
    pub fn new() -> Self {
        Self {
            parents: Vec::new(),
            classes: Vec::new(),
            hashcons: HashMap::new(),
        }
    }

    /// Canonical root lookup in the union-find structure with path compression.
    pub fn find(&mut self, id: EClassId) -> EClassId {
        let mut root = id.0;
        while root != self.parents[root] {
            root = self.parents[root];
        }
        // Compress path
        let mut curr = id.0;
        while curr != root {
            let next = self.parents[curr];
            self.parents[curr] = root;
            curr = next;
        }
        EClassId(root)
    }

    /// Read-only find without mutating path compression.
    pub fn find_const(&self, mut id: EClassId) -> EClassId {
        while id.0 != self.parents[id.0] {
            id = EClassId(self.parents[id.0]);
        }
        id
    }

    /// Merges two equivalence classes, returning the canonical root.
    pub fn union(&mut self, id1: EClassId, id2: EClassId) -> EClassId {
        let root1 = self.find(id1);
        let root2 = self.find(id2);
        if root1 == root2 {
            return root1;
        }

        // Canonical root: lower index wins
        let (parent, child) = if root1.0 < root2.0 {
            (root1, root2)
        } else {
            (root2, root1)
        };

        self.parents[child.0] = parent.0;

        // Merge nodes into parent class
        let child_nodes = std::mem::take(&mut self.classes[child.0].nodes);
        self.classes[parent.0].nodes.extend(child_nodes);

        parent
    }

    /// Inserts a canonicalized e-node into the e-graph.
    pub fn add(&mut self, mut node: ENode) -> EClassId {
        node = self.canonicalize_node(node);
        if let Some(&existing_id) = self.hashcons.get(&node) {
            return self.find(existing_id);
        }

        let new_id = EClassId(self.classes.len());
        self.parents.push(new_id.0);

        let mut nodes = HashSet::new();
        nodes.insert(node.clone());
        self.classes.push(EClass { id: new_id, nodes });

        self.hashcons.insert(node, new_id);
        new_id
    }

    /// Re-canonicalizes an e-node by resolving child IDs to their current roots.
    fn canonicalize_node(&self, mut node: ENode) -> ENode {
        match &mut node {
            ENode::Literal(_) | ENode::Field(_) | ENode::EmptyRelation => {}
            ENode::Binary { left, right, .. } => {
                *left = self.find_const(*left);
                *right = self.find_const(*right);
            }
            ENode::Not(child) => {
                *child = self.find_const(*child);
            }
            ENode::Scan { filter, .. } => {
                if let Some(f) = filter {
                    *f = self.find_const(*f);
                }
            }
        }
        node
    }

    /// Rebuilds invariant: canonicalizes all e-nodes and merges congruence closures.
    pub fn rebuild(&mut self) -> usize {
        let mut merges = 0;
        let mut new_hashcons = HashMap::new();

        for i in 0..self.classes.len() {
            let root = self.find(EClassId(i));
            if root.0 != i {
                continue;
            }

            let nodes: Vec<ENode> = self.classes[root.0].nodes.drain().collect();
            for node in nodes {
                let canonical = self.canonicalize_node(node);
                if let Some(&existing_id) = new_hashcons.get(&canonical) {
                    let canon_existing = self.find(existing_id);
                    if canon_existing != root {
                        self.union(canon_existing, root);
                        merges += 1;
                    }
                } else {
                    new_hashcons.insert(canonical.clone(), root);
                }
                self.classes[root.0].nodes.insert(canonical);
            }
        }

        self.hashcons = new_hashcons;
        merges
    }

    /// Adds a strongly typed [`Expr`] recursively into the e-graph.
    pub fn add_expr(&mut self, expr: &Expr) -> EClassId {
        match expr {
            Expr::Literal(lit) => self.add(ENode::Literal(LiteralRepr::from(lit))),
            Expr::Field(name) => self.add(ENode::Field(name.clone())),
            Expr::Not(inner) => {
                let child = self.add_expr(inner);
                self.add(ENode::Not(child))
            }
            Expr::Binary { op, left, right } => {
                let l = self.add_expr(left);
                let r = self.add_expr(right);
                self.add(ENode::Binary {
                    op: *op,
                    left: l,
                    right: r,
                })
            }
        }
    }

    /// Adds a simplified scan/filter query plan into the e-graph.
    pub fn add_plan(&mut self, plan: &LogicalPlan) -> Result<EClassId, QueryError> {
        match plan {
            LogicalPlan::Scan {
                entity,
                filter,
                limit,
                ..
            } => {
                let filter_class = filter.as_ref().map(|f| self.add_expr(f));
                Ok(self.add(ENode::Scan {
                    entity: *entity,
                    filter: filter_class,
                    limit: *limit,
                }))
            }
            _ => Err(QueryError::Unsupported(
                "E-graph currently plans Scan and Filter logical plans".to_string(),
            )),
        }
    }

    /// Runs equality saturation applying algebraic rewrite rules.
    pub fn saturate(&mut self, max_iterations: usize) -> SaturationReport {
        let mut total_rewrites = 0;
        let mut iterations_run = 0;

        let true_node = ENode::Literal(LiteralRepr::Bool(true));
        let false_node = ENode::Literal(LiteralRepr::Bool(false));
        let true_id = self.add(true_node);
        let false_id = self.add(false_node);

        for iter in 0..max_iterations {
            iterations_run = iter + 1;
            let mut matches_to_union: Vec<(EClassId, EClassId)> = Vec::new();

            let canon_true = self.find_const(true_id);
            let canon_false = self.find_const(false_id);

            // Snapshot existing classes and nodes to avoid borrowing conflicts
            let class_snapshots: Vec<(EClassId, Vec<ENode>)> = self
                .classes
                .iter()
                .enumerate()
                .filter(|(idx, _)| self.find_const(EClassId(*idx)) == EClassId(*idx))
                .map(|(idx, cls)| (EClassId(idx), cls.nodes.iter().cloned().collect()))
                .collect();

            for (root, nodes) in &class_snapshots {
                let canon_root = self.find_const(*root);
                for node in nodes {
                    match node {
                        ENode::Not(child) => {
                            let child_root = self.find_const(*child);
                            if let Some(child_cls) = self.classes.get(child_root.0) {
                                for child_node in &child_cls.nodes {
                                    if let ENode::Not(inner) = child_node {
                                        matches_to_union.push((canon_root, *inner));
                                    }
                                }
                            }
                            if child_root == canon_true {
                                matches_to_union.push((canon_root, canon_false));
                            } else if child_root == canon_false {
                                matches_to_union.push((canon_root, canon_true));
                            }
                        }
                        ENode::Binary { op, left, right } => {
                            let l_root = self.find_const(*left);
                            let r_root = self.find_const(*right);

                            match op {
                                BinaryOp::And => {
                                    if r_root == canon_true {
                                        matches_to_union.push((canon_root, l_root));
                                    } else if l_root == canon_true {
                                        matches_to_union.push((canon_root, r_root));
                                    }
                                    if r_root == canon_false || l_root == canon_false {
                                        matches_to_union.push((canon_root, canon_false));
                                    }
                                    if l_root == r_root {
                                        matches_to_union.push((canon_root, l_root));
                                    }
                                }
                                BinaryOp::Or => {
                                    if r_root == canon_false {
                                        matches_to_union.push((canon_root, l_root));
                                    } else if l_root == canon_false {
                                        matches_to_union.push((canon_root, r_root));
                                    }
                                    if r_root == canon_true || l_root == canon_true {
                                        matches_to_union.push((canon_root, canon_true));
                                    }
                                    if l_root == r_root {
                                        matches_to_union.push((canon_root, l_root));
                                    }
                                }
                                BinaryOp::Eq => {
                                    if l_root == r_root {
                                        matches_to_union.push((canon_root, canon_true));
                                    }
                                }
                                _ => {}
                            }

                            // Constant folding
                            let left_literals: Vec<LiteralRepr> = self.classes[l_root.0]
                                .nodes
                                .iter()
                                .filter_map(|n| match n {
                                    ENode::Literal(l) => Some(l.clone()),
                                    _ => None,
                                })
                                .collect();

                            let right_literals: Vec<LiteralRepr> = self.classes[r_root.0]
                                .nodes
                                .iter()
                                .filter_map(|n| match n {
                                    ENode::Literal(l) => Some(l.clone()),
                                    _ => None,
                                })
                                .collect();

                            for l_lit in &left_literals {
                                for r_lit in &right_literals {
                                    if let Some(folded) = fold_literals(*op, l_lit, r_lit) {
                                        let folded_id = self.add(ENode::Literal(folded));
                                        matches_to_union.push((canon_root, folded_id));
                                    }
                                }
                            }
                        }
                        ENode::Scan {
                            entity,
                            filter: Some(f),
                            limit,
                        } => {
                            let f_root = self.find_const(*f);
                            if f_root == canon_true {
                                let simplified_id = self.add(ENode::Scan {
                                    entity: *entity,
                                    filter: None,
                                    limit: *limit,
                                });
                                matches_to_union.push((canon_root, simplified_id));
                            } else if f_root == canon_false {
                                let empty_id = self.add(ENode::EmptyRelation);
                                matches_to_union.push((canon_root, empty_id));
                            }
                        }
                        _ => {}
                    }
                }
            }

            if matches_to_union.is_empty() {
                break;
            }

            let mut any_merged = false;
            for (a, b) in matches_to_union {
                let ra = self.find(a);
                let rb = self.find(b);
                if ra != rb {
                    self.union(ra, rb);
                    total_rewrites += 1;
                    any_merged = true;
                }
            }

            self.rebuild();
            if !any_merged {
                break;
            }
        }

        SaturationReport {
            iterations: iterations_run,
            total_rewrites,
            total_classes: self.classes.len(),
        }
    }

    /// Extracts the lowest-cost equivalent [`Expr`] from an equivalence class.
    pub fn extract_best_expr(&self, root: EClassId) -> Result<(Expr, usize), String> {
        let root = self.find_const(root);
        let num_classes = self.classes.len();
        let mut best_cost = vec![usize::MAX; num_classes];
        let mut best_node: Vec<Option<ENode>> = vec![None; num_classes];

        // Iterative Bellman-Ford style relaxation to handle cycles safely
        let mut changed = true;
        let mut iterations = 0;
        let max_iters = num_classes + 10;

        while changed && iterations < max_iters {
            changed = false;
            iterations += 1;

            for i in 0..num_classes {
                let canon = self.find_const(EClassId(i));
                for node in &self.classes[i].nodes {
                    let cost = match node {
                        ENode::Literal(_) => Some(1),
                        ENode::Field(_) => Some(2),
                        ENode::Not(child) => {
                            let c_canon = self.find_const(*child);
                            let child_cost = best_cost[c_canon.0];
                            if child_cost < usize::MAX {
                                Some(2 + child_cost)
                            } else {
                                None
                            }
                        }
                        ENode::Binary { left, right, .. } => {
                            let l_canon = self.find_const(*left);
                            let r_canon = self.find_const(*right);
                            let l_cost = best_cost[l_canon.0];
                            let r_cost = best_cost[r_canon.0];
                            if l_cost < usize::MAX && r_cost < usize::MAX {
                                Some(3 + l_cost + r_cost)
                            } else {
                                None
                            }
                        }
                        ENode::Scan { .. } | ENode::EmptyRelation => None,
                    };

                    if let Some(c) = cost {
                        if c < best_cost[canon.0] {
                            best_cost[canon.0] = c;
                            best_node[canon.0] = Some(node.clone());
                            changed = true;
                        }
                    }
                }
            }
        }

        if best_cost[root.0] == usize::MAX {
            return Err(format!(
                "Could not extract scalar expression from class {root}"
            ));
        }

        let expr = self.reconstruct_expr(root, &best_node)?;
        Ok((expr, best_cost[root.0]))
    }

    fn reconstruct_expr(
        &self,
        class: EClassId,
        best_node: &[Option<ENode>],
    ) -> Result<Expr, String> {
        let canon = self.find_const(class);
        let node = best_node[canon.0]
            .as_ref()
            .ok_or_else(|| format!("No optimal node recorded for class {canon}"))?;

        match node {
            ENode::Literal(lit) => Ok(Expr::Literal(Literal::from(lit.clone()))),
            ENode::Field(name) => Ok(Expr::Field(name.clone())),
            ENode::Not(child) => {
                let inner = self.reconstruct_expr(*child, best_node)?;
                Ok(Expr::Not(Box::new(inner)))
            }
            ENode::Binary { op, left, right } => {
                let l = self.reconstruct_expr(*left, best_node)?;
                let r = self.reconstruct_expr(*right, best_node)?;
                Ok(Expr::Binary {
                    op: *op,
                    left: Box::new(l),
                    right: Box::new(r),
                })
            }
            ENode::Scan { .. } | ENode::EmptyRelation => {
                Err("Cannot reconstruct scalar expression from relational plan node".to_string())
            }
        }
    }

    /// Evaluates if an expression matches an e-class.
    pub fn class_contains_expr(&self, class_id: EClassId, expr: &Expr) -> bool {
        let canon = self.find_const(class_id);
        match expr {
            Expr::Literal(lit) => {
                let repr = LiteralRepr::from(lit);
                self.classes[canon.0].nodes.contains(&ENode::Literal(repr))
            }
            Expr::Field(f) => self.classes[canon.0]
                .nodes
                .contains(&ENode::Field(f.clone())),
            _ => false,
        }
    }
}

fn fold_literals(op: BinaryOp, l: &LiteralRepr, r: &LiteralRepr) -> Option<LiteralRepr> {
    match (op, l, r) {
        (BinaryOp::Eq, LiteralRepr::Int(a), LiteralRepr::Int(b)) => Some(LiteralRepr::Bool(a == b)),
        (BinaryOp::NotEq, LiteralRepr::Int(a), LiteralRepr::Int(b)) => {
            Some(LiteralRepr::Bool(a != b))
        }
        (BinaryOp::Lt, LiteralRepr::Int(a), LiteralRepr::Int(b)) => Some(LiteralRepr::Bool(a < b)),
        (BinaryOp::Lte, LiteralRepr::Int(a), LiteralRepr::Int(b)) => {
            Some(LiteralRepr::Bool(a <= b))
        }
        (BinaryOp::Gt, LiteralRepr::Int(a), LiteralRepr::Int(b)) => Some(LiteralRepr::Bool(a > b)),
        (BinaryOp::Gte, LiteralRepr::Int(a), LiteralRepr::Int(b)) => {
            Some(LiteralRepr::Bool(a >= b))
        }
        (BinaryOp::And, LiteralRepr::Bool(a), LiteralRepr::Bool(b)) => {
            Some(LiteralRepr::Bool(*a && *b))
        }
        (BinaryOp::Or, LiteralRepr::Bool(a), LiteralRepr::Bool(b)) => {
            Some(LiteralRepr::Bool(*a || *b))
        }
        _ => None,
    }
}

/// Statistics collected during equality saturation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaturationReport {
    pub iterations: usize,
    pub total_rewrites: usize,
    pub total_classes: usize,
}
