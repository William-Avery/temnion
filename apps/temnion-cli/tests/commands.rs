// SPDX-License-Identifier: AGPL-3.0-only
use std::process::Command;

fn tem(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_tem"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn demo_runs_real_state_and_bounded_history() {
    let result = tem(&["demo"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let text = String::from_utf8(result.stdout).unwrap();
    assert!(text.contains("Recorded events: 3"));
    assert!(text.contains("Known-as-of 15: 2 events"));
    assert!(text.contains("Removed handle rejected"));
    assert!(
        !text.contains("sequence=2"),
        "late evidence must not leak into known-as-of output"
    );
}

#[test]
fn capabilities_do_not_advertise_unimplemented_features() {
    let result = tem(&["describe"]);
    assert!(result.status.success());
    let text = String::from_utf8(result.stdout).unwrap();
    for capability in ["server", "studio"] {
        assert!(text.contains(&format!("\"{capability}\": false")));
    }
    assert!(text.contains("\"temql\": true"));
    assert!(text.contains("\"sql\": true"));
    assert!(text.contains("\"tnp\": true"));
    assert!(text.contains("\"mcp\": true"));
    assert!(text.contains("\"durable\": true"));
    assert!(text.contains("\"tsf\": true"));
    assert!(text.contains("\"storage\": \"volatile-memory-and-os-synced-source-log\""));
    assert!(text.contains("\"scalar-schemas\""));
    assert!(text.contains("\"hierarchical-summaries\""));
    assert!(text.contains("\"nd-layouts\""));
    assert!(text.contains("\"alternate-projections\""));
    assert!(text.contains("\"virtual-shards\""));
    assert!(text.contains("\"background-dag\""));
    assert!(text.contains("\"storage-hierarchy\""));
    assert!(text.contains("\"query-ir\""));
    assert!(text.contains("\"compact-tem\""));
    assert!(text.contains("\"sql\""));
    assert!(text.contains("\"tnp\""));
    assert!(text.contains("\"local-ipc\""));
    assert!(text.contains("\"arrow-columnar\""));
    assert!(text.contains("\"c-abi\""));
    assert!(text.contains("\"flight\""));
    assert!(text.contains("\"mcp\""));
}

#[test]
fn invalid_commands_and_extra_arguments_fail() {
    for args in [&["query"][..], &["demo", "ignored"][..]] {
        let result = tem(args);
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
        assert!(!result.stderr.is_empty());
    }
}

#[test]
fn help_and_version_succeed() {
    for args in [&[][..], &["help"][..], &["--help"][..], &["version"][..]] {
        let result = tem(args);
        assert!(result.status.success());
        assert!(!result.stdout.is_empty());
    }
}

struct DatabaseDirectory(std::path::PathBuf);

impl DatabaseDirectory {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "temnion-cli-test-{}-{time}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn command(&self, name: &str, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_tem"))
            .arg(name)
            .arg(&self.0)
            .args(args)
            .output()
            .unwrap()
    }
}

impl Drop for DatabaseDirectory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove the test-created database directory");
    }
}

