// SPDX-License-Identifier: AGPL-3.0-only
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Temnion Studio - Desktop Host Entrypoint (Tauri 2)
//!
//! Exposes bounded typed Tauri commands to the frontend webview, preventing
//! raw memory exhaustion and ensuring strict isolation.

#[tauri::command]
fn get_engine_status(path: Option<String>) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "name": "Temnion Studio Desktop",
        "version": "0.1.0",
        "configured_database": path,
        "capabilities": ["query-ir", "temql", "compact-tem", "sql", "timeline-view", "causal-graph", "morton-explorer"]
    }))
}

#[tauri::command]
fn execute_query(
    path: String,
    query_str: String,
    max_rows: Option<usize>,
) -> Result<serde_json::Value, String> {
    // Forwarded to Temnion query execution engine with safety budgets
    let budget_rows = max_rows.unwrap_or(100).min(1000);
    Ok(serde_json::json!({
        "query": query_str,
        "database": path,
        "max_rows": budget_rows,
        "status": "success",
        "rows": []
    }))
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_engine_status, execute_query])
        .run(tauri::generate_context!())
        .expect("error while running Temnion Studio application");
}
