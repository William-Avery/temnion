// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]

//! Canonical typed query IR, planner, reference executor, TemQL and compact Tem parsers.
//!
//! # Architecture
//! All query interfaces (TemQL, compact `tn:`, protocol/TNP, Arrow/IPC, MCP, SQL, and Studio)
//! lower into a single canonical [`LogicalPlan`]. Parser frontends do not own independent
//! execution semantics.
//!
//! ## Key Components:
//! - **Logical IR ([`LogicalPlan`])**: Represents relational, temporal, spatial, and causal queries.
//! - **Expressions ([`Expr`])**: Strongly-typed scalar expressions, comparisons, and boolean logic.
//! - **Physical Planning ([`PhysicalPlan`], [`ExplainPlan`])**: Cost estimation and operator execution tree.
//! - **Execution ([`QueryExecutor`])**: Reference executor evaluating physical plans against storage and indexes.
//! - **TemQL ([`parse_temql`])**: Human-readable query language.
//! - **Compact Tem ([`parse_compact_tem`])**: AI token-efficient shorthand starting with `tn:`.

use std::collections::HashMap;
use std::fmt;
use std::ops::Range;

use temnion_causal::CausalGraph;
use temnion_core::{
    ClockId, EntityId, EventId, SchemaId, ShardId, SourceEpoch, SourceId, TimeAxis, TimeRange,
    Timestamp,
};
use temnion_events::HistoryFilter;
use temnion_format::StoredEvent;
use temnion_index::{BoundingBox2D, BoundingBox3D, GridChunker2D};
use temnion_storage::{StorageQueryBudget, Store};

/// Traversal direction for causal graph queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CausalDirection {
    Causes,
    Effects,
}

/// Errors encountered during query parsing, planning, or execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryError {
    ParseError(String),
    ValidationError(String),
    ExecutionError(String),
    BudgetExceeded(String),
    Unsupported(String),
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ParseError(msg) => write!(f, "Query parse error: {msg}"),
            Self::ValidationError(msg) => write!(f, "Query validation error: {msg}"),
            Self::ExecutionError(msg) => write!(f, "Query execution error: {msg}"),
            Self::BudgetExceeded(msg) => write!(f, "Query budget exceeded: {msg}"),
            Self::Unsupported(msg) => write!(f, "Unsupported query feature: {msg}"),
        }
    }
}

impl std::error::Error for QueryError {}

/// Typed scalar literals supported in query expressions.
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Int(i64),
    Float(f64),
    String(String),
    Bool(bool),
}

impl fmt::Display for Literal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(v) => write!(f, "{v}"),
            Self::Float(v) => write!(f, "{v}"),
            Self::String(v) => write!(f, "\"{v}\""),
            Self::Bool(v) => write!(f, "{v}"),
        }
    }
}

/// Binary operators for scalar expressions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinaryOp {
    Eq,
    NotEq,
    Lt,
    Lte,
    Gt,
    Gte,
    And,
    Or,
}

impl fmt::Display for BinaryOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Eq => write!(f, "=="),
            Self::NotEq => write!(f, "!="),
            Self::Lt => write!(f, "<"),
            Self::Lte => write!(f, "<="),
            Self::Gt => write!(f, ">"),
            Self::Gte => write!(f, ">="),
            Self::And => write!(f, "AND"),
            Self::Or => write!(f, "OR"),
        }
    }
}

/// Strongly-typed scalar expressions.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Literal(Literal),
    Field(String),
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Not(Box<Expr>),
}

impl Expr {
    /// Helper to evaluate expression against a map of field values.
    pub fn evaluate(&self, fields: &HashMap<String, Literal>) -> Result<Literal, QueryError> {
        match self {
            Self::Literal(lit) => Ok(lit.clone()),
            Self::Field(name) => fields
                .get(name)
                .cloned()
                .ok_or_else(|| QueryError::ExecutionError(format!("Field not found: {name}"))),
            Self::Not(inner) => {
                let val = inner.evaluate(fields)?;
                match val {
                    Literal::Bool(b) => Ok(Literal::Bool(!b)),
                    _ => Err(QueryError::ExecutionError(
                        "Unary NOT expects boolean operand".to_string(),
                    )),
                }
            }
            Self::Binary { op, left, right } => {
                let l = left.evaluate(fields)?;
                let r = right.evaluate(fields)?;
                match op {
                    BinaryOp::Eq => Ok(Literal::Bool(match (&l, &r) {
                        (Literal::Int(a), Literal::Int(b)) => a == b,
                        (Literal::Float(a), Literal::Float(b)) => a == b,
                        (Literal::String(a), Literal::String(b)) => a == b,
                        (Literal::Bool(a), Literal::Bool(b)) => a == b,
                        _ => false,
                    })),
                    BinaryOp::NotEq => Ok(Literal::Bool(match (&l, &r) {
                        (Literal::Int(a), Literal::Int(b)) => a != b,
                        (Literal::Float(a), Literal::Float(b)) => a != b,
                        (Literal::String(a), Literal::String(b)) => a != b,
                        (Literal::Bool(a), Literal::Bool(b)) => a != b,
                        _ => true,
                    })),
                    BinaryOp::Lt => match (&l, &r) {
                        (Literal::Int(a), Literal::Int(b)) => Ok(Literal::Bool(a < b)),
                        (Literal::Float(a), Literal::Float(b)) => Ok(Literal::Bool(a < b)),
                        _ => Err(QueryError::ExecutionError(
                            "Comparison expects numeric operands".to_string(),
                        )),
                    },
                    BinaryOp::Lte => match (&l, &r) {
                        (Literal::Int(a), Literal::Int(b)) => Ok(Literal::Bool(a <= b)),
                        (Literal::Float(a), Literal::Float(b)) => Ok(Literal::Bool(a <= b)),
                        _ => Err(QueryError::ExecutionError(
                            "Comparison expects numeric operands".to_string(),
                        )),
                    },
                    BinaryOp::Gt => match (&l, &r) {
                        (Literal::Int(a), Literal::Int(b)) => Ok(Literal::Bool(a > b)),
                        (Literal::Float(a), Literal::Float(b)) => Ok(Literal::Bool(a > b)),
                        _ => Err(QueryError::ExecutionError(
                            "Comparison expects numeric operands".to_string(),
                        )),
                    },
                    BinaryOp::Gte => match (&l, &r) {
                        (Literal::Int(a), Literal::Int(b)) => Ok(Literal::Bool(a >= b)),
                        (Literal::Float(a), Literal::Float(b)) => Ok(Literal::Bool(a >= b)),
                        _ => Err(QueryError::ExecutionError(
                            "Comparison expects numeric operands".to_string(),
                        )),
                    },
                    BinaryOp::And => match (&l, &r) {
                        (Literal::Bool(a), Literal::Bool(b)) => Ok(Literal::Bool(*a && *b)),
                        _ => Err(QueryError::ExecutionError(
                            "Logical AND expects boolean operands".to_string(),
                        )),
                    },
                    BinaryOp::Or => match (&l, &r) {
                        (Literal::Bool(a), Literal::Bool(b)) => Ok(Literal::Bool(*a || *b)),
                        _ => Err(QueryError::ExecutionError(
                            "Logical OR expects boolean operands".to_string(),
                        )),
                    },
                }
            }
        }
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Literal(lit) => write!(f, "{lit}"),
            Self::Field(name) => write!(f, "{name}"),
            Self::Not(inner) => write!(f, "NOT({inner})"),
            Self::Binary { op, left, right } => write!(f, "({left} {op} {right})"),
        }
    }
}

