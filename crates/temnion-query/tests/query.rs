// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use temnion_causal::CausalGraph;
use temnion_core::{
    ClockId, EntityId, EventId, EventTimes, SchemaId, ShardId, SourceEpoch, SourceId, Timestamp,
};
use temnion_format::Limits;
use temnion_query::{
    BinaryOp, CausalDirection, Expr, Literal, LogicalPlan, PhysicalPlan, QueryBudget, QueryError,
    QueryExecutor, explain_query, parse_compact_tem, parse_sql, parse_temql, plan_query,
};
use temnion_storage::{Store, WriteEvent};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("temnion-query-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn expression_evaluation_and_type_checks() {
    let mut fields = HashMap::new();
    fields.insert("health".to_string(), Literal::Int(75));
    fields.insert("shield".to_string(), Literal::Float(12.5));
    fields.insert("visible".to_string(), Literal::Bool(true));
    fields.insert("name".to_string(), Literal::String("Unit7".to_string()));

    // health > 50 -> true
    let expr1 = Expr::Binary {
        op: BinaryOp::Gt,
        left: Box::new(Expr::Field("health".to_string())),
        right: Box::new(Expr::Literal(Literal::Int(50))),
    };
    assert_eq!(expr1.evaluate(&fields).unwrap(), Literal::Bool(true));

    // health <= 50 -> false
    let expr2 = Expr::Binary {
        op: BinaryOp::Lte,
        left: Box::new(Expr::Field("health".to_string())),
        right: Box::new(Expr::Literal(Literal::Int(50))),
    };
    assert_eq!(expr2.evaluate(&fields).unwrap(), Literal::Bool(false));

    // visible AND health > 50 -> true
    let expr3 = Expr::Binary {
        op: BinaryOp::And,
        left: Box::new(Expr::Field("visible".to_string())),
        right: Box::new(expr1),
    };
    assert_eq!(expr3.evaluate(&fields).unwrap(), Literal::Bool(true));

    // NOT visible -> false
    let expr4 = Expr::Not(Box::new(Expr::Field("visible".to_string())));
    assert_eq!(expr4.evaluate(&fields).unwrap(), Literal::Bool(false));

    // Missing field error
    let expr_missing = Expr::Field("mana".to_string());
    assert!(matches!(
        expr_missing.evaluate(&fields),
        Err(QueryError::ExecutionError(_))
    ));
}

#[test]
fn sql_temql_and_compact_tem_canonical_equivalence() {
    let temql_input = r#"
        FROM temnion
        ENTITY 0:1:0
        TIME valid 10..20
        KNOWN_AS_OF 20
        WHERE health > 50
        SELECT position, health
        LIMIT 32
    "#;

    let compact_input = "tn:#0:1:0@v10..20@k20?health>50>position,health!32";

    let sql_input = r#"
        SELECT position, health
        FROM temnion
        WHERE entity = '#0:1:0'
          AND valid_time >= 10
          AND valid_time < 20
          AND known_as_of = 20
          AND health > 50
        LIMIT 32
    "#;

    let plan_from_temql = parse_temql(temql_input).unwrap();
    let plan_from_compact = parse_compact_tem(compact_input).unwrap();
    let plan_from_sql = parse_sql(sql_input).unwrap();

    let expected = LogicalPlan::Scan {
        entity: Some(EntityId {
            shard: ShardId(0),
            slot: 1,
            generation: 0,
        }),
        schema: None,
        valid_range: Some(
            Timestamp {
                clock: ClockId(1),
                ticks: 10,
            }..Timestamp {
                clock: ClockId(1),
                ticks: 20,
            },
        ),
        known_as_of: Some(Timestamp {
            clock: ClockId(1),
            ticks: 20,
        }),
        filter: Some(Expr::Binary {
            op: BinaryOp::Gt,
            left: Box::new(Expr::Field("health".to_string())),
            right: Box::new(Expr::Literal(Literal::Int(50))),
        }),
        projection: Some(vec!["position".to_string(), "health".to_string()]),
        limit: Some(32),
    };

    assert_eq!(plan_from_temql, expected);
    assert_eq!(plan_from_compact, expected);
    assert_eq!(plan_from_sql, expected);
    assert_eq!(plan_from_temql, plan_from_compact);
    assert_eq!(plan_from_temql, plan_from_sql);
}

