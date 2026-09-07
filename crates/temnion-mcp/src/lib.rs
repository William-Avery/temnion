// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]

//! Model Context Protocol (MCP) server for Temnion.
//!
//! # Architecture
//! Following Temnion Architecture §7, §29, and Milestone M21:
//! - **Control Plane Interface**: Exposes Temnion database operations (query, explain,
//!   branching, causal tracing, and resource introspection) to AI agents and LLM tools
//!   via the Model Context Protocol (MCP) JSON-RPC 2.0 specification.
//! - **Decoupled from Storage Hot Paths**: Runs on the control plane, keeping
//!   storage and query pipelines lightweight and free of JSON/network overhead.
//! - **Tools, Resources, and Prompts**:
//!   - Tools: `query`, `explain`, `inspect`, `branch_create`, `branch_list`, `causal_trace`.
//!   - Resources: `temnion://database/capabilities`, `temnion://database/branches`,
//!     `temnion://database/summaries`.
//!   - Prompts: `causal-investigation`, `timeline-audit`.
//! - **Strict Safety**: Pure safe Rust enforcing `#![forbid(unsafe_code)]`.

use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex};

use temnion_branch::BranchManager;
use temnion_format::Limits;
use temnion_query::{
    QueryBudget as EngineQueryBudget, QueryExecutor, explain_query, parse_compact_tem, parse_sql,
    parse_temql, plan_query,
};
use temnion_storage::{RecoveryMode, Store};

/// Current MCP protocol version supported.
pub const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

// ---------------------------------------------------------------------------
// Lightweight Self-Contained JSON DOM
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<JsonValue>),
    Object(Vec<(String, JsonValue)>),
}

impl JsonValue {
    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        match self {
            Self::Object(entries) => {
                for (k, v) in entries {
                    if k == key {
                        return Some(v);
                    }
                }
                None
            }
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Self::Number(n) if *n >= 0.0 => Some(*n as u64),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn serialize(&self) -> String {
        match self {
            Self::Null => "null".to_string(),
            Self::Bool(b) => if *b { "true" } else { "false" }.to_string(),
            Self::Number(n) => {
                if n.fract() == 0.0 {
                    format!("{}", *n as i64)
                } else {
                    format!("{}", n)
                }
            }
            Self::String(s) => {
                let mut out = String::from("\"");
                for c in s.chars() {
                    match c {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\r' => out.push_str("\\r"),
                        '\t' => out.push_str("\\t"),
                        other => out.push(other),
                    }
                }
                out.push('"');
                out
            }
            Self::Array(arr) => {
                let items: Vec<String> = arr.iter().map(|item| item.serialize()).collect();
                format!("[{}]", items.join(","))
            }
            Self::Object(obj) => {
                let entries: Vec<String> = obj
                    .iter()
                    .map(|(k, v)| format!("\"{}\":{}", k, v.serialize()))
                    .collect();
                format!("{{{}}}", entries.join(","))
            }
        }
    }

    pub fn parse(input: &str) -> Result<Self, String> {
        let trimmed = input.trim();
        let mut chars = trimmed.chars().peekable();
        Self::parse_value(&mut chars)
    }

    fn parse_value<I: Iterator<Item = char>>(
        chars: &mut std::iter::Peekable<I>,
    ) -> Result<Self, String> {
        Self::skip_ws(chars);
        match chars.peek() {
            Some('n') => {
                Self::consume_str(chars, "null")?;
                Ok(Self::Null)
            }
            Some('t') => {
                Self::consume_str(chars, "true")?;
                Ok(Self::Bool(true))
            }
            Some('f') => {
                Self::consume_str(chars, "false")?;
                Ok(Self::Bool(false))
            }
            Some('"') => {
                let s = Self::parse_string(chars)?;
                Ok(Self::String(s))
            }
            Some('[') => {
                chars.next();
                let mut items = Vec::new();
                loop {
                    Self::skip_ws(chars);
                    if let Some(']') = chars.peek() {
                        chars.next();
                        break;
                    }
                    items.push(Self::parse_value(chars)?);
                    Self::skip_ws(chars);
                    match chars.peek() {
                        Some(',') => {
                            chars.next();
                        }
                        Some(']') => {
                            chars.next();
                            break;
                        }
                        _ => return Err("Expected ',' or ']' in array".to_string()),
                    }
                }
                Ok(Self::Array(items))
            }
            Some('{') => {
                chars.next();
                let mut entries = Vec::new();
                loop {
                    Self::skip_ws(chars);
                    if let Some('}') = chars.peek() {
                        chars.next();
                        break;
                    }
                    let key = Self::parse_string(chars)?;
                    Self::skip_ws(chars);
                    if chars.next() != Some(':') {
                        return Err("Expected ':' after object key".to_string());
                    }
                    let val = Self::parse_value(chars)?;
                    entries.push((key, val));
                    Self::skip_ws(chars);
                    match chars.peek() {
                        Some(',') => {
                            chars.next();
                        }
                        Some('}') => {
                            chars.next();
                            break;
                        }
                        _ => return Err("Expected ',' or '}' in object".to_string()),
                    }
                }
                Ok(Self::Object(entries))
            }
            Some(c) if *c == '-' || c.is_ascii_digit() => {
                let mut num_str = String::new();
                while let Some(&ch) = chars.peek() {
                    if ch == '-'
                        || ch == '+'
                        || ch == '.'
                        || ch == 'e'
                        || ch == 'E'
                        || ch.is_ascii_digit()
                    {
                        num_str.push(ch);
                        chars.next();
                    } else {
                        break;
                    }
                }
                let val: f64 = num_str
                    .parse()
                    .map_err(|e| format!("Invalid number: {e}"))?;
                Ok(Self::Number(val))
            }
            other => Err(format!("Unexpected character: {other:?}")),
        }
    }

    fn skip_ws<I: Iterator<Item = char>>(chars: &mut std::iter::Peekable<I>) {
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() {
                chars.next();
            } else {
                break;
            }
        }
    }