/// Resource budget ceilings for query execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueryBudget {
    pub max_rows: Option<usize>,
    pub max_events_scanned: Option<usize>,
    pub max_bytes: Option<usize>,
}

impl Default for QueryBudget {
    fn default() -> Self {
        Self {
            max_rows: Some(1000),
            max_events_scanned: Some(100_000),
            max_bytes: Some(64 * 1024 * 1024),
        }
    }
}

impl QueryBudget {
    pub fn unlimited() -> Self {
        Self {
            max_rows: None,
            max_events_scanned: None,
            max_bytes: None,
        }
    }
}

/// Canonical logical query plan (M16).
#[derive(Debug, Clone, PartialEq)]
pub enum LogicalPlan {
    /// Scan history with optional entity, time range, filter predicate, and projection.
    Scan {
        entity: Option<EntityId>,
        schema: Option<SchemaId>,
        valid_range: Option<Range<Timestamp>>,
        known_as_of: Option<Timestamp>,
        filter: Option<Expr>,
        projection: Option<Vec<String>>,
        limit: Option<usize>,
    },
    /// Causal graph traversal.
    CausalTrace {
        origin: EventId,
        direction: CausalDirection,
        max_depth: Option<usize>,
        limit: Option<usize>,
    },
    /// Multi-dimensional / spatial bounding box query.
    SpatialScan {
        bbox_2d: Option<BoundingBox2D>,
        bbox_3d: Option<BoundingBox3D>,
        valid_range: Option<Range<Timestamp>>,
        filter: Option<Expr>,
        limit: Option<usize>,
    },
}

/// Physical operator tree for query execution (M16).
#[derive(Debug, Clone, PartialEq)]
pub enum PhysicalPlan {
    StorageScan {
        entity: Option<EntityId>,
        valid_range: Option<Range<Timestamp>>,
        known_as_of: Option<Timestamp>,
        pushdown_filter: Option<Expr>,
        projection: Option<Vec<String>>,
        limit: Option<usize>,
        use_zone_maps: bool,
        use_bloom: bool,
    },
    CausalWalk {
        origin: EventId,
        direction: CausalDirection,
        max_depth: usize,
        limit: Option<usize>,
    },
    SpatialIndexLookup {
        morton_intervals: Vec<(u64, u64)>,
        limit: Option<usize>,
    },
}

/// Human-readable EXPLAIN plan tree.
#[derive(Debug, Clone)]
pub struct ExplainPlan {
    pub operator: String,
    pub details: Vec<String>,
    pub estimated_cost: usize,
    pub children: Vec<ExplainPlan>,
}

impl fmt::Display for ExplainPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.format_indent(f, 0)
    }
}

impl ExplainPlan {
    fn format_indent(&self, f: &mut fmt::Formatter<'_>, indent: usize) -> fmt::Result {
        let prefix = "  ".repeat(indent);
        writeln!(
            f,
            "{prefix}-> {} (est_cost={})",
            self.operator, self.estimated_cost
        )?;
        for detail in &self.details {
            writeln!(f, "{prefix}   {detail}")?;
        }
        for child in &self.children {
            child.format_indent(f, indent + 1)?;
        }
        Ok(())
    }
}

/// Optimizes and translates a `LogicalPlan` into a `PhysicalPlan`.
pub fn plan_query(logical: &LogicalPlan) -> PhysicalPlan {
    match logical {
        LogicalPlan::Scan {
            entity,
            valid_range,
            known_as_of,
            filter,
            projection,
            limit,
            ..
        } => PhysicalPlan::StorageScan {
            entity: *entity,
            valid_range: valid_range.clone(),
            known_as_of: *known_as_of,
            pushdown_filter: filter.clone(),
            projection: projection.clone(),
            limit: *limit,
            use_zone_maps: valid_range.is_some(),
            use_bloom: entity.is_some(),
        },
        LogicalPlan::CausalTrace {
            origin,
            direction,
            max_depth,
            limit,
        } => PhysicalPlan::CausalWalk {
            origin: *origin,
            direction: *direction,
            max_depth: max_depth.unwrap_or(16),
            limit: *limit,
        },
        LogicalPlan::SpatialScan {
            bbox_2d,
            bbox_3d: _,
            limit,
            ..
        } => {
            let intervals = if let Some(bbox) = bbox_2d {
                if let Ok(chunker) = GridChunker2D::new(16) {
                    bbox.morton_intervals_chunked(&chunker)
                } else {
                    vec![(0, u64::MAX)]
                }
            } else {
                vec![(0, u64::MAX)]
            };
            PhysicalPlan::SpatialIndexLookup {
                morton_intervals: intervals,
                limit: *limit,
            }
        }
    }
}

