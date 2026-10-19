//! Smoke test: fast, minimal process-level check that the real `pamet`
//! binary starts and a basic remember+recall roundtrip works. Meant to catch
//! fundamental breakage (binary doesn't build/run, DB can't be created)
//! before the slower, more detailed e2e suite runs.

use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_pamet")
}

#[test]
fn binary_starts_and_prints_help() {
    let output = Command::new(bin())
        .arg("--help")
        .output()
        .expect("failed to run pamet --help");
    assert!(output.status.success(), "pamet --help must exit 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Usage"),
        "help output should mention usage: {stdout}"
    );
}

#[test]
fn basic_remember_then_recall_roundtrip() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let db_path = tmp.path().to_str().unwrap();
    std::fs::remove_file(db_path).ok(); // let pamet create it fresh

    let remember = Command::new(bin())
        .args(["--db", db_path, "remember", "--text", "Smoke test fact."])
        .output()
        .expect("failed to run pamet remember");
    assert!(remember.status.success(), "remember must exit 0");
    let remember_stdout = String::from_utf8_lossy(&remember.stdout);
    assert!(
        remember_stdout.starts_with("store"),
        "expected a store decision: {remember_stdout}"
    );

    let recall = Command::new(bin())
        .args([
            "--db",
            db_path,
            "recall",
            "--query",
            "Smoke test",
            "--budget",
            "3",
        ])
        .output()
        .expect("failed to run pamet recall");
    assert!(recall.status.success(), "recall must exit 0");
    let recall_stdout = String::from_utf8_lossy(&recall.stdout);
    assert!(
        recall_stdout.contains("Smoke test fact"),
        "expected the just-remembered fact back: {recall_stdout}"
    );

    std::fs::remove_file(db_path).ok();
}