#[test]
fn temql_causal_and_compact_causal_equivalence() {
    let temql_causes = "EVENT 1:2:100 TRACE CAUSES DEPTH 8 LIMIT 16";
    let compact_causes = "tn:$1:2:100<-8";

    let plan_temql = parse_temql(temql_causes).unwrap();
    let plan_compact = parse_compact_tem(compact_causes).unwrap();

    let expected_causes = LogicalPlan::CausalTrace {
        origin: EventId {
            source: SourceId(1),
            epoch: SourceEpoch(2),
            sequence: 100,
        },
        direction: CausalDirection::Causes,
        max_depth: Some(8),
        limit: Some(16),
    };

    assert_eq!(plan_temql, expected_causes);
    if let LogicalPlan::CausalTrace {
        origin,
        direction,
        max_depth,
        ..
    } = plan_compact
    {
        assert_eq!(
            origin,
            EventId {
                source: SourceId(1),
                epoch: SourceEpoch(2),
                sequence: 100
            }
        );
        assert_eq!(direction, CausalDirection::Causes);
        assert_eq!(max_depth, Some(8));
    } else {
        panic!("Expected CausalTrace plan");
    }

    let temql_effects = "EVENT 2:1:500 TRACE EFFECTS DEPTH 4";
    let compact_effects = "tn:$2:1:500->4";

    let plan_effects_temql = parse_temql(temql_effects).unwrap();
    let plan_effects_compact = parse_compact_tem(compact_effects).unwrap();

    assert_eq!(
        plan_effects_temql,
        LogicalPlan::CausalTrace {
            origin: EventId {
                source: SourceId(2),
                epoch: SourceEpoch(1),
                sequence: 500
            },
            direction: CausalDirection::Effects,
            max_depth: Some(4),
            limit: None,
        }
    );
    assert_eq!(plan_effects_temql, plan_effects_compact);
}

#[test]
fn physical_plan_and_explain_output() {
    let logical = LogicalPlan::Scan {
        entity: Some(EntityId {
            shard: ShardId(0),
            slot: 42,
            generation: 1,
        }),
        schema: Some(SchemaId(1)),
        valid_range: Some(
            Timestamp {
                clock: ClockId(1),
                ticks: 100,
            }..Timestamp {
                clock: ClockId(1),
                ticks: 200,
            },
        ),
        known_as_of: Some(Timestamp {
            clock: ClockId(1),
            ticks: 200,
        }),
        filter: Some(Expr::Binary {
            op: BinaryOp::Lt,
            left: Box::new(Expr::Field("sequence".to_string())),
            right: Box::new(Expr::Literal(Literal::Int(10))),
        }),
        projection: Some(vec!["valid_time".to_string(), "sequence".to_string()]),
        limit: Some(5),
    };

    let physical = plan_query(&logical);
    match &physical {
        PhysicalPlan::StorageScan {
            use_zone_maps,
            use_bloom,
            limit,
            ..
        } => {
            assert!(*use_zone_maps);
            assert!(*use_bloom);
            assert_eq!(*limit, Some(5));
        }
        _ => panic!("Expected StorageScan"),
    }

    let explain = explain_query(&logical);
    let explain_str = explain.to_string();
    assert!(explain_str.contains("StorageScan"));
    assert!(explain_str.contains("entity=0:42:1"));
    assert!(explain_str.contains("valid_range=100..200"));
    assert!(explain_str.contains("pushdown_zone_maps=true, pushdown_bloom=true"));
    assert!(explain_str.contains("limit=5"));
}