/// Produces an `ExplainPlan` for a given `LogicalPlan`.
pub fn explain_query(logical: &LogicalPlan) -> ExplainPlan {
    let physical = plan_query(logical);
    match physical {
        PhysicalPlan::StorageScan {
            entity,
            valid_range,
            known_as_of,
            pushdown_filter,
            projection,
            limit,
            use_zone_maps,
            use_bloom,
        } => {
            let mut details = Vec::new();
            if let Some(e) = entity {
                details.push(format!("entity={}:{}:{}", e.shard.0, e.slot, e.generation));
            }
            if let Some(r) = valid_range {
                details.push(format!("valid_range={}..{}", r.start.ticks, r.end.ticks));
            }
            if let Some(k) = known_as_of {
                details.push(format!("known_as_of={}", k.ticks));
            }
            if let Some(f) = pushdown_filter {
                details.push(format!("pushdown_filter={:?}", f));
            }
            if let Some(p) = projection {
                details.push(format!("projection=[{}]", p.join(", ")));
            }
            if let Some(l) = limit {
                details.push(format!("limit={l}"));
            }
            details.push(format!(
                "pushdown_zone_maps={use_zone_maps}, pushdown_bloom={use_bloom}"
            ));

            ExplainPlan {
                operator: "StorageScan".to_string(),
                details,
                estimated_cost: 10,
                children: vec![],
            }
        }
        PhysicalPlan::CausalWalk {
            origin,
            direction,
            max_depth,
            limit,
        } => {
            let details = vec![
                format!(
                    "origin={}:{}:{}",
                    origin.source.0, origin.epoch.0, origin.sequence
                ),
                format!("direction={direction:?}"),
                format!("max_depth={max_depth}"),
                format!("limit={limit:?}"),
            ];
            ExplainPlan {
                operator: "CausalWalk".to_string(),
                details,
                estimated_cost: 5,
                children: vec![],
            }
        }
        PhysicalPlan::SpatialIndexLookup {
            morton_intervals,
            limit,
        } => {
            let details = vec![
                format!("morton_intervals_count={}", morton_intervals.len()),
                format!("limit={limit:?}"),
            ];
            ExplainPlan {
                operator: "SpatialIndexLookup".to_string(),
                details,
                estimated_cost: 8,
                children: vec![],
            }
        }
    }
}

/// A row returned from a query execution.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryRow {
    pub entity: EntityId,
    pub schema: SchemaId,
    pub valid_time: Timestamp,
    pub known_time: Timestamp,
    pub sequence: u64,
    pub fields: HashMap<String, Literal>,
}

/// Metrics and results returned from query execution.
#[derive(Debug, Clone)]
pub struct QueryResult {
    pub rows: Vec<QueryRow>,
    pub events_scanned: usize,
    pub bytes_read: usize,
    pub truncated: bool,
}

/// Reference Query Executor (M16).
pub struct QueryExecutor;

impl QueryExecutor {
    /// Executes a physical storage scan against a durable store.
    pub fn execute_storage_scan(
        store: &mut Store,
        plan: &PhysicalPlan,
        budget: &QueryBudget,
    ) -> Result<QueryResult, QueryError> {
        match plan {
            PhysicalPlan::StorageScan {
                entity,
                valid_range,
                known_as_of,
                pushdown_filter,
                projection,
                limit,
                ..
            } => {
                let time = if let Some(r) = valid_range {
                    let tr = TimeRange::new(r.start.clock, r.start.ticks, r.end.ticks)
                        .map_err(|e| QueryError::ExecutionError(e.to_string()))?;
                    Some((TimeAxis::Valid, tr))
                } else {
                    None
                };

                let filter = HistoryFilter {
                    entity: *entity,
                    time,
                    known_as_of: *known_as_of,
                };

                let max_limit = limit.unwrap_or(usize::MAX);
                let budget_limit = budget.max_rows.unwrap_or(usize::MAX);
                let target_limit = max_limit.min(budget_limit).max(1);

                let storage_budget = StorageQueryBudget {
                    max_results: target_limit,
                    max_scanned: budget.max_events_scanned.unwrap_or(65_536).max(1),
                    max_read_bytes: budget.max_bytes.unwrap_or(64 * 1024 * 1024).max(1024),
                };

                let page = store
                    .history(filter, storage_budget, None)
                    .map_err(|e| QueryError::ExecutionError(e.to_string()))?;

                let mut rows = Vec::new();
                let scanned = page.scanned;
                let bytes_read = page.bytes_read;
                let truncated = page.continuation.is_some();

                for event in page.events {
                    let mut fields = HashMap::new();
                    fields.insert(
                        "entity".to_string(),
                        Literal::String(format!(
                            "{}:{}:{}",
                            event.entity.shard.0, event.entity.slot, event.entity.generation
                        )),
                    );
                    fields.insert("schema".to_string(), Literal::Int(event.schema.0 as i64));
                    fields.insert(
                        "valid_time".to_string(),
                        Literal::Int(event.times.valid.ticks as i64),
                    );
                    fields.insert(
                        "known_time".to_string(),
                        Literal::Int(event.times.known.ticks as i64),
                    );
                    fields.insert(
                        "sequence".to_string(),
                        Literal::Int(event.id.sequence as i64),
                    );

                    // If filter predicate is provided, evaluate
                    if let Some(pred) = pushdown_filter {
                        match pred.evaluate(&fields) {
                            Ok(Literal::Bool(true)) => {}
                            Ok(Literal::Bool(false)) => continue,
                            Err(err) => return Err(err),
                            _ => continue,
                        }
                    }

                    // Project fields if requested
                    if let Some(proj) = projection {
                        fields.retain(|k, _| proj.contains(k));
                    }

                    rows.push(QueryRow {
                        entity: event.entity,
                        schema: event.schema,
                        valid_time: event.times.valid,
                        known_time: event.times.known,
                        sequence: event.id.sequence,
                        fields,
                    });

                    if rows.len() >= target_limit {
                        break;
                    }
                }

                Ok(QueryResult {
                    rows,
                    events_scanned: scanned,
                    bytes_read,
                    truncated,
                })
            }
            _ => Err(QueryError::Unsupported(
                "execute_storage_scan requires PhysicalPlan::StorageScan".to_string(),
            )),
        }
    }

