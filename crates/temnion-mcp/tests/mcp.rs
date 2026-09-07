// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]

use std::fs;
use std::io::Cursor;
use std::path::PathBuf;

use temnion_core::{
    ClockId, EntityId, EventTimes, SchemaId, ShardId, SourceEpoch, SourceId, Timestamp,
};
use temnion_format::Limits;
use temnion_mcp::{JsonValue, MCP_PROTOCOL_VERSION, McpServer};
use temnion_storage::{Store, WriteEvent};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("temnion-mcp-test-{name}-{}", std::process::id()));
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
fn json_value_dom_parsing_and_serialization() {
    let json_text = r#"{"name":"temnion","version":1,"active":true,"tags":["db","temporal"],"nested":{"null_val":null}}"#;
    let val = JsonValue::parse(json_text).expect("valid json should parse");

    assert_eq!(val.get("name").and_then(|v| v.as_str()), Some("temnion"));
    assert_eq!(val.get("version").and_then(|v| v.as_u64()), Some(1));
    assert_eq!(val.get("active").and_then(|v| v.as_bool()), Some(true));

    let nested = val.get("nested").unwrap();
    assert_eq!(nested.get("null_val"), Some(&JsonValue::Null));

    // Serialization roundtrip
    let serialized = val.serialize();
    let re_parsed = JsonValue::parse(&serialized).expect("serialized json should re-parse");
    assert_eq!(val, re_parsed);
}

#[test]
fn mcp_initialize_and_ping_protocol() {
    let mut server = McpServer::new();

    // 1. initialize
    let init_req = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}"#;
    let init_resp_str = server
        .handle_message(init_req)
        .expect("should return response");
    let init_resp = JsonValue::parse(&init_resp_str).unwrap();

    assert_eq!(init_resp.get("id").and_then(|v| v.as_u64()), Some(1));
    let res = init_resp.get("result").unwrap();
    assert_eq!(
        res.get("protocolVersion").and_then(|v| v.as_str()),
        Some(MCP_PROTOCOL_VERSION)
    );
    let server_info = res.get("serverInfo").unwrap();
    assert_eq!(
        server_info.get("name").and_then(|v| v.as_str()),
        Some("temnion-mcp")
    );

    // 2. ping
    let ping_req = r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#;
    let ping_resp_str = server
        .handle_message(ping_req)
        .expect("should return response");
    let ping_resp = JsonValue::parse(&ping_resp_str).unwrap();
    assert_eq!(ping_resp.get("id").and_then(|v| v.as_u64()), Some(2));
    assert!(ping_resp.get("result").is_some());
}

