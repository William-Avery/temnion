// SPDX-License-Identifier: AGPL-3.0-only
//! Virtual-shard execution, background task DAGs, and storage hierarchy for Temnion.
//!
//! Following Temnion Architecture §6, §10, and §11:
//! - **Virtual-Shard Execution (M8)**:
//!   - Single-writer ownership model eliminating global cross-shard database locks.
//!   - Shard routing mapping logical `ShardId` to virtual execution partitions.
//!   - Deterministic fanout and multi-shard sequence merge ordering.
//! - **Background Task DAG (M9)**:
//!   - Priority classes: `Seal`, `Compress`, `Index`, `Summary`, `Maintenance`.
//!   - Directed acyclic graph with explicit prerequisite dependencies and cycle prevention.
//!   - Foreground pressure tracking and adaptive task yielding/throttling.
//! - **Storage Hierarchy & Tiers (M10)**:
//!   - Tiers: `HotDram` (active mutable state), `WarmMapped` (cached segments/indexes),
//!     `ColdMedia` (persistent immutable storage).
//!   - Transparent migration, access counting, and LRU-bounded capacity eviction.

use std::collections::{BTreeMap, HashMap};
use std::error::Error;
use std::fmt;

use temnion_core::{ClockId, EntityId, EventId, EventTimes, SchemaId, ShardId};

#[derive(Debug, PartialEq, Eq)]
pub enum RuntimeError {
    InvalidConfiguration(&'static str),
    PartitionNotFound(u32),
    TaskNotFound(u64),
    DependencyCycleDetected,
    TaskPrerequisitesNotMet(u64),
    SegmentNotFound(u64),
    CapacityExceeded(&'static str),
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration(msg) => write!(f, "invalid runtime configuration: {msg}"),
            Self::PartitionNotFound(id) => write!(f, "virtual shard partition {id} not found"),
            Self::TaskNotFound(id) => write!(f, "background task {id} not found"),
            Self::DependencyCycleDetected => write!(f, "cycle detected in background task DAG"),
            Self::TaskPrerequisitesNotMet(id) => {
                write!(f, "prerequisites for task {id} are not completed")
            }
            Self::SegmentNotFound(id) => write!(f, "storage segment {id} not found in hierarchy"),
            Self::CapacityExceeded(msg) => write!(f, "storage capacity budget exceeded: {msg}"),
        }
    }
}

impl Error for RuntimeError {}

// ============================================================================
// Virtual-Shard Execution (M8)
// ============================================================================

/// Logical partition identifier for single-writer virtual shard execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VirtualShardId(pub u32);

/// Routing policy mapping logical ShardId to VirtualShardId.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShardRoutingPolicy {
    /// Modular hash-based assignment across `N` virtual shards.
    Modular(u32),
    /// Explicit mapping table.
    Explicit(HashMap<ShardId, VirtualShardId>),
}

/// Shard router distributing entities and write requests across virtual execution partitions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShardRouter {
    policy: ShardRoutingPolicy,
}

impl ShardRouter {
    pub fn new_modular(partition_count: u32) -> Result<Self, RuntimeError> {
        if partition_count == 0 {
            return Err(RuntimeError::InvalidConfiguration(
                "partition count must be at least 1",
            ));
        }
        Ok(Self {
            policy: ShardRoutingPolicy::Modular(partition_count),
        })
    }

    pub fn new_explicit(mapping: HashMap<ShardId, VirtualShardId>) -> Result<Self, RuntimeError> {
        if mapping.is_empty() {
            return Err(RuntimeError::InvalidConfiguration(
                "explicit mapping cannot be empty",
            ));
        }
        Ok(Self {
            policy: ShardRoutingPolicy::Explicit(mapping),
        })
    }