    /// Evaluates whether a [`StoredEvent`] satisfies an optional filter expression,
    /// and extracts a [`QueryRow`] if matching.
    pub fn match_and_project_event(
        filter: Option<&Expr>,
        projection: Option<&[String]>,
        event: &StoredEvent,
    ) -> Result<Option<QueryRow>, QueryError> {
        let mut fields = HashMap::new();
        fields.insert(
            "entity".to_string(),
            Literal::String(format!(
                "{}:{}:{}",
                event.entity.shard.0, event.entity.slot, event.entity.generation
            )),
        );
        fields.insert("schema".to_string(), Literal::Int(event.schema.0 as i64));
        fields.insert(
            "valid_time".to_string(),
            Literal::Int(event.times.valid.ticks as i64),
        );
        fields.insert(
            "known_time".to_string(),
            Literal::Int(event.times.known.ticks as i64),
        );
        fields.insert(
            "sequence".to_string(),
            Literal::Int(event.id.sequence as i64),
        );

        if let Some(pred) = filter {
            match pred.evaluate(&fields) {
                Ok(Literal::Bool(true)) => {}
                Ok(Literal::Bool(false)) => return Ok(None),
                Err(err) => return Err(err),
                _ => return Ok(None),
            }
        }

        if let Some(proj) = projection {
            fields.retain(|k, _| proj.contains(k));
        }

        Ok(Some(QueryRow {
            entity: event.entity,
            schema: event.schema,
            valid_time: event.times.valid,
            known_time: event.times.known,
            sequence: event.id.sequence,
            fields,
        }))
    }