#[test]
fn durable_cli_roundtrip_across_independent_processes() {
    let directory = DatabaseDirectory::new();
    let created = directory.command("init", &[]);
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let appended = directory.command("append", &["0:1:0", "1", "1:5", "2:10", "2a00"]);
    assert!(
        appended.status.success(),
        "{}",
        String::from_utf8_lossy(&appended.stderr)
    );
    assert!(String::from_utf8_lossy(&appended.stdout).contains("Durable sequence=0"));
    let history = directory.command("history", &[]);
    assert!(history.status.success());
    assert!(String::from_utf8_lossy(&history.stdout).contains("preview=2a00"));
    let inspection = directory.command("inspect", &[]);
    assert!(inspection.status.success());
    assert!(String::from_utf8_lossy(&inspection.stdout).contains("records=1"));
    let sealed = directory.command("seal", &[]);
    assert!(
        sealed.status.success(),
        "{}",
        String::from_utf8_lossy(&sealed.stderr)
    );
    assert!(String::from_utf8_lossy(&sealed.stdout).contains("segments_created=1"));
    let segment = directory
        .0
        .join("segments")
        .join(format!("{:020}-{:020}.tsf", 0, 0));
    let verified = Command::new(env!("CARGO_BIN_EXE_tem"))
        .arg("verify-segment")
        .arg(segment)
        .output()
        .unwrap();
    assert!(verified.status.success());
    assert!(String::from_utf8_lossy(&verified.stdout).contains("Valid TSF: records=1"));

    let summary = directory
        .0
        .join("segments")
        .join(format!("{:020}-{:020}.tsm", 0, 0));
    let inspected = Command::new(env!("CARGO_BIN_EXE_tem"))
        .arg("inspect-summary")
        .arg(summary)
        .output()
        .unwrap();
    assert!(inspected.status.success());
    let inspected_stdout = String::from_utf8_lossy(&inspected.stdout);
    assert!(inspected_stdout.contains("Valid TSM: records=1 blocks=1"));
    assert!(inspected_stdout.contains("Sequence: 0..=0"));
    assert!(inspected_stdout.contains("Valid time: clock=1 5..=5"));
    assert!(inspected_stdout.contains("Known time: clock=2 10..=10"));
}

#[test]
fn malformed_persistent_cli_inputs_do_not_admit_events() {
    let directory = DatabaseDirectory::new();
    assert!(directory.command("init", &[]).status.success());
    for args in [
        &["0:1", "1", "1:5", "2:10", "aa"][..],
        &["0:1:0", "1", "1:5", "2:10", "not-hex"][..],
        &["0:1:0", "1", "1:5", "2:10", "a"][..],
        &["0:1:0", "1", "1:-1", "2:10", "aa"][..],
    ] {
        let output = directory.command("append", args);
        assert!(!output.status.success(), "{args:?}");
        assert!(output.stdout.is_empty());
    }
    assert!(!directory.command("history", &["0"]).status.success());
    let inspection = directory.command("inspect", &[]);
    assert!(String::from_utf8_lossy(&inspection.stdout).contains("records=0"));
}

#[test]
fn recovery_is_explicit_and_does_not_hide_discarded_bytes() {
    use std::io::Write;
    let directory = DatabaseDirectory::new();
    assert!(directory.command("init", &[]).status.success());
    assert!(
        directory
            .command("append", &["0:1:0", "1", "1:5", "2:10", "aa"])
            .status
            .success()
    );
    std::fs::OpenOptions::new()
        .append(true)
        .open(directory.0.join("events.wal"))
        .unwrap()
        .write_all(b"TNW")
        .unwrap();
    assert!(!directory.command("inspect", &[]).status.success());
    let recovered = directory.command("recover", &[]);
    assert!(
        recovered.status.success(),
        "{}",
        String::from_utf8_lossy(&recovered.stderr)
    );
    assert!(
        String::from_utf8_lossy(&recovered.stdout).contains("discarded_incomplete_tail_bytes=3")
    );
    assert!(directory.command("inspect", &[]).status.success());
}