    pub fn route(&self, shard: ShardId) -> VirtualShardId {
        match &self.policy {
            ShardRoutingPolicy::Modular(count) => VirtualShardId(shard.0 % count),
            ShardRoutingPolicy::Explicit(map) => {
                map.get(&shard).copied().unwrap_or(VirtualShardId(0))
            }
        }
    }
}

/// Runtime event payload for virtual shard storage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeEvent {
    pub id: EventId,
    pub entity: EntityId,
    pub schema: SchemaId,
    pub times: EventTimes,
    pub payload: Vec<u8>,
}

/// Isolated, single-writer partition owning a subset of virtual shards.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VirtualShardPartition {
    pub id: VirtualShardId,
    pub events: Vec<RuntimeEvent>,
    next_sequence: u64,
}

impl VirtualShardPartition {
    pub fn new(id: VirtualShardId) -> Self {
        Self {
            id,
            events: Vec::new(),
            next_sequence: 0,
        }
    }

    pub fn append(&mut self, mut event: RuntimeEvent) -> u64 {
        let seq = self.next_sequence;
        event.id.sequence = seq;
        self.events.push(event);
        self.next_sequence += 1;
        seq
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

/// Query filter for virtual shard execution.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RuntimeFilter {
    pub entity: Option<EntityId>,
    pub schema: Option<SchemaId>,
    pub clock: Option<ClockId>,
    pub min_tick: Option<u64>,
    pub max_tick: Option<u64>,
}

impl RuntimeFilter {
    pub fn matches(&self, event: &RuntimeEvent) -> bool {
        if let Some(e) = self.entity {
            if event.entity != e {
                return false;
            }
        }
        if let Some(s) = self.schema {
            if event.schema != s {
                return false;
            }
        }
        if let Some(c) = self.clock {
            if event.times.valid.clock != c {
                return false;
            }
            if let Some(min_t) = self.min_tick {
                if event.times.valid.ticks < min_t {
                    return false;
                }
            }
            if let Some(max_t) = self.max_tick {
                if event.times.valid.ticks > max_t {
                    return false;
                }
            }
        }
        true
    }
}

/// Virtual shard coordinator orchestrating partition ownership and deterministic fanout queries.
pub struct VirtualShardCoordinator {
    router: ShardRouter,
    partitions: HashMap<VirtualShardId, VirtualShardPartition>,
}

impl VirtualShardCoordinator {
    pub fn new(router: ShardRouter, partition_ids: &[VirtualShardId]) -> Self {
        let mut partitions = HashMap::new();
        for &id in partition_ids {
            partitions.insert(id, VirtualShardPartition::new(id));
        }
        Self { router, partitions }
    }

    pub fn append(&mut self, event: RuntimeEvent) -> Result<(VirtualShardId, u64), RuntimeError> {
        let target = self.router.route(event.entity.shard);
        let partition = self
            .partitions
            .get_mut(&target)
            .ok_or(RuntimeError::PartitionNotFound(target.0))?;
        let seq = partition.append(event);
        Ok((target, seq))
    }

    pub fn partition(&self, id: VirtualShardId) -> Option<&VirtualShardPartition> {
        self.partitions.get(&id)
    }

    pub fn total_events(&self) -> usize {
        self.partitions.values().map(|p| p.len()).sum()
    }