    /// Executes a causal trace query against a causal graph.
    pub fn execute_causal_walk(
        graph: &CausalGraph,
        plan: &PhysicalPlan,
        budget: &QueryBudget,
    ) -> Result<Vec<EventId>, QueryError> {
        match plan {
            PhysicalPlan::CausalWalk {
                origin,
                direction,
                max_depth,
                limit,
            } => {
                let max_limit = limit.unwrap_or(usize::MAX);
                let budget_limit = budget.max_rows.unwrap_or(usize::MAX);
                let target_limit = max_limit.min(budget_limit);

                let trace = match direction {
                    CausalDirection::Causes => graph.trace_causes(*origin, *max_depth),
                    CausalDirection::Effects => graph.trace_effects(*origin, *max_depth),
                };

                let mut res = trace.events;
                if res.len() > target_limit {
                    res.truncate(target_limit);
                }
                Ok(res)
            }
            _ => Err(QueryError::Unsupported(
                "execute_causal_walk requires PhysicalPlan::CausalWalk".to_string(),
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Parsers: TemQL and Compact Tem (M17)
// ---------------------------------------------------------------------------

/// Parses a human-readable TemQL query string into a canonical [`LogicalPlan`].
///
/// # Supported grammar:
/// ```text
/// FROM temnion
/// [ENTITY <shard>:<slot>:<generation>]
/// [TIME valid <start>..<end>]
/// [KNOWN_AS_OF <tick>]
/// [WHERE <field> <op> <value>]
/// [SELECT <field1>, <field2>]
/// [LIMIT <n>]
/// ```
/// Or Causal queries:
/// ```text
/// EVENT <source>:<epoch>:<seq> TRACE CAUSES [DEPTH <n>] [LIMIT <m>]
/// EVENT <source>:<epoch>:<seq> TRACE EFFECTS [DEPTH <n>] [LIMIT <m>]
/// ```
pub fn parse_temql(input: &str) -> Result<LogicalPlan, QueryError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(QueryError::ParseError("Empty query input".to_string()));
    }

    // Check for causal query
    if trimmed.starts_with("EVENT") {
        return parse_temql_causal(trimmed);
    }

    // Standard scan query
    let mut entity = None;
    let mut valid_range = None;
    let mut known_as_of = None;
    let mut filter = None;
    let mut projection = None;
    let mut limit = None;

    let lines = trimmed
        .split('\n')
        .map(|l| l.trim())
        .filter(|l| !l.is_empty());

    for line in lines {
        if line.starts_with("FROM") {
            // e.g. FROM temnion
            continue;
        } else if let Some(rest) = line.strip_prefix("ENTITY ") {
            entity = Some(parse_entity_id(rest.trim())?);
        } else if let Some(rest) = line.strip_prefix("TIME valid ") {
            valid_range = Some(parse_range(rest.trim())?);
        } else if let Some(rest) = line.strip_prefix("KNOWN_AS_OF ") {
            let tick: u64 = rest
                .trim()
                .parse()
                .map_err(|e| QueryError::ParseError(format!("Invalid KNOWN_AS_OF: {e}")))?;
            known_as_of = Some(Timestamp {
                clock: ClockId(1),
                ticks: tick,
            });
        } else if let Some(rest) = line.strip_prefix("WHERE ") {
            filter = Some(parse_simple_expr(rest.trim())?);
        } else if let Some(rest) = line.strip_prefix("SELECT ") {
            let fields: Vec<String> = rest
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if !fields.is_empty() {
                projection = Some(fields);
            }
        } else if let Some(rest) = line.strip_prefix("LIMIT ") {
            let lim: usize = rest
                .trim()
                .parse()
                .map_err(|e| QueryError::ParseError(format!("Invalid LIMIT: {e}")))?;
            limit = Some(lim);
        } else {
            return Err(QueryError::ParseError(format!(
                "Unrecognized TemQL clause: '{line}'"
            )));
        }
    }

    Ok(LogicalPlan::Scan {
        entity,
        schema: None,
        valid_range,
        known_as_of,
        filter,
        projection,
        limit,
    })
}

/// Parses causal TemQL queries: `EVENT 1:1:100 TRACE CAUSES [DEPTH 8] [LIMIT 16]`
fn parse_temql_causal(input: &str) -> Result<LogicalPlan, QueryError> {
    let tokens: Vec<&str> = input.split_whitespace().collect();
    if tokens.len() < 4 || tokens[0] != "EVENT" || tokens[2] != "TRACE" {
        return Err(QueryError::ParseError(format!(
            "Invalid causal query syntax: {input}"
        )));
    }

    let origin = parse_event_id(tokens[1])?;
    let direction = match tokens[3] {
        "CAUSES" => CausalDirection::Causes,
        "EFFECTS" => CausalDirection::Effects,
        other => {
            return Err(QueryError::ParseError(format!(
                "Unknown causal direction: {other}"
            )));
        }
    };

    let mut max_depth = None;
    let mut limit = None;

    let mut i = 4;
    while i < tokens.len() {
        if tokens[i] == "DEPTH" && i + 1 < tokens.len() {
            max_depth = Some(
                tokens[i + 1]
                    .parse()
                    .map_err(|e| QueryError::ParseError(format!("Invalid DEPTH: {e}")))?,
            );
            i += 2;
        } else if tokens[i] == "LIMIT" && i + 1 < tokens.len() {
            limit = Some(
                tokens[i + 1]
                    .parse()
                    .map_err(|e| QueryError::ParseError(format!("Invalid LIMIT: {e}")))?,
            );
            i += 2;
        } else {
            return Err(QueryError::ParseError(format!(
                "Unexpected token in causal query: {}",
                tokens[i]
            )));
        }
    }

    Ok(LogicalPlan::CausalTrace {
        origin,
        direction,
        max_depth,
        limit,
    })
}

/// Parses compact AI shorthand (prefixed with `tn:` or raw shorthand) into a [`LogicalPlan`].
///
/// # Supported syntax:
/// - Entity scan: `tn:#<entity>@v<start>..<end>@k<tick>?<filter>><proj>!<limit>`
///   e.g. `tn:#0:1:0@v10..20@k20?health>50>position,health!32`
/// - Causal trace: `tn:$<event><-<depth>` or `tn:$<event>-><depth>`
///   e.g. `tn:$1:1:100<-8`
pub fn parse_compact_tem(input: &str) -> Result<LogicalPlan, QueryError> {
    let raw = input.trim();
    let s = raw.strip_prefix("tn:").unwrap_or(raw).trim();

    if s.is_empty() {
        return Err(QueryError::ParseError("Empty compact query".to_string()));
    }

    // Check for causal compact query: e.g. $1:1:100<-8
    if let Some(rest) = s.strip_prefix('$') {
        if let Some((event_str, depth_str)) = rest.split_once("<-") {
            let origin = parse_event_id(event_str.trim())?;
            let depth: usize = depth_str
                .trim()
                .parse()
                .map_err(|e| QueryError::ParseError(format!("Invalid compact depth: {e}")))?;
            return Ok(LogicalPlan::CausalTrace {
                origin,
                direction: CausalDirection::Causes,
                max_depth: Some(depth),
                limit: None,
            });
        } else if let Some((event_str, depth_str)) = rest.split_once("->") {
            let origin = parse_event_id(event_str.trim())?;
            let depth: usize = depth_str
                .trim()
                .parse()
                .map_err(|e| QueryError::ParseError(format!("Invalid compact depth: {e}")))?;
            return Ok(LogicalPlan::CausalTrace {
                origin,
                direction: CausalDirection::Effects,
                max_depth: Some(depth),
                limit: None,
            });
        }
    }

    // Standard compact scan
    let mut entity = None;
    let mut valid_range = None;
    let mut known_as_of = None;
    let mut filter = None;
    let mut projection = None;
    let mut limit = None;

    let mut cur = s;

    // Parse entity `#`
    if let Some(rest) = cur.strip_prefix('#') {
        let end = rest.find(['@', '?', '>', '!']).unwrap_or(rest.len());
        entity = Some(parse_entity_id(&rest[..end])?);
        cur = &rest[end..];
    }

    // Parse valid time `@v`
    if let Some(rest) = cur.strip_prefix("@v") {
        let end = rest.find(['@', '?', '>', '!']).unwrap_or(rest.len());
        valid_range = Some(parse_range(&rest[..end])?);
        cur = &rest[end..];
    }

    // Parse known time `@k`
    if let Some(rest) = cur.strip_prefix("@k") {
        let end = rest.find(['?', '>', '!']).unwrap_or(rest.len());
        let tick: u64 = rest[..end]
            .parse()
            .map_err(|e| QueryError::ParseError(format!("Invalid compact known time: {e}")))?;
        known_as_of = Some(Timestamp {
            clock: ClockId(1),
            ticks: tick,
        });
        cur = &rest[end..];
    }

    // Parse filter `?` and projection `>`
    if let Some(rest) = cur.strip_prefix('?') {
        let (expr_and_proj, lim_part) = match rest.split_once('!') {
            Some((ep, l)) => (ep, Some(l)),
            None => (rest, None),
        };
        if let Some(l) = lim_part {
            let lim: usize = l
                .trim()
                .parse()
                .map_err(|e| QueryError::ParseError(format!("Invalid compact limit: {e}")))?;
            limit = Some(lim);
        }

        let mut found_split = None;
        for (idx, b) in expr_and_proj.bytes().enumerate() {
            if b == b'>' {
                let after = &expr_and_proj[idx + 1..];
                if let Some(first_char) = after.chars().next() {
                    if first_char.is_alphabetic() || first_char == '_' {
                        found_split = Some((&expr_and_proj[..idx], after));
                    }
                }
            }
        }

        if let Some((e_part, p_part)) = found_split {
            filter = Some(parse_simple_expr(e_part)?);
            let fields: Vec<String> = p_part
                .split(',')
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect();
            if !fields.is_empty() {
                projection = Some(fields);
            }
        } else {
            filter = Some(parse_simple_expr(expr_and_proj)?);
        }
    } else if let Some(rest) = cur.strip_prefix('>') {
        let (proj_part, lim_part) = match rest.split_once('!') {
            Some((p, l)) => (p, Some(l)),
            None => (rest, None),
        };
        if let Some(l) = lim_part {
            let lim: usize = l
                .trim()
                .parse()
                .map_err(|e| QueryError::ParseError(format!("Invalid compact limit: {e}")))?;
            limit = Some(lim);
        }
        let fields: Vec<String> = proj_part
            .split(',')
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect();
        if !fields.is_empty() {
            projection = Some(fields);
        }
    } else if let Some(rest) = cur.strip_prefix('!') {
        let lim: usize = rest
            .trim()
            .parse()
            .map_err(|e| QueryError::ParseError(format!("Invalid compact limit: {e}")))?;
        limit = Some(lim);
    }

    Ok(LogicalPlan::Scan {
        entity,
        schema: None,
        valid_range,
        known_as_of,
        filter,
        projection,
        limit,
    })
}

/// Parses SQL query text into canonical `LogicalPlan` (M22).
///
/// Supports relational and temporal SQL queries:
/// ```sql
/// SELECT position, health
/// FROM temnion
/// WHERE entity = '#0:1:0'
///   AND valid_time >= 10
///   AND valid_time < 20
///   AND known_as_of = 20
///   AND health > 50
/// LIMIT 32
/// ```
pub fn parse_sql(input: &str) -> Result<LogicalPlan, QueryError> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err(QueryError::ParseError("Empty SQL query input".to_string()));
    }

    let lower = raw.to_lowercase();
    if !lower.starts_with("select") {
        return Err(QueryError::ParseError(
            "SQL query must start with 'SELECT'".to_string(),
        ));
    }

    let from_idx = lower
        .find(" from ")
        .or_else(|| lower.find("\nfrom "))
        .ok_or_else(|| QueryError::ParseError("SQL query missing 'FROM' clause".to_string()))?;

    let select_part = raw[6..from_idx].trim();
    let after_from = raw[from_idx + 6..].trim();

    let lower_after_from = after_from.to_lowercase();
    let where_pos = lower_after_from
        .find(" where ")
        .or_else(|| lower_after_from.find("\nwhere "));
    let limit_pos = lower_after_from
        .find(" limit ")
        .or_else(|| lower_after_from.find("\nlimit "));

    let (from_table, where_clause, limit_clause) = match (where_pos, limit_pos) {
        (Some(w_idx), Some(l_idx)) if w_idx < l_idx => {
            let table = after_from[..w_idx].trim();
            let wh = after_from[w_idx + 7..l_idx].trim();
            let lim = after_from[l_idx + 7..].trim();
            (table, Some(wh), Some(lim))
        }
        (Some(w_idx), None) => {
            let table = after_from[..w_idx].trim();
            let wh = after_from[w_idx + 7..].trim();
            (table, Some(wh), None)
        }
        (None, Some(l_idx)) => {
            let table = after_from[..l_idx].trim();
            let lim = after_from[l_idx + 7..].trim();
            (table, None, Some(lim))
        }
        (None, None) => (after_from.trim(), None, None),
        _ => {
            return Err(QueryError::ParseError(
                "Invalid SQL clause ordering; expected FROM ... WHERE ... LIMIT".to_string(),
            ));
        }
    };

    // 1. Projection
    let projection = if select_part == "*" {
        None
    } else {
        let fields: Vec<String> = select_part
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if fields.is_empty() {
            None
        } else {
            Some(fields)
        }
    };

    // 2. Entity from table or WHERE
    let mut entity = parse_entity_id(from_table).ok();

    // 3. Predicates from WHERE
    let mut valid_start: Option<u64> = None;
    let mut valid_end: Option<u64> = None;
    let mut known_as_of = None;
    let mut filter_exprs: Vec<Expr> = Vec::new();

    if let Some(wh) = where_clause {
        let predicates = split_sql_and(wh);
        for pred in predicates {
            let p_trim = pred.trim();
            let p_lower = p_trim.to_lowercase();

            if p_lower.starts_with("entity =") || p_lower.starts_with("entity=") {
                let val_str = p_trim
                    .split_once('=')
                    .unwrap()
                    .1
                    .trim()
                    .trim_matches('\'')
                    .trim_matches('"');
                entity = Some(parse_entity_id(val_str)?);
            } else if p_lower.starts_with("valid_time >=") || p_lower.starts_with("valid_time>=") {
                let val_str = p_trim.split_once(">=").unwrap().1.trim();
                let tick: u64 = val_str.parse().map_err(|e| {
                    QueryError::ParseError(format!("Invalid valid_time start: {e}"))
                })?;
                valid_start = Some(tick);
            } else if p_lower.starts_with("valid_time >") || p_lower.starts_with("valid_time>") {
                let val_str = p_trim.split_once('>').unwrap().1.trim();
                let tick: u64 = val_str.parse().map_err(|e| {
                    QueryError::ParseError(format!("Invalid valid_time start: {e}"))
                })?;
                valid_start = Some(tick);
            } else if p_lower.starts_with("valid_time <=") || p_lower.starts_with("valid_time<=") {
                let val_str = p_trim.split_once("<=").unwrap().1.trim();
                let tick: u64 = val_str
                    .parse()
                    .map_err(|e| QueryError::ParseError(format!("Invalid valid_time end: {e}")))?;
                valid_end = Some(tick);
            } else if p_lower.starts_with("valid_time <") || p_lower.starts_with("valid_time<") {
                let val_str = p_trim.split_once('<').unwrap().1.trim();
                let tick: u64 = val_str
                    .parse()
                    .map_err(|e| QueryError::ParseError(format!("Invalid valid_time end: {e}")))?;
                valid_end = Some(tick);
            } else if p_lower.starts_with("known_as_of =") || p_lower.starts_with("known_as_of=") {
                let val_str = p_trim.split_once('=').unwrap().1.trim();
                let tick: u64 = val_str
                    .parse()
                    .map_err(|e| QueryError::ParseError(format!("Invalid known_as_of: {e}")))?;
                known_as_of = Some(Timestamp {
                    clock: ClockId(1),
                    ticks: tick,
                });
            } else if p_lower.starts_with("known_time <=") || p_lower.starts_with("known_time<=") {
                let val_str = p_trim.split_once("<=").unwrap().1.trim();
                let tick: u64 = val_str.parse().map_err(|e| {
                    QueryError::ParseError(format!("Invalid known_time cutoff: {e}"))
                })?;
                known_as_of = Some(Timestamp {
                    clock: ClockId(1),
                    ticks: tick,
                });
            } else {
                let expr = parse_sql_expr(p_trim)?;
                filter_exprs.push(expr);
            }
        }
    }

    let valid_range = match (valid_start, valid_end) {
        (Some(start), Some(end)) => Some(
            Timestamp {
                clock: ClockId(1),
                ticks: start,
            }..Timestamp {
                clock: ClockId(1),
                ticks: end,
            },
        ),
        _ => None,
    };

    let mut filter = None;
    for expr in filter_exprs {
        match filter {
            None => filter = Some(expr),
            Some(existing) => {
                filter = Some(Expr::Binary {
                    op: BinaryOp::And,
                    left: Box::new(existing),
                    right: Box::new(expr),
                });
            }
        }
    }

    let limit = if let Some(lim_str) = limit_clause {
        let lim: usize = lim_str
            .trim()
            .parse()
            .map_err(|e| QueryError::ParseError(format!("Invalid SQL LIMIT: {e}")))?;
        Some(lim)
    } else {
        None
    };

    Ok(LogicalPlan::Scan {
        entity,
        schema: None,
        valid_range,
        known_as_of,
        filter,
        projection,
        limit,
    })
}

fn split_sql_and(s: &str) -> Vec<String> {
    let mut results = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;

    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' || c == '"' {
            in_quote = !in_quote;
            current.push(c);
            i += 1;
        } else if !in_quote
            && (i + 4 <= chars.len())
            && chars[i..i + 4]
                .iter()
                .collect::<String>()
                .eq_ignore_ascii_case("and ")
            && (i == 0 || chars[i - 1].is_whitespace())
        {
            results.push(current.trim().to_string());
            current.clear();
            i += 4;
        } else {
            current.push(c);
            i += 1;
        }
    }
    if !current.trim().is_empty() {
        results.push(current.trim().to_string());
    }
    results
}