#[test]
fn evaluate_codecs_command_runs_and_reports_lossless_ratios() {
    let result = tem(&["evaluate-codecs"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let text = String::from_utf8(result.stdout).unwrap();
    assert!(text.contains("Lossless Codec Evaluation:"));
    assert!(text.contains("Pattern 1"));
    assert!(text.contains("Pattern 2"));
    assert!(text.contains("Pattern 3"));
    assert!(text.contains("Pattern 4"));
}

#[test]
fn checkpoint_and_reconstruct_cli_roundtrip() {
    let directory = DatabaseDirectory::new();
    assert!(directory.command("init", &[]).status.success());
    assert!(
        directory
            .command("append", &["0:1:0", "1", "1:100", "2:200", "deadbeef"])
            .status
            .success()
    );
    assert!(
        directory
            .command("append", &["0:2:0", "1", "1:101", "2:201", "cafebabe"])
            .status
            .success()
    );

    // Reconstruct at sequence 0
    let recon0 = directory.command("reconstruct", &["0"]);
    assert!(
        recon0.status.success(),
        "{}",
        String::from_utf8_lossy(&recon0.stderr)
    );
    let text0 = String::from_utf8(recon0.stdout).unwrap();
    assert!(text0.contains("Reconstructed sequence=0 entities=1"));
    assert!(text0.contains("entity=0:1:0 schema=1 payload_bytes=4"));

    // Take checkpoint at latest sequence (1)
    let cp = directory.command("checkpoint", &[]);
    assert!(
        cp.status.success(),
        "{}",
        String::from_utf8_lossy(&cp.stderr)
    );
    let cp_text = String::from_utf8(cp.stdout).unwrap();
    assert!(cp_text.contains("Checkpoint sequence=1 entities=2"));

    // Reconstruct at sequence 1 (should utilize checkpoint)
    let recon1 = directory.command("reconstruct", &["1"]);
    assert!(
        recon1.status.success(),
        "{}",
        String::from_utf8_lossy(&recon1.stderr)
    );
    let text1 = String::from_utf8(recon1.stdout).unwrap();
    assert!(text1.contains("Reconstructed sequence=1 entities=2"));
    assert!(text1.contains("entity=0:1:0 schema=1 payload_bytes=4"));
    assert!(text1.contains("entity=0:2:0 schema=1 payload_bytes=4"));
}

#[test]
fn branch_lifecycle_and_listing_cli_roundtrip() {
    let directory = DatabaseDirectory::new();
    assert!(directory.command("init", &[]).status.success());
    assert!(
        directory
            .command("append", &["0:1:0", "1", "1:100", "2:200", "aabbccdd"])
            .status
            .success()
    );

    // Initial branch-list should show root branch 0
    let list0 = directory.command("branch-list", &[]);
    assert!(
        list0.status.success(),
        "{}",
        String::from_utf8_lossy(&list0.stderr)
    );
    let list0_text = String::from_utf8(list0.stdout).unwrap();
    assert!(list0_text.contains("id=0 name=\"main\" root=true lifecycle=active"));

    // Fork a new branch "experiment" from parent 0 at sequence 0
    let create = directory.command("branch-create", &["experiment", "0", "0"]);
    assert!(
        create.status.success(),
        "{}",
        String::from_utf8_lossy(&create.stderr)
    );
    let create_text = String::from_utf8(create.stdout).unwrap();
    assert!(
        create_text.contains("Branch created id=1 name=\"experiment\" parent=0 fork_sequence=0")
    );

    // List branches again, verifying both root and the candidate fork exist
    let list1 = directory.command("branch-list", &[]);
    assert!(list1.status.success());
    let list1_text = String::from_utf8(list1.stdout).unwrap();
    assert!(list1_text.contains("id=0 name=\"main\" root=true lifecycle=active"));
    assert!(
        list1_text
            .contains("id=1 name=\"experiment\" parent=0 fork_sequence=0 lifecycle=candidate")
    );
}

#[test]
fn causal_trace_cli_roundtrip() {
    let directory = DatabaseDirectory::new();
    assert!(directory.command("init", &[]).status.success());

    // Append root event e0 (source=1, epoch=1, seq=0) with no causes
    assert!(
        directory
            .command("append", &["0:1:0", "1", "1:100", "2:200", "1111"])
            .status
            .success()
    );

    // Append e1 caused by e0 (1:1:0)
    assert!(
        directory
            .command("append", &["0:1:0", "1", "1:101", "2:201", "2222", "1:1:0"])
            .status
            .success()
    );

    // Append e2 caused by e1 (1:1:1)
    assert!(
        directory
            .command("append", &["0:1:0", "1", "1:102", "2:202", "3333", "1:1:1"])
            .status
            .success()
    );

    // Trace event 1 (e1): should have upstream cause e0 (depth=1) and downstream effect e2 (depth=1)
    let trace1 = directory.command("causal-trace", &["1"]);
    assert!(
        trace1.status.success(),
        "{}",
        String::from_utf8_lossy(&trace1.stderr)
    );
    let trace1_text = String::from_utf8(trace1.stdout).unwrap();
    assert!(trace1_text.contains("Causal Trace for Event 1:1:1:"));
    assert!(trace1_text.contains("Upstream Causes (total=1):"));
    assert!(trace1_text.contains("depth=1 event=1:1:0"));
    assert!(trace1_text.contains("Downstream Effects (total=1):"));
    assert!(trace1_text.contains("depth=1 event=1:1:2"));

    // Trace event 2 (e2): should have upstream causes e1 (depth 1) and e0 (depth 2)
    let trace2 = directory.command("causal-trace", &["2", "5"]);
    assert!(
        trace2.status.success(),
        "{}",
        String::from_utf8_lossy(&trace2.stderr)
    );
    let trace2_text = String::from_utf8(trace2.stdout).unwrap();
    assert!(trace2_text.contains("Causal Trace for Event 1:1:2:"));
    assert!(trace2_text.contains("Upstream Causes (total=2):"));
    assert!(trace2_text.contains("depth=1 event=1:1:1"));
    assert!(trace2_text.contains("depth=2 event=1:1:0"));
    assert!(trace2_text.contains("Downstream Effects (total=0):"));
}

#[test]
fn explain_and_query_cli_roundtrip() {
    let directory = DatabaseDirectory::new();

    // Test explain on TemQL
    let temql_explain = tem(&[
        "explain",
        "FROM temnion\nENTITY 0:1:0\nTIME valid 10..50\nLIMIT 5",
    ]);
    assert!(
        temql_explain.status.success(),
        "{}",
        String::from_utf8_lossy(&temql_explain.stderr)
    );
    let explain_text = String::from_utf8(temql_explain.stdout).unwrap();
    assert!(explain_text.contains("StorageScan"));
    assert!(explain_text.contains("entity=0:1:0"));
    assert!(explain_text.contains("valid_range=10..50"));
    assert!(explain_text.contains("limit=5"));

    // Test explain on compact tn:
    let compact_explain = tem(&["explain", "tn:#0:1:0@v10..50!5"]);
    assert!(compact_explain.status.success());
    let compact_text = String::from_utf8(compact_explain.stdout).unwrap();
    assert_eq!(explain_text, compact_text);

    // Test explain on SQL
    let sql_explain = tem(&[
        "explain",
        "SELECT * FROM temnion WHERE entity = '0:1:0' AND valid_time >= 10 AND valid_time < 50 LIMIT 5",
    ]);
    assert!(sql_explain.status.success());
    let sql_text = String::from_utf8(sql_explain.stdout).unwrap();
    assert_eq!(explain_text, sql_text);

    // Initialize database and append test events
    assert!(directory.command("init", &[]).status.success());
    assert!(
        directory
            .command("append", &["0:1:0", "1", "1:15", "2:15", "aaaa"])
            .status
            .success()
    );
    assert!(
        directory
            .command("append", &["0:1:0", "1", "1:25", "2:25", "bbbb"])
            .status
            .success()
    );
    assert!(
        directory
            .command("append", &["0:2:0", "1", "1:35", "2:35", "cccc"])
            .status
            .success()
    );

    // Query entity 0:1:0 using compact syntax
    let dir_str = directory.0.to_str().unwrap();
    let query_res = tem(&["query", dir_str, "tn:#0:1:0@v10..30!10"]);
    assert!(
        query_res.status.success(),
        "{}",
        String::from_utf8_lossy(&query_res.stderr)
    );
    let query_text = String::from_utf8(query_res.stdout).unwrap();
    assert!(query_text.contains("Query results (rows=2"));
    assert!(query_text.contains("entity=0:1:0 valid=15"));
    assert!(query_text.contains("entity=0:1:0 valid=25"));
    assert!(!query_text.contains("entity=0:2:0"));

    // Query entity 0:1:0 using SQL syntax
    let sql_query_res = tem(&[
        "query",
        dir_str,
        "SELECT * FROM temnion WHERE entity = '0:1:0' AND valid_time >= 10 AND valid_time < 30 LIMIT 10",
    ]);
    assert!(sql_query_res.status.success());
    let sql_query_text = String::from_utf8(sql_query_res.stdout).unwrap();
    assert_eq!(query_text, sql_query_text);
}
