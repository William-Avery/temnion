// SPDX-License-Identifier: AGPL-3.0-only
use temnion_causal::{CausalError, CausalGraph, CsrCausalGraph};
use temnion_core::{EventId, SourceEpoch, SourceId};

fn eid(seq: u64) -> EventId {
    EventId {
        source: SourceId(1),
        epoch: SourceEpoch(1),
        sequence: seq,
    }
}

#[test]
fn immediate_causes_and_effects() {
    let mut graph = CausalGraph::new();
    let e1 = eid(1);
    let e2 = eid(2);
    let e3 = eid(3);

    graph.add_event(e1, vec![]).unwrap();
    graph.add_event(e2, vec![e1]).unwrap();
    graph.add_event(e3, vec![e1]).unwrap();

    assert_eq!(graph.immediate_causes(e1), &[]);
    assert_eq!(graph.immediate_causes(e2), &[e1]);
    assert_eq!(graph.immediate_causes(e3), &[e1]);

    let effects_of_e1 = graph.immediate_effects(e1);
    assert_eq!(effects_of_e1.len(), 2);
    assert!(effects_of_e1.contains(&e2));
    assert!(effects_of_e1.contains(&e3));
}

#[test]
fn diamond_causal_topology_and_transitive_tracing() {
    // A -> B -> D
    // A -> C -> D
    let mut graph = CausalGraph::new();
    let a = eid(10);
    let b = eid(20);
    let c = eid(30);
    let d = eid(40);

    graph.add_event(a, vec![]).unwrap();
    graph.add_event(b, vec![a]).unwrap();
    graph.add_event(c, vec![a]).unwrap();
    graph.add_event(d, vec![b, c]).unwrap();

    // Trace causes of D: should include D (depth 0), B & C (depth 1), A (depth 2)
    let trace_d = graph.trace_causes(d, 5);
    assert_eq!(trace_d.events.len(), 4);
    assert_eq!(trace_d.depths.get(&d), Some(&0));
    assert_eq!(trace_d.depths.get(&b), Some(&1));
    assert_eq!(trace_d.depths.get(&c), Some(&1));
    assert_eq!(trace_d.depths.get(&a), Some(&2));

    // Depth-bounded trace: depth 1
    let trace_d_bounded = graph.trace_causes(d, 1);
    assert_eq!(trace_d_bounded.events.len(), 3); // d, b, c (a is at depth 2)
    assert!(!trace_d_bounded.events.contains(&a));

    // Trace effects of A
    let trace_a = graph.trace_effects(a, 5);
    assert_eq!(trace_a.events.len(), 4);
    assert_eq!(trace_a.depths.get(&a), Some(&0));
    assert_eq!(trace_a.depths.get(&d), Some(&2));

    // Ancestry checks
    assert!(graph.is_ancestor_of(a, d));
    assert!(graph.is_ancestor_of(b, d));
    assert!(graph.is_ancestor_of(c, d));
    assert!(!graph.is_ancestor_of(d, a));
    assert!(!graph.is_ancestor_of(b, c));
}

#[test]
fn cycle_prevention_rejects_circular_edges() {
    let mut graph = CausalGraph::new();
    let e1 = eid(1);
    let e2 = eid(2);
    let e3 = eid(3);

    graph.add_event(e1, vec![]).unwrap();
    graph.add_event(e2, vec![e1]).unwrap();
    graph.add_event(e3, vec![e2]).unwrap();

    // Self-cycle: e4 caused by e4
    let e4 = eid(4);
    assert!(matches!(
        graph.add_event(e4, vec![e4]),
        Err(CausalError::CycleDetected { .. })
    ));

    // Indirect cycle: e1 caused by e3 (since e1 -> e2 -> e3)
    assert!(matches!(
        graph.add_event(e1, vec![e3]),
        Err(CausalError::CycleDetected { .. })
    ));
}

#[test]
fn topological_ordering_respects_causal_dependencies() {
    let mut graph = CausalGraph::new();
    let e1 = eid(1);
    let e2 = eid(2);
    let e3 = eid(3);
    let e4 = eid(4);

    graph.add_event(e1, vec![]).unwrap();
    graph.add_event(e2, vec![e1]).unwrap();
    graph.add_event(e3, vec![e1]).unwrap();
    graph.add_event(e4, vec![e2, e3]).unwrap();

    let order = graph.topological_sort().unwrap();
    assert_eq!(order.len(), 4);

    let pos = |id: EventId| order.iter().position(|&x| x == id).unwrap();
    assert!(pos(e1) < pos(e2));
    assert!(pos(e1) < pos(e3));
    assert!(pos(e2) < pos(e4));
    assert!(pos(e3) < pos(e4));
}

#[test]
fn csr_causal_graph_roundtrip_and_tamper_detection() {
    let mut graph = CausalGraph::new();
    let e1 = eid(10);
    let e2 = eid(20);
    let e3 = eid(30);

    graph.add_event(e1, vec![]).unwrap();
    graph.add_event(e2, vec![e1]).unwrap();
    graph.add_event(e3, vec![e1, e2]).unwrap();

    let csr = graph.to_csr();
    assert_eq!(csr.event_count(), 3);

    let encoded = csr.encode();
    let decoded = CsrCausalGraph::decode(&encoded).unwrap();
    assert_eq!(csr, decoded);

    // Tamper detection: flip 1 bit in payload
    let mut tampered = encoded.clone();
    tampered[16] ^= 0xFF;
    assert_eq!(
        CsrCausalGraph::decode(&tampered),
        Err(CausalError::ChecksumMismatch)
    );
}