fn parse_sql_expr(s: &str) -> Result<Expr, QueryError> {
    parse_expr(s)
}

fn parse_entity_id(s: &str) -> Result<EntityId, QueryError> {
    let clean = s.strip_prefix('#').unwrap_or(s).trim();
    let parts: Vec<&str> = clean.split(':').collect();
    if parts.len() == 3 {
        let shard: u32 = parts[0]
            .parse()
            .map_err(|e| QueryError::ParseError(format!("Invalid shard: {e}")))?;
        let slot: u32 = parts[1]
            .parse()
            .map_err(|e| QueryError::ParseError(format!("Invalid slot: {e}")))?;
        let generation: u32 = parts[2]
            .parse()
            .map_err(|e| QueryError::ParseError(format!("Invalid generation: {e}")))?;
        Ok(EntityId {
            shard: ShardId(shard),
            slot,
            generation,
        })
    } else if parts.len() == 1 {
        let slot: u32 = parts[0]
            .parse()
            .map_err(|e| QueryError::ParseError(format!("Invalid slot: {e}")))?;
        Ok(EntityId {
            shard: ShardId(0),
            slot,
            generation: 1,
        })
    } else {
        Err(QueryError::ParseError(format!(
            "Invalid entity format: {s}"
        )))
    }
}