    /// Fans out a query across all owned partitions and deterministically merges results by valid time.
    pub fn fanout_query(&self, filter: &RuntimeFilter) -> Vec<RuntimeEvent> {
        let mut results = Vec::new();
        for partition in self.partitions.values() {
            for event in &partition.events {
                if filter.matches(event) {
                    results.push(event.clone());
                }
            }
        }

        // Deterministic total ordering across partitions: (valid.clock, valid.ticks, source, epoch, sequence)
        results.sort_unstable_by(|a, b| {
            a.times
                .valid
                .clock
                .0
                .cmp(&b.times.valid.clock.0)
                .then_with(|| a.times.valid.ticks.cmp(&b.times.valid.ticks))
                .then_with(|| a.id.source.0.cmp(&b.id.source.0))
                .then_with(|| a.id.epoch.0.cmp(&b.id.epoch.0))
                .then_with(|| a.id.sequence.cmp(&b.id.sequence))
        });
        results
    }
}

// ============================================================================
// Background Task DAG (M9)
// ============================================================================

/// Unique identifier for a background task.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskId(pub u64);

/// Background task class determining execution priority.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TaskClass {
    /// Sealing full WAL batches into immutable segments (highest priority).
    Seal = 4,
    /// Applying scored lossless codecs.
    Compress = 3,
    /// Generating alternate projections.
    Index = 2,
    /// Generating multi-resolution hierarchical summaries.
    Summary = 1,
    /// Maintenance, checkpoints, and garbage collection (lowest priority).
    Maintenance = 0,
}

impl TaskClass {
    pub const fn priority_weight(&self) -> u32 {
        match self {
            Self::Seal => 100,
            Self::Compress => 80,
            Self::Index => 60,
            Self::Summary => 40,
            Self::Maintenance => 20,
        }
    }
}

/// Execution lifecycle state for a background task.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

/// A node in the background task dependency graph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskNode {
    pub id: TaskId,
    pub class: TaskClass,
    pub description: String,
    pub status: TaskStatus,
    pub dependencies: Vec<TaskId>,
}

/// Directed acyclic graph managing background storage tasks and their dependencies.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TaskDag {
    tasks: HashMap<TaskId, TaskNode>,
    next_id: u64,
}

impl TaskDag {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_task(
        &mut self,
        class: TaskClass,
        description: impl Into<String>,
        dependencies: Vec<TaskId>,
    ) -> Result<TaskId, RuntimeError> {
        // Validate dependencies exist
        for &dep in &dependencies {
            if !self.tasks.contains_key(&dep) {
                return Err(RuntimeError::TaskNotFound(dep.0));
            }
        }

        let id = TaskId(self.next_id);
        self.next_id += 1;

        let node = TaskNode {
            id,
            class,
            description: description.into(),
            status: TaskStatus::Pending,
            dependencies,
        };

        self.tasks.insert(id, node);

        // Verify DAG property (no cycles)
        if self.has_cycle() {
            self.tasks.remove(&id);
            return Err(RuntimeError::DependencyCycleDetected);
        }

        Ok(id)
    }

    pub fn task(&self, id: TaskId) -> Option<&TaskNode> {
        self.tasks.get(&id)
    }

    pub fn mark_running(&mut self, id: TaskId) -> Result<(), RuntimeError> {
        let node = self
            .tasks
            .get(&id)
            .ok_or(RuntimeError::TaskNotFound(id.0))?;

        // All dependencies must be Completed
        for &dep in &node.dependencies {
            let dep_node = &self.tasks[&dep];
            if dep_node.status != TaskStatus::Completed {
                return Err(RuntimeError::TaskPrerequisitesNotMet(id.0));
            }
        }

        let node_mut = self.tasks.get_mut(&id).unwrap();
        node_mut.status = TaskStatus::Running;
        Ok(())
    }

    pub fn mark_completed(&mut self, id: TaskId) -> Result<(), RuntimeError> {
        let node = self
            .tasks
            .get_mut(&id)
            .ok_or(RuntimeError::TaskNotFound(id.0))?;
        node.status = TaskStatus::Completed;
        Ok(())
    }

    pub fn mark_failed(&mut self, id: TaskId) -> Result<(), RuntimeError> {
        let node = self
            .tasks
            .get_mut(&id)
            .ok_or(RuntimeError::TaskNotFound(id.0))?;
        node.status = TaskStatus::Failed;
        Ok(())
    }