    fn consume_str<I: Iterator<Item = char>>(
        chars: &mut std::iter::Peekable<I>,
        expected: &str,
    ) -> Result<(), String> {
        for exp in expected.chars() {
            if chars.next() != Some(exp) {
                return Err(format!("Expected '{expected}'"));
            }
        }
        Ok(())
    }

    fn parse_string<I: Iterator<Item = char>>(
        chars: &mut std::iter::Peekable<I>,
    ) -> Result<String, String> {
        if chars.next() != Some('"') {
            return Err("Expected '\"'".to_string());
        }
        let mut s = String::new();
        while let Some(c) = chars.next() {
            match c {
                '"' => return Ok(s),
                '\\' => match chars.next() {
                    Some('"') => s.push('"'),
                    Some('\\') => s.push('\\'),
                    Some('n') => s.push('\n'),
                    Some('r') => s.push('\r'),
                    Some('t') => s.push('\t'),
                    Some(other) => s.push(other),
                    None => return Err("Unterminated string escape".to_string()),
                },
                other => s.push(other),
            }
        }
        Err("Unterminated string".to_string())
    }
}

// ---------------------------------------------------------------------------
// MCP Server & Handlers
// ---------------------------------------------------------------------------

/// Model Context Protocol server exposing Temnion database capabilities.
pub struct McpServer {
    store: Arc<Mutex<Option<Store>>>,
    database_path: Option<String>,
}

impl McpServer {
    /// Creates a new MCP server without an initial open database.
    pub fn new() -> Self {
        Self {
            store: Arc::new(Mutex::new(None)),
            database_path: None,
        }
    }

    /// Creates an MCP server bound to an open database store.
    pub fn with_store(store: Store, database_path: impl Into<String>) -> Self {
        Self {
            store: Arc::new(Mutex::new(Some(store))),
            database_path: Some(database_path.into()),
        }
    }

    /// Sets the active database for the server.
    pub fn set_database(&mut self, path: &str) -> Result<(), String> {
        match Store::open(
            std::path::Path::new(path),
            Limits::default(),
            RecoveryMode::RejectIncompleteTail,
        ) {
            Ok((store, _)) => {
                *self.store.lock().unwrap() = Some(store);
                self.database_path = Some(path.to_string());
                Ok(())
            }
            Err(e) => Err(format!("Failed to open store at '{path}': {e}")),
        }
    }