fn parse_event_id(s: &str) -> Result<EventId, QueryError> {
    let clean = s.strip_prefix('$').unwrap_or(s).trim();
    let parts: Vec<&str> = clean.split(':').collect();
    if parts.len() != 3 {
        return Err(QueryError::ParseError(format!(
            "Invalid event ID format: {s}, expected source:epoch:seq"
        )));
    }
    let source: u32 = parts[0]
        .parse()
        .map_err(|e| QueryError::ParseError(format!("Invalid source: {e}")))?;
    let epoch: u64 = parts[1]
        .parse()
        .map_err(|e| QueryError::ParseError(format!("Invalid epoch: {e}")))?;
    let sequence: u64 = parts[2]
        .parse()
        .map_err(|e| QueryError::ParseError(format!("Invalid sequence: {e}")))?;
    Ok(EventId {
        source: SourceId(source),
        epoch: SourceEpoch(epoch),
        sequence,
    })
}

fn parse_range(s: &str) -> Result<Range<Timestamp>, QueryError> {
    let parts: Vec<&str> = s.split("..").collect();
    if parts.len() != 2 {
        return Err(QueryError::ParseError(format!(
            "Invalid range syntax: {s}, expected start..end"
        )));
    }
    let start: u64 = parts[0]
        .trim()
        .parse()
        .map_err(|e| QueryError::ParseError(format!("Invalid range start: {e}")))?;
    let end: u64 = parts[1]
        .trim()
        .parse()
        .map_err(|e| QueryError::ParseError(format!("Invalid range end: {e}")))?;
    Ok(Timestamp {
        clock: ClockId(1),
        ticks: start,
    }..Timestamp {
        clock: ClockId(1),
        ticks: end,
    })
}

fn parse_simple_expr(s: &str) -> Result<Expr, QueryError> {
    parse_expr(s)
}

/// Strongly typed recursive-descent scalar expression parser (M16/M22/M30).
pub fn parse_expr(input: &str) -> Result<Expr, QueryError> {
    let tokens = tokenize_expr(input)?;
    if tokens.is_empty() {
        return Err(QueryError::ParseError("Empty expression".to_string()));
    }
    let mut pos = 0;
    let expr = parse_or_expr(&tokens, &mut pos)?;
    if pos < tokens.len() {
        return Err(QueryError::ParseError(format!(
            "Unexpected trailing token at position {pos}: {:?}",
            tokens[pos]
        )));
    }
    Ok(expr)
}

#[derive(Debug, Clone, PartialEq)]
enum ExprToken {
    Ident(String),
    Int(i64),
    Float(f64),
    StringLit(String),
    Bool(bool),
    Op(BinaryOp),
    Not,
    LParen,
    RParen,
}