#[test]
fn query_executor_storage_scan_with_filters_and_budgets() {
    let dir = TempDir::new("storage-scan");
    let mut store =
        Store::create(&dir.path, SourceId(1), SourceEpoch(1), Limits::default()).unwrap();

    let entity1 = EntityId {
        shard: ShardId(0),
        slot: 1,
        generation: 1,
    };
    let entity2 = EntityId {
        shard: ShardId(0),
        slot: 2,
        generation: 1,
    };

    let mut records = Vec::new();
    for i in 0..10 {
        let entity = if i % 2 == 0 { entity1 } else { entity2 };
        records.push(WriteEvent {
            entity,
            times: EventTimes {
                valid: Timestamp::new(ClockId(1), 10 + i),
                observed: None,
                known: Timestamp::new(ClockId(1), 10 + i),
            },
            schema: SchemaId(1),
            payload: vec![i as u8, 0xAA],
            causes: Vec::new(),
        });
    }

    store.append(records).unwrap();

    // Query entity1 only within valid range 10..16
    let plan = PhysicalPlan::StorageScan {
        entity: Some(entity1),
        valid_range: Some(
            Timestamp {
                clock: ClockId(1),
                ticks: 10,
            }..Timestamp {
                clock: ClockId(1),
                ticks: 16,
            },
        ),
        known_as_of: None,
        pushdown_filter: None,
        projection: Some(vec!["valid_time".to_string(), "sequence".to_string()]),
        limit: Some(10),
        use_zone_maps: true,
        use_bloom: true,
    };

    let result =
        QueryExecutor::execute_storage_scan(&mut store, &plan, &QueryBudget::default()).unwrap();
    // Entity1 appears at i = 0, 2, 4 (ticks 10, 12, 14; 16 is exclusive)
    assert_eq!(result.rows.len(), 3);
    for row in &result.rows {
        assert_eq!(row.entity, entity1);
        assert!(row.valid_time.ticks >= 10 && row.valid_time.ticks < 16);
        assert!(row.fields.contains_key("valid_time"));
        assert!(row.fields.contains_key("sequence"));
    }

    // Test budget enforcement (limit 2 rows)
    let budget = QueryBudget {
        max_rows: Some(2),
        max_events_scanned: Some(100),
        max_bytes: Some(1024 * 1024),
    };
    let budget_result = QueryExecutor::execute_storage_scan(&mut store, &plan, &budget).unwrap();
    assert_eq!(budget_result.rows.len(), 2);
}

#[test]
fn query_executor_causal_walk() {
    let mut graph = CausalGraph::new();
    let e0 = EventId {
        source: SourceId(1),
        epoch: SourceEpoch(1),
        sequence: 0,
    };
    let e1 = EventId {
        source: SourceId(1),
        epoch: SourceEpoch(1),
        sequence: 1,
    };
    let e2 = EventId {
        source: SourceId(1),
        epoch: SourceEpoch(1),
        sequence: 2,
    };
    let e3 = EventId {
        source: SourceId(1),
        epoch: SourceEpoch(1),
        sequence: 3,
    };

    // e0 -> e1 -> e2 -> e3
    graph.add_event(e0, vec![]).unwrap();
    graph.add_event(e1, vec![e0]).unwrap();
    graph.add_event(e2, vec![e1]).unwrap();
    graph.add_event(e3, vec![e2]).unwrap();

    // Trace causes of e3 up to depth 2: should find [e3, e2, e1]
    let plan_causes = PhysicalPlan::CausalWalk {
        origin: e3,
        direction: CausalDirection::Causes,
        max_depth: 2,
        limit: Some(10),
    };

    let causes =
        QueryExecutor::execute_causal_walk(&graph, &plan_causes, &QueryBudget::default()).unwrap();
    assert_eq!(causes, vec![e3, e2, e1]);

    // Trace effects of e0 up to depth 2: should find [e0, e1, e2]
    let plan_effects = PhysicalPlan::CausalWalk {
        origin: e0,
        direction: CausalDirection::Effects,
        max_depth: 2,
        limit: Some(10),
    };

    let effects =
        QueryExecutor::execute_causal_walk(&graph, &plan_effects, &QueryBudget::default()).unwrap();
    assert_eq!(effects, vec![e0, e1, e2]);
}
