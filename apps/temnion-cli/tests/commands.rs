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
    for capability in ["durable", "server", "temql", "tnp", "tsf", "mcp", "studio"] {
        assert!(text.contains(&format!("\"{capability}\": false")));
    }
    assert!(text.contains("\"storage\": \"volatile-memory-only\""));
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