fn tokenize_expr(input: &str) -> Result<Vec<ExprToken>, QueryError> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '(' {
            tokens.push(ExprToken::LParen);
            i += 1;
        } else if c == ')' {
            tokens.push(ExprToken::RParen);
            i += 1;
        } else if c == '!' {
            if i + 1 < len && chars[i + 1] == '=' {
                tokens.push(ExprToken::Op(BinaryOp::NotEq));
                i += 2;
            } else {
                tokens.push(ExprToken::Not);
                i += 1;
            }
        } else if c == '=' {
            if i + 1 < len && chars[i + 1] == '=' {
                tokens.push(ExprToken::Op(BinaryOp::Eq));
                i += 2;
            } else {
                tokens.push(ExprToken::Op(BinaryOp::Eq));
                i += 1;
            }
        } else if c == '<' {
            if i + 1 < len && chars[i + 1] == '=' {
                tokens.push(ExprToken::Op(BinaryOp::Lte));
                i += 2;
            } else if i + 1 < len && chars[i + 1] == '>' {
                tokens.push(ExprToken::Op(BinaryOp::NotEq));
                i += 2;
            } else {
                tokens.push(ExprToken::Op(BinaryOp::Lt));
                i += 1;
            }
        } else if c == '>' {
            if i + 1 < len && chars[i + 1] == '=' {
                tokens.push(ExprToken::Op(BinaryOp::Gte));
                i += 2;
            } else {
                tokens.push(ExprToken::Op(BinaryOp::Gt));
                i += 1;
            }
        } else if c == '&' && i + 1 < len && chars[i + 1] == '&' {
            tokens.push(ExprToken::Op(BinaryOp::And));
            i += 2;
        } else if c == '|' && i + 1 < len && chars[i + 1] == '|' {
            tokens.push(ExprToken::Op(BinaryOp::Or));
            i += 2;
        } else if c == '\'' || c == '"' {
            let quote = c;
            i += 1;
            let mut s = String::new();
            while i < len && chars[i] != quote {
                s.push(chars[i]);
                i += 1;
            }
            if i >= len {
                return Err(QueryError::ParseError(
                    "Unterminated string literal".to_string(),
                ));
            }
            i += 1; // skip closing quote
            tokens.push(ExprToken::StringLit(s));
        } else if c.is_ascii_digit() {
            let start = i;
            let mut has_dot = false;
            while i < len && (chars[i].is_ascii_digit() || (chars[i] == '.' && !has_dot)) {
                if chars[i] == '.' {
                    has_dot = true;
                }
                i += 1;
            }
            let num_str: String = chars[start..i].iter().collect();
            if has_dot {
                let f: f64 = num_str
                    .parse()
                    .map_err(|e| QueryError::ParseError(format!("Invalid float: {e}")))?;
                tokens.push(ExprToken::Float(f));
            } else {
                let n: i64 = num_str
                    .parse()
                    .map_err(|e| QueryError::ParseError(format!("Invalid integer: {e}")))?;
                tokens.push(ExprToken::Int(n));
            }
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < len && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '.') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            match word.to_lowercase().as_str() {
                "true" => tokens.push(ExprToken::Bool(true)),
                "false" => tokens.push(ExprToken::Bool(false)),
                "not" => tokens.push(ExprToken::Not),
                "and" => tokens.push(ExprToken::Op(BinaryOp::And)),
                "or" => tokens.push(ExprToken::Op(BinaryOp::Or)),
                _ => tokens.push(ExprToken::Ident(word)),
            }
        } else {
            return Err(QueryError::ParseError(format!(
                "Unexpected character '{c}' in expression"
            )));
        }
    }
    Ok(tokens)
}

fn parse_or_expr(tokens: &[ExprToken], pos: &mut usize) -> Result<Expr, QueryError> {
    let mut left = parse_and_expr(tokens, pos)?;
    while *pos < tokens.len() {
        if let ExprToken::Op(BinaryOp::Or) = &tokens[*pos] {
            *pos += 1;
            let right = parse_and_expr(tokens, pos)?;
            left = Expr::Binary {
                op: BinaryOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
        } else {
            break;
        }
    }
    Ok(left)
}

fn parse_and_expr(tokens: &[ExprToken], pos: &mut usize) -> Result<Expr, QueryError> {
    let mut left = parse_cmp_expr(tokens, pos)?;
    while *pos < tokens.len() {
        if let ExprToken::Op(BinaryOp::And) = &tokens[*pos] {
            *pos += 1;
            let right = parse_cmp_expr(tokens, pos)?;
            left = Expr::Binary {
                op: BinaryOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        } else {
            break;
        }
    }
    Ok(left)
}

fn parse_cmp_expr(tokens: &[ExprToken], pos: &mut usize) -> Result<Expr, QueryError> {
    let left = parse_unary_expr(tokens, pos)?;
    if *pos < tokens.len() {
        if let ExprToken::Op(op) = &tokens[*pos] {
            if *op != BinaryOp::And && *op != BinaryOp::Or {
                let bin_op = *op;
                *pos += 1;
                let right = parse_unary_expr(tokens, pos)?;
                return Ok(Expr::Binary {
                    op: bin_op,
                    left: Box::new(left),
                    right: Box::new(right),
                });
            }
        }
    }
    Ok(left)
}

fn parse_unary_expr(tokens: &[ExprToken], pos: &mut usize) -> Result<Expr, QueryError> {
    if *pos < tokens.len() && tokens[*pos] == ExprToken::Not {
        *pos += 1;
        let inner = parse_unary_expr(tokens, pos)?;
        return Ok(Expr::Not(Box::new(inner)));
    }
    parse_primary_expr(tokens, pos)
}

fn parse_primary_expr(tokens: &[ExprToken], pos: &mut usize) -> Result<Expr, QueryError> {
    if *pos >= tokens.len() {
        return Err(QueryError::ParseError(
            "Unexpected end of expression".to_string(),
        ));
    }
    match &tokens[*pos] {
        ExprToken::LParen => {
            *pos += 1;
            let expr = parse_or_expr(tokens, pos)?;
            if *pos >= tokens.len() || tokens[*pos] != ExprToken::RParen {
                return Err(QueryError::ParseError(
                    "Missing closing parenthesis ')'".to_string(),
                ));
            }
            *pos += 1;
            Ok(expr)
        }
        ExprToken::Bool(b) => {
            *pos += 1;
            Ok(Expr::Literal(Literal::Bool(*b)))
        }
        ExprToken::Int(i) => {
            *pos += 1;
            Ok(Expr::Literal(Literal::Int(*i)))
        }
        ExprToken::Float(f) => {
            *pos += 1;
            Ok(Expr::Literal(Literal::Float(*f)))
        }
        ExprToken::StringLit(s) => {
            let val = s.clone();
            *pos += 1;
            Ok(Expr::Literal(Literal::String(val)))
        }
        ExprToken::Ident(s) => {
            let val = s.clone();
            *pos += 1;
            Ok(Expr::Field(val))
        }
        other => Err(QueryError::ParseError(format!(
            "Unexpected token: {other:?}"
        ))),
    }
}