    /// Returns all tasks ready for execution (status Pending and all dependencies Completed),
    /// ordered by task priority descending.
    pub fn ready_tasks(&self) -> Vec<TaskId> {
        let mut ready = Vec::new();
        for node in self.tasks.values() {
            if node.status != TaskStatus::Pending {
                continue;
            }
            let all_deps_done = node
                .dependencies
                .iter()
                .all(|dep| self.tasks[dep].status == TaskStatus::Completed);
            if all_deps_done {
                ready.push(node);
            }
        }

        // Sort by TaskClass priority descending, then by TaskId ascending
        ready.sort_unstable_by(|a, b| b.class.cmp(&a.class).then_with(|| a.id.0.cmp(&b.id.0)));

        ready.into_iter().map(|n| n.id).collect()
    }

    fn has_cycle(&self) -> bool {
        let mut in_degree: HashMap<TaskId, usize> = HashMap::new();
        let mut adj: HashMap<TaskId, Vec<TaskId>> = HashMap::new();

        for &id in self.tasks.keys() {
            in_degree.insert(id, 0);
            adj.insert(id, Vec::new());
        }

        for node in self.tasks.values() {
            for &dep in &node.dependencies {
                adj.entry(dep).or_default().push(node.id);
                *in_degree.entry(node.id).or_default() += 1;
            }
        }

        let mut queue: Vec<TaskId> = in_degree
            .iter()
            .filter(|&(_, &deg)| deg == 0)
            .map(|(&id, _)| id)
            .collect();

        let mut visited = 0;
        while let Some(u) = queue.pop() {
            visited += 1;
            for &v in &adj[&u] {
                let deg = in_degree.get_mut(&v).unwrap();
                *deg -= 1;
                if *deg == 0 {
                    queue.push(v);
                }
            }
        }

        visited != self.tasks.len()
    }
}

/// Foreground pressure level monitoring write queue backlog and latency.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PressureLevel {
    Normal = 0,
    Moderate = 1,
    High = 2,
    Critical = 3,
}

/// Controller regulating background task execution to protect foreground latency.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PressureController {
    pub foreground_queue_limit: usize,
    pub current_queue_depth: usize,
}

impl PressureController {
    pub fn new(foreground_queue_limit: usize) -> Self {
        Self {
            foreground_queue_limit,
            current_queue_depth: 0,
        }
    }

    pub fn set_queue_depth(&mut self, depth: usize) {
        self.current_queue_depth = depth;
    }

    pub fn pressure_level(&self) -> PressureLevel {
        if self.foreground_queue_limit == 0 {
            return PressureLevel::Normal;
        }
        let ratio = (self.current_queue_depth * 100) / self.foreground_queue_limit;
        if ratio >= 90 {
            PressureLevel::Critical
        } else if ratio >= 70 {
            PressureLevel::High
        } else if ratio >= 40 {
            PressureLevel::Moderate
        } else {
            PressureLevel::Normal
        }
    }

    /// Determines whether background processing should throttle (stop new tasks).
    pub fn should_throttle(&self) -> bool {
        self.pressure_level() >= PressureLevel::High
    }

    /// Determines whether a task of a specific class should yield during execution.
    pub fn should_yield(&self, class: TaskClass) -> bool {
        match self.pressure_level() {
            PressureLevel::Critical => true,
            PressureLevel::High => class != TaskClass::Seal,
            PressureLevel::Moderate => class <= TaskClass::Index,
            PressureLevel::Normal => false,
        }
    }
}

// ============================================================================
// Storage Hierarchy & Tiers (M10)
// ============================================================================

/// Storage tier in the memory/storage hierarchy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StorageTier {
    /// Active volatile write buffers and live state slab.
    HotDram = 2,
    /// Memory-mapped or page-cache resident read-only segments.
    WarmMapped = 1,
    /// Persistent archival disk files.
    ColdMedia = 0,
}

/// Metadata tracking segment residency, access statistics, and tier placement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TieredSegment {
    pub segment_id: u64,
    pub byte_size: usize,
    pub tier: StorageTier,
    pub access_count: u64,
    pub last_access_step: u64,
}