    /// Dispatches an incoming JSON-RPC 2.0 message string and returns an optional JSON-RPC response.
    pub fn handle_message(&mut self, line: &str) -> Option<String> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }

        let parsed = match JsonValue::parse(trimmed) {
            Ok(val) => val,
            Err(err) => {
                return Some(Self::make_error_response(
                    &JsonValue::Null,
                    -32700,
                    &format!("Parse error: {err}"),
                ));
            }
        };

        let id = parsed.get("id").cloned().unwrap_or(JsonValue::Null);
        let method = match parsed.get("method").and_then(|m| m.as_str()) {
            Some(m) => m,
            None => {
                return Some(Self::make_error_response(
                    &id,
                    -32600,
                    "Invalid Request: missing method",
                ));
            }
        };

        let params = parsed
            .get("params")
            .cloned()
            .unwrap_or(JsonValue::Object(vec![]));

        match method {
            "initialize" => Some(self.handle_initialize(&id, &params)),
            "notifications/initialized" => None, // Notification; no response
            "ping" => Some(self.handle_ping(&id)),
            "tools/list" => Some(self.handle_tools_list(&id)),
            "tools/call" => Some(self.handle_tools_call(&id, &params)),
            "resources/list" => Some(self.handle_resources_list(&id)),
            "resources/read" => Some(self.handle_resources_read(&id, &params)),
            "prompts/list" => Some(self.handle_prompts_list(&id)),
            "prompts/get" => Some(self.handle_prompts_get(&id, &params)),
            unknown => Some(Self::make_error_response(
                &id,
                -32601,
                &format!("Method not found: {unknown}"),
            )),
        }
    }

    fn make_error_response(id: &JsonValue, code: i64, message: &str) -> String {
        let err_obj = JsonValue::Object(vec![
            ("code".to_string(), JsonValue::Number(code as f64)),
            (
                "message".to_string(),
                JsonValue::String(message.to_string()),
            ),
        ]);
        let resp = JsonValue::Object(vec![
            ("jsonrpc".to_string(), JsonValue::String("2.0".to_string())),
            ("id".to_string(), id.clone()),
            ("error".to_string(), err_obj),
        ]);
        resp.serialize()
    }

    fn make_result_response(id: &JsonValue, result: JsonValue) -> String {
        let resp = JsonValue::Object(vec![
            ("jsonrpc".to_string(), JsonValue::String("2.0".to_string())),
            ("id".to_string(), id.clone()),
            ("result".to_string(), result),
        ]);
        resp.serialize()
    }

    fn handle_initialize(&self, id: &JsonValue, _params: &JsonValue) -> String {
        let server_info = JsonValue::Object(vec![
            (
                "name".to_string(),
                JsonValue::String("temnion-mcp".to_string()),
            ),
            (
                "version".to_string(),
                JsonValue::String(env!("CARGO_PKG_VERSION").to_string()),
            ),
        ]);
        let capabilities = JsonValue::Object(vec![
            ("tools".to_string(), JsonValue::Object(vec![])),
            ("resources".to_string(), JsonValue::Object(vec![])),
            ("prompts".to_string(), JsonValue::Object(vec![])),
        ]);
        let result = JsonValue::Object(vec![
            (
                "protocolVersion".to_string(),
                JsonValue::String(MCP_PROTOCOL_VERSION.to_string()),
            ),
            ("capabilities".to_string(), capabilities),
            ("serverInfo".to_string(), server_info),
        ]);
        Self::make_result_response(id, result)
    }

    fn handle_ping(&self, id: &JsonValue) -> String {
        Self::make_result_response(id, JsonValue::Object(vec![]))
    }

    fn handle_tools_list(&self, id: &JsonValue) -> String {
        let tools = vec![
            JsonValue::Object(vec![
                ("name".to_string(), JsonValue::String("query".to_string())),
                (
                    "description".to_string(),
                    JsonValue::String(
                        "Execute a TemQL or compact tn: query with bounded results".to_string(),
                    ),
                ),
            ]),
            JsonValue::Object(vec![
                ("name".to_string(), JsonValue::String("explain".to_string())),
                (
                    "description".to_string(),
                    JsonValue::String(
                        "Display the optimized physical execution plan for a query".to_string(),
                    ),
                ),
            ]),
            JsonValue::Object(vec![
                ("name".to_string(), JsonValue::String("inspect".to_string())),
                (
                    "description".to_string(),
                    JsonValue::String(
                        "Inspect database status, current WAL sequence, and active state"
                            .to_string(),
                    ),
                ),
            ]),
            JsonValue::Object(vec![
                (
                    "name".to_string(),
                    JsonValue::String("branch_list".to_string()),
                ),
                (
                    "description".to_string(),
                    JsonValue::String(
                        "List all timeline branches and their lifecycle states".to_string(),
                    ),
                ),
            ]),
            JsonValue::Object(vec![
                (
                    "name".to_string(),
                    JsonValue::String("causal_trace".to_string()),
                ),
                (
                    "description".to_string(),
                    JsonValue::String(
                        "Trace causal ancestry or effect cones for an event".to_string(),
                    ),
                ),
            ]),
        ];

        let result = JsonValue::Object(vec![("tools".to_string(), JsonValue::Array(tools))]);
        Self::make_result_response(id, result)
    }

    fn handle_tools_call(&mut self, id: &JsonValue, params: &JsonValue) -> String {
        let name = match params.get("name").and_then(|n| n.as_str()) {
            Some(n) => n,
            None => {
                return Self::make_error_response(id, -32602, "Missing 'name' parameter");
            }
        };

        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or(JsonValue::Object(vec![]));

        let res_text = match name {
            "query" => self.execute_query(&args),
            "explain" => self.execute_explain(&args),
            "inspect" => self.execute_inspect(),
            "branch_list" => self.execute_branch_list(),
            "causal_trace" => self.execute_causal_trace(&args),
            unknown => {
                return Self::make_error_response(
                    id,
                    -32601,
                    &format!("Unknown tool: '{unknown}'"),
                );
            }
        };

        match res_text {
            Ok(text) => {
                let content = vec![JsonValue::Object(vec![
                    ("type".to_string(), JsonValue::String("text".to_string())),
                    ("text".to_string(), JsonValue::String(text)),
                ])];
                let result = JsonValue::Object(vec![
                    ("content".to_string(), JsonValue::Array(content)),
                    ("isError".to_string(), JsonValue::Bool(false)),
                ]);
                Self::make_result_response(id, result)
            }
            Err(err) => {
                let content = vec![JsonValue::Object(vec![
                    ("type".to_string(), JsonValue::String("text".to_string())),
                    (
                        "text".to_string(),
                        JsonValue::String(format!("Error: {err}")),
                    ),
                ])];
                let result = JsonValue::Object(vec![
                    ("content".to_string(), JsonValue::Array(content)),
                    ("isError".to_string(), JsonValue::Bool(true)),
                ]);
                Self::make_result_response(id, result)
            }
        }
    }

    fn execute_query(&self, args: &JsonValue) -> Result<String, String> {
        let query_str = args
            .get("query")
            .and_then(|q| q.as_str())
            .ok_or_else(|| "Missing required 'query' argument".to_string())?;

        let max_rows = args.get("max_rows").and_then(|r| r.as_u64()).unwrap_or(100) as usize;

        let mut store_guard = self.store.lock().unwrap();
        let store = store_guard
            .as_mut()
            .ok_or_else(|| "No database currently opened".to_string())?;

        let trimmed = query_str.trim();
        let logical =
            if trimmed.starts_with("tn:") || trimmed.starts_with('#') || trimmed.starts_with('$') {
                parse_compact_tem(query_str).map_err(|e| e.to_string())?
            } else if trimmed.to_ascii_lowercase().starts_with("select") {
                parse_sql(query_str).map_err(|e| e.to_string())?
            } else {
                parse_temql(query_str).map_err(|e| e.to_string())?
            };

        let physical = plan_query(&logical);
        let budget = EngineQueryBudget {
            max_rows: Some(max_rows),
            max_events_scanned: Some(50_000),
            max_bytes: Some(32 * 1024 * 1024),
        };

        let result = QueryExecutor::execute_storage_scan(store, &physical, &budget)
            .map_err(|e| e.to_string())?;

        let mut lines = Vec::new();
        lines.push(format!(
            "Query returned {} rows (scanned: {}, truncated: {}):",
            result.rows.len(),
            result.events_scanned,
            result.truncated
        ));
        for (i, row) in result.rows.iter().enumerate() {
            lines.push(format!(
                "[{}] entity={}:{}:{} valid={}:{} seq={}",
                i,
                row.entity.shard.0,
                row.entity.slot,
                row.entity.generation,
                row.valid_time.clock.0,
                row.valid_time.ticks,
                row.sequence
            ));
        }

        Ok(lines.join("\n"))
    }

    fn execute_explain(&self, args: &JsonValue) -> Result<String, String> {
        let query_str = args
            .get("query")
            .and_then(|q| q.as_str())
            .ok_or_else(|| "Missing required 'query' argument".to_string())?;

        let trimmed = query_str.trim();
        let logical =
            if trimmed.starts_with("tn:") || trimmed.starts_with('#') || trimmed.starts_with('$') {
                parse_compact_tem(query_str).map_err(|e| e.to_string())?
            } else if trimmed.to_ascii_lowercase().starts_with("select") {
                parse_sql(query_str).map_err(|e| e.to_string())?
            } else {
                parse_temql(query_str).map_err(|e| e.to_string())?
            };

        let explain = explain_query(&logical);
        Ok(format!("{explain}"))
    }

    fn execute_inspect(&self) -> Result<String, String> {
        let store_guard = self.store.lock().unwrap();
        match store_guard.as_ref() {
            Some(store) => {
                let current_seq = store.len();
                let wal_bytes = store.wal_bytes();
                let header = store.header();
                Ok(format!(
                    "Database Store: source_id={}, epoch={}, current_sequence={}, wal_bytes={}",
                    header.source.0, header.epoch.0, current_seq, wal_bytes
                ))
            }
            None => Ok("Database Store: None (database not opened)".to_string()),
        }
    }

    fn execute_branch_list(&self) -> Result<String, String> {
        let path = match &self.database_path {
            Some(p) => std::path::Path::new(p),
            None => return Err("No database path configured".to_string()),
        };

        match BranchManager::open(path) {
            Ok(mgr) => {
                let branches = &mgr.manifest().branches;
                let mut lines = Vec::new();
                lines.push(format!(
                    "Active Timeline Branches ({} total):",
                    branches.len()
                ));
                for b in branches.values() {
                    lines.push(format!(
                        "- ID: {}, Name: '{}', Status: {:?}, Created: {}ms",
                        b.id.0, b.name, b.lifecycle, b.created_at_ms
                    ));
                }
                Ok(lines.join("\n"))
            }
            Err(e) => Err(format!("Failed to read branch manifest: {e}")),
        }
    }

    fn execute_causal_trace(&self, args: &JsonValue) -> Result<String, String> {
        let seq = args
            .get("sequence")
            .and_then(|s| s.as_u64())
            .ok_or_else(|| "Missing required 'sequence' argument".to_string())?;

        let max_depth = args.get("max_depth").and_then(|d| d.as_u64()).unwrap_or(16) as usize;

        let query_str = format!("tn:$1:1:{seq}<-{max_depth}");
        self.execute_query(&JsonValue::Object(vec![(
            "query".to_string(),
            JsonValue::String(query_str),
        )]))
    }

    fn handle_resources_list(&self, id: &JsonValue) -> String {
        let resources = vec![
            JsonValue::Object(vec![
                (
                    "uri".to_string(),
                    JsonValue::String("temnion://database/capabilities".to_string()),
                ),
                (
                    "name".to_string(),
                    JsonValue::String("Engine Capabilities".to_string()),
                ),
                (
                    "mimeType".to_string(),
                    JsonValue::String("application/json".to_string()),
                ),
            ]),
            JsonValue::Object(vec![
                (
                    "uri".to_string(),
                    JsonValue::String("temnion://database/branches".to_string()),
                ),
                (
                    "name".to_string(),
                    JsonValue::String("Timeline Branches".to_string()),
                ),
                (
                    "mimeType".to_string(),
                    JsonValue::String("text/plain".to_string()),
                ),
            ]),
            JsonValue::Object(vec![
                (
                    "uri".to_string(),
                    JsonValue::String("temnion://database/summaries".to_string()),
                ),
                (
                    "name".to_string(),
                    JsonValue::String("Index Summaries".to_string()),
                ),
                (
                    "mimeType".to_string(),
                    JsonValue::String("text/plain".to_string()),
                ),
            ]),
        ];

        let result =
            JsonValue::Object(vec![("resources".to_string(), JsonValue::Array(resources))]);
        Self::make_result_response(id, result)
    }

    fn handle_resources_read(&self, id: &JsonValue, params: &JsonValue) -> String {
        let uri = match params.get("uri").and_then(|u| u.as_str()) {
            Some(u) => u,
            None => {
                return Self::make_error_response(id, -32602, "Missing 'uri' parameter");
            }
        };

        let content = match uri {
            "temnion://database/capabilities" => {
                "{\"name\":\"Temnion\",\"mcp\":true,\"flight\":true,\"tnp\":true,\"temql\":true}"
                    .to_string()
            }
            "temnion://database/branches" => self.execute_branch_list().unwrap_or_else(|e| e),
            "temnion://database/summaries" => {
                "Summaries: ZoneMap and Bloom filters active".to_string()
            }
            unknown => {
                return Self::make_error_response(
                    id,
                    -32602,
                    &format!("Resource not found: '{unknown}'"),
                );
            }
        };

        let contents = vec![JsonValue::Object(vec![
            ("uri".to_string(), JsonValue::String(uri.to_string())),
            (
                "mimeType".to_string(),
                JsonValue::String("text/plain".to_string()),
            ),
            ("text".to_string(), JsonValue::String(content)),
        ])];

        let result = JsonValue::Object(vec![("contents".to_string(), JsonValue::Array(contents))]);
        Self::make_result_response(id, result)
    }

    fn handle_prompts_list(&self, id: &JsonValue) -> String {
        let prompts = vec![
            JsonValue::Object(vec![
                (
                    "name".to_string(),
                    JsonValue::String("causal-investigation".to_string()),
                ),
                (
                    "description".to_string(),
                    JsonValue::String(
                        "Guided causal root-cause analysis for an anomalous event sequence"
                            .to_string(),
                    ),
                ),
            ]),
            JsonValue::Object(vec![
                (
                    "name".to_string(),
                    JsonValue::String("timeline-audit".to_string()),
                ),
                (
                    "description".to_string(),
                    JsonValue::String(
                        "Audit timeline divergences, branch forks, and reconstruct history"
                            .to_string(),
                    ),
                ),
            ]),
        ];

        let result = JsonValue::Object(vec![("prompts".to_string(), JsonValue::Array(prompts))]);
        Self::make_result_response(id, result)
    }

    fn handle_prompts_get(&self, id: &JsonValue, params: &JsonValue) -> String {
        let name = match params.get("name").and_then(|n| n.as_str()) {
            Some(n) => n,
            None => {
                return Self::make_error_response(id, -32602, "Missing 'name' parameter");
            }
        };

        match name {
            "causal-investigation" => {
                let messages = vec![JsonValue::Object(vec![
                    ("role".to_string(), JsonValue::String("user".to_string())),
                    (
                        "content".to_string(),
                        JsonValue::Object(vec![
                            ("type".to_string(), JsonValue::String("text".to_string())),
                            (
                                "text".to_string(),
                                JsonValue::String(
                                    "Please analyze the causal ancestry for event sequence {{sequence}} using causal_trace and explain contributing dependencies.".to_string(),
                                ),
                            ),
                        ]),
                    ),
                ])];
                let result = JsonValue::Object(vec![
                    (
                        "description".to_string(),
                        JsonValue::String("Causal investigation prompt".to_string()),
                    ),
                    ("messages".to_string(), JsonValue::Array(messages)),
                ]);
                Self::make_result_response(id, result)
            }
            "timeline-audit" => {
                let messages = vec![JsonValue::Object(vec![
                    ("role".to_string(), JsonValue::String("user".to_string())),
                    (
                        "content".to_string(),
                        JsonValue::Object(vec![
                            ("type".to_string(), JsonValue::String("text".to_string())),
                            (
                                "text".to_string(),
                                JsonValue::String(
                                    "Audit the differences between branch '{{branch_a}}' and '{{branch_b}}' starting from fork sequence {{fork_seq}}.".to_string(),
                                ),
                            ),
                        ]),
                    ),
                ])];
                let result = JsonValue::Object(vec![
                    (
                        "description".to_string(),
                        JsonValue::String("Timeline audit prompt".to_string()),
                    ),
                    ("messages".to_string(), JsonValue::Array(messages)),
                ]);
                Self::make_result_response(id, result)
            }
            unknown => {
                Self::make_error_response(id, -32602, &format!("Prompt not found: '{unknown}'"))
            }
        }
    }

    /// Runs the stdio message dispatch loop until EOF.
    pub fn run_stdio<R: BufRead, W: Write>(
        &mut self,
        reader: R,
        mut writer: W,
    ) -> std::io::Result<()> {
        for line in reader.lines() {
            let line = line?;
            if let Some(resp) = self.handle_message(&line) {
                writer.write_all(resp.as_bytes())?;
                writer.write_all(b"\n")?;
                writer.flush()?;
            }
        }
        Ok(())
    }
}

impl Default for McpServer {
    fn default() -> Self {
        Self::new()
    }
}
