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
    for capability in ["server", "temql", "tnp", "mcp", "studio"] {
        assert!(text.contains(&format!("\"{capability}\": false")));
    }
    assert!(text.contains("\"durable\": true"));
    assert!(text.contains("\"tsf\": true"));
    assert!(text.contains("\"storage\": \"volatile-memory-and-os-synced-source-log\""));
    assert!(text.contains("\"scalar-schemas\""));
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