/// Tiered storage manager balancing memory capacity and migrating segments between tiers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TieredStorageManager {
    pub hot_capacity_bytes: usize,
    pub warm_capacity_bytes: usize,
    current_step: u64,
    segments: BTreeMap<u64, TieredSegment>,
}

impl TieredStorageManager {
    pub fn new(hot_capacity_bytes: usize, warm_capacity_bytes: usize) -> Self {
        Self {
            hot_capacity_bytes,
            warm_capacity_bytes,
            current_step: 0,
            segments: BTreeMap::new(),
        }
    }

    pub fn register_segment(
        &mut self,
        segment_id: u64,
        byte_size: usize,
        initial_tier: StorageTier,
    ) {
        let segment = TieredSegment {
            segment_id,
            byte_size,
            tier: initial_tier,
            access_count: 0,
            last_access_step: self.current_step,
        };
        self.segments.insert(segment_id, segment);
    }

    pub fn segment(&self, id: u64) -> Option<&TieredSegment> {
        self.segments.get(&id)
    }

    pub fn tier_of(&self, id: u64) -> Option<StorageTier> {
        self.segments.get(&id).map(|s| s.tier)
    }

    /// Records an access to a segment, updating LRU stats and promoting tier when appropriate.
    pub fn record_access(&mut self, segment_id: u64) -> Result<StorageTier, RuntimeError> {
        self.current_step += 1;
        let step = self.current_step;

        let seg = self
            .segments
            .get_mut(&segment_id)
            .ok_or(RuntimeError::SegmentNotFound(segment_id))?;

        seg.access_count += 1;
        seg.last_access_step = step;

        // Auto-promotion: if accessed heavily from ColdMedia, promote to WarmMapped
        if seg.tier == StorageTier::ColdMedia && seg.access_count >= 2 {
            seg.tier = StorageTier::WarmMapped;
        }

        let tier = seg.tier;
        self.enforce_capacity();
        Ok(tier)
    }

    /// Explicitly migrates a segment to a target tier.
    pub fn migrate(
        &mut self,
        segment_id: u64,
        target_tier: StorageTier,
    ) -> Result<(), RuntimeError> {
        let seg = self
            .segments
            .get_mut(&segment_id)
            .ok_or(RuntimeError::SegmentNotFound(segment_id))?;
        seg.tier = target_tier;
        self.enforce_capacity();
        Ok(())
    }

    pub fn total_bytes_in_tier(&self, tier: StorageTier) -> usize {
        self.segments
            .values()
            .filter(|s| s.tier == tier)
            .map(|s| s.byte_size)
            .sum()
    }

    /// Enforces memory capacity budgets across Hot and Warm tiers using LRU eviction.
    pub fn enforce_capacity(&mut self) {
        // Enforce HotDram capacity -> demote to WarmMapped
        while self.total_bytes_in_tier(StorageTier::HotDram) > self.hot_capacity_bytes {
            let lru_id = self
                .segments
                .values()
                .filter(|s| s.tier == StorageTier::HotDram)
                .min_by_key(|s| s.last_access_step)
                .map(|s| s.segment_id);

            match lru_id {
                Some(id) => {
                    self.segments.get_mut(&id).unwrap().tier = StorageTier::WarmMapped;
                }
                None => break,
            }
        }

        // Enforce WarmMapped capacity -> demote to ColdMedia
        while self.total_bytes_in_tier(StorageTier::WarmMapped) > self.warm_capacity_bytes {
            let lru_id = self
                .segments
                .values()
                .filter(|s| s.tier == StorageTier::WarmMapped)
                .min_by_key(|s| s.last_access_step)
                .map(|s| s.segment_id);

            match lru_id {
                Some(id) => {
                    self.segments.get_mut(&id).unwrap().tier = StorageTier::ColdMedia;
                }
                None => break,
            }
        }
    }
}
