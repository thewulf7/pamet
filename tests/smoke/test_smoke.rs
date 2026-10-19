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

#[test]
fn postgres_url_without_feature_fails_clearly_not_silently() {
    // FR-007 / contracts/cli-contract.md "Feature gating": a postgres://
    // URL on a build without `postgres-backend` must fail fast with a
    // clear "not compiled in" error, never silently fall back to SQLite
    // or no-op. This test only means anything when the smoke-test binary
    // itself was built WITHOUT the postgres-backend feature (the default
    // `cargo test` invocation, with no --features flag) — it's a no-op
    // assertion-skip rather than a false pass if that feature happens to
    // be on, since the behavior it's checking doesn't apply then.
    if cfg!(feature = "postgres-backend") {
        eprintln!("skipping: this binary was built WITH postgres-backend");
        return;
    }

    let recall = Command::new(bin())
        .args([
            "--db",
            "postgres://postgres:pamet@localhost:5433/pamet_test",
            "recall",
            "--query",
            "anything",
        ])
        .output()
        .expect("failed to run pamet recall");

    assert!(
        !recall.status.success(),
        "a postgres:// URL on a build without postgres-backend must exit non-zero, not succeed"
    );
    let stderr = String::from_utf8_lossy(&recall.stderr);
    assert!(
        stderr.contains("postgres-backend") || stderr.contains("not compiled"),
        "expected a clear 'not compiled in' error, got: {stderr}"
    );
}