#[test]
fn mcp_tools_and_database_execution() {
    let dir = TempDir::new("mcp-tools");
    let mut store =
        Store::create(&dir.path, SourceId(1), SourceEpoch(1), Limits::default()).unwrap();

    let entity = EntityId {
        shard: ShardId(0),
        slot: 1,
        generation: 1,
    };

    let mut records = Vec::new();
    for i in 0..5 {
        records.push(WriteEvent {
            entity,
            times: EventTimes {
                valid: Timestamp::new(ClockId(1), 10 + i),
                observed: None,
                known: Timestamp::new(ClockId(1), 10 + i),
            },
            schema: SchemaId(1),
            payload: vec![i as u8, 0x55],
            causes: Vec::new(),
        });
    }
    store.append(records).unwrap();

    let mut server = McpServer::with_store(store, dir.path.to_str().unwrap());

    // 1. tools/list
    let list_req = r#"{"jsonrpc":"2.0","id":10,"method":"tools/list"}"#;
    let list_resp_str = server.handle_message(list_req).unwrap();
    let list_resp = JsonValue::parse(&list_resp_str).unwrap();
    let tools_res = list_resp.get("result").unwrap();
    let tools_arr = match tools_res.get("tools").unwrap() {
        JsonValue::Array(a) => a,
        _ => panic!("Expected tools array"),
    };
    assert!(tools_arr.len() >= 4);

    // 2. tools/call: inspect
    let inspect_req = r#"{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"inspect","arguments":{}}}"#;
    let inspect_resp_str = server.handle_message(inspect_req).unwrap();
    assert!(inspect_resp_str.contains("current_sequence=5"));

    // 3. tools/call: explain
    let explain_req = r#"{"jsonrpc":"2.0","id":12,"method":"tools/call","params":{"name":"explain","arguments":{"query":"FROM temnion\nENTITY 0:1:1"}}}"#;
    let explain_resp_str = server.handle_message(explain_req).unwrap();
    assert!(explain_resp_str.contains("StorageScan"));

    // 4. tools/call: query
    let query_req = r#"{"jsonrpc":"2.0","id":13,"method":"tools/call","params":{"name":"query","arguments":{"query":"FROM temnion\nENTITY 0:1:1\nTIME valid 10..13","max_rows":10}}}"#;
    let query_resp_str = server.handle_message(query_req).unwrap();
    assert!(query_resp_str.contains("Query returned 3 rows"));

    // 4b. tools/call: query via SQL
    let sql_query_req = r#"{"jsonrpc":"2.0","id":14,"method":"tools/call","params":{"name":"query","arguments":{"query":"SELECT * FROM temnion WHERE entity = '0:1:1' AND valid_time >= 10 AND valid_time < 13","max_rows":10}}}"#;
    let sql_query_resp_str = server.handle_message(sql_query_req).unwrap();
    assert!(sql_query_resp_str.contains("Query returned 3 rows"));

    // 4c. tools/call: explain via SQL
    let sql_explain_req = r#"{"jsonrpc":"2.0","id":15,"method":"tools/call","params":{"name":"explain","arguments":{"query":"SELECT * FROM temnion WHERE entity = '0:1:1'"}}}"#;
    let sql_explain_resp_str = server.handle_message(sql_explain_req).unwrap();
    assert!(sql_explain_resp_str.contains("StorageScan"));

    // 4d. tools/call: why_trace
    let why_req = r#"{"jsonrpc":"2.0","id":16,"method":"tools/call","params":{"name":"why_trace","arguments":{"id":"k:1"}}}"#;
    let why_resp_str = server.handle_message(why_req).unwrap();
    assert!(why_resp_str.contains("Epistemic WHY Trace"));
    assert!(why_resp_str.contains("Grounding Storage Events"));

    // 4e. tools/call: rewrite_expr
    let rewrite_req = r#"{"jsonrpc":"2.0","id":17,"method":"tools/call","params":{"name":"rewrite_expr","arguments":{"expr":"(status AND true) OR (status AND false)"}}}"#;
    let rewrite_resp_str = server.handle_message(rewrite_req).unwrap();
    assert!(rewrite_resp_str.contains("E-Graph Equality Saturation Complete"));
    assert!(rewrite_resp_str.contains("Optimized Expression: status"));

    // 4f. tools/call: evolution_status
    let evo_status_req = r#"{"jsonrpc":"2.0","id":18,"method":"tools/call","params":{"name":"evolution_status","arguments":{}}}"#;
    let evo_status_resp = server.handle_message(evo_status_req).unwrap();
    assert!(evo_status_resp.contains("Champion/Challenger Evolution Engine Status"));
    assert!(evo_status_resp.contains("Active Incumbents: 1"));

    // 4g. tools/call: candidate_evaluate
    let eval_req = r#"{"jsonrpc":"2.0","id":19,"method":"tools/call","params":{"name":"candidate_evaluate","arguments":{"domain":"segment_compression"}}}"#;
    let eval_resp = server.handle_message(eval_req).unwrap();
    assert!(eval_resp.contains("Candidate Evaluation Complete"));
    assert!(eval_resp.contains("Net Benefit Score"));
    assert!(eval_resp.contains("Promotable: true"));

    // 4h. tools/call: constitution_audit
    let audit_req = r#"{"jsonrpc":"2.0","id":20,"method":"tools/call","params":{"name":"constitution_audit","arguments":{}}}"#;
    let audit_resp = server.handle_message(audit_req).unwrap();
    assert!(audit_resp.contains("Constitution Audit Report"));
    assert!(audit_resp.contains("Conformance: CERTIFIED"));
    assert!(audit_resp.contains("Axioms Certified: 8/8"));

    // 5. tools/call: unknown tool
    let bad_req =
        r#"{"jsonrpc":"2.0","id":21,"method":"tools/call","params":{"name":"non_existent_tool"}}"#;
    let bad_resp_str = server.handle_message(bad_req).unwrap();
    assert!(bad_resp_str.contains("-32601"));
}

#[test]
fn mcp_resources_and_prompts() {
    let mut server = McpServer::new();

    // 1. resources/list
    let res_list_req = r#"{"jsonrpc":"2.0","id":20,"method":"resources/list"}"#;
    let res_list_str = server.handle_message(res_list_req).unwrap();
    assert!(res_list_str.contains("temnion://database/capabilities"));

    // 2. resources/read
    let res_read_req = r#"{"jsonrpc":"2.0","id":21,"method":"resources/read","params":{"uri":"temnion://database/capabilities"}}"#;
    let res_read_str = server.handle_message(res_read_req).unwrap();
    assert!(res_read_str.contains("mcp") && res_read_str.contains("flight"));

    // 3. prompts/list
    let prompt_list_req = r#"{"jsonrpc":"2.0","id":22,"method":"prompts/list"}"#;
    let prompt_list_str = server.handle_message(prompt_list_req).unwrap();
    assert!(prompt_list_str.contains("causal-investigation"));

    // 4. prompts/get
    let prompt_get_req = r#"{"jsonrpc":"2.0","id":23,"method":"prompts/get","params":{"name":"causal-investigation"}}"#;
    let prompt_get_str = server.handle_message(prompt_get_req).unwrap();
    assert!(prompt_get_str.contains("causal_trace"));
}

#[test]
fn mcp_stdio_stream_dispatch() {
    let mut server = McpServer::new();
    let input = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}\n";
    let reader = Cursor::new(input.as_bytes());
    let mut output = Vec::new();

    server.run_stdio(reader, &mut output).unwrap();

    let output_str = String::from_utf8(output).unwrap();
    let lines: Vec<&str> = output_str.trim().split('\n').collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("\"id\":1"));
    assert!(lines[1].contains("\"id\":2"));
}
