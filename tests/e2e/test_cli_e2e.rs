//! End-to-end test: drives the real, compiled `pamet` binary as a separate
//! process through all three quickstart.md scenarios, exactly as a human
//! operator would from a shell. This is deliberately process-level (not a
//! library call) so it also catches CLI wiring bugs (arg parsing, stdout
//! formatting, exit codes) that the library-level contract/integration
//! tests can't see.

use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_pamet")
}

fn run(db: &str, args: &[&str]) -> (bool, String, String) {
    let output = Command::new(bin())
        .arg("--db")
        .arg(db)
        .args(args)
        .output()
        .expect("failed to spawn pamet");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

#[test]
fn full_quickstart_scenario_1_write_decisions() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let db = tmp.path().to_str().unwrap();
    std::fs::remove_file(db).ok();

    let (ok, out, _) = run(db, &["remember", "--text", "The project is called Pamet."]);
    assert!(ok);
    assert!(out.starts_with("store"), "first fact should be stored: {out}");

    let (ok, out, _) = run(db, &["remember", "--text", "lol ok"]);
    assert!(ok);
    assert!(out.starts_with("skip"), "low-value chatter should be skipped: {out}");

    let (ok, review_out, _) = run(db, &["review", "list"]);
    assert!(ok);
    // No ambiguous/contradicting candidates were sent in this scenario, so
    // the review queue should still be empty.
    assert!(review_out.trim().is_empty(), "review queue should be empty: {review_out}");

    std::fs::remove_file(db).ok();
}

#[test]
fn full_quickstart_scenario_2_hybrid_retrieval_with_budget() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let db = tmp.path().to_str().unwrap();
    std::fs::remove_file(db).ok();

    run(db, &["remember", "--text", "The project is called Pamet."]);
    run(db, &["remember", "--text", "Bananas are a good source of potassium."]);

    let (ok, out, _) = run(db, &["recall", "--query", "what is the project called", "--budget", "2"]);
    assert!(ok, "recall must exit 0 even for an empty result");
    assert!(out.contains("Pamet"), "relevant query should surface the Pamet fact: {out}");

    let (ok, out, _) = run(db, &["recall", "--query", "completely unrelated query about weather", "--budget", "2"]);
    assert!(ok, "recall must exit 0 for a no-match query (FR-008)");
    assert_eq!(out.trim(), "no relevant memory found");

    std::fs::remove_file(db).ok();
}

#[test]
fn full_quickstart_scenario_3_outcome_logging_closes_the_loop() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let db = tmp.path().to_str().unwrap();
    std::fs::remove_file(db).ok();

    let (_, remember_out, _) = run(db, &["remember", "--text", "Pamet is an agent memory layer."]);
    let item_id = remember_out
        .split("item=")
        .nth(1)
        .expect("remember output must include item=<id>")
        .trim()
        .to_string();

    let (ok, _, recall_stderr) = run(db, &["recall", "--query", "Pamet memory layer", "--budget", "5"]);
    assert!(ok);
    let event_id = recall_stderr
        .split("event_id=")
        .nth(1)
        .expect("recall stderr must include event_id=<id>")
        .trim()
        .to_string();

    let (ok, outcome_out, _) =
        run(db, &["outcome", "--event-id", &event_id, "--item-id", &item_id, "--used", "true"]);
    assert!(ok, "outcome command must succeed");
    assert_eq!(outcome_out.trim(), "ok");

    let (ok, export_out, _) = run(db, &["export-log"]);
    assert!(ok);
    assert!(
        export_out.contains("\"event_type\":\"outcome_update\""),
        "export-log must contain the outcome_update event: {export_out}"
    );
    assert!(
        export_out.contains(&item_id),
        "export-log must reference the item that received the outcome: {export_out}"
    );

    std::fs::remove_file(db).ok();
}

#[test]
fn forget_removes_item_from_recall_end_to_end() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let db = tmp.path().to_str().unwrap();
    std::fs::remove_file(db).ok();

    let (_, remember_out, _) = run(db, &["remember", "--text", "A fact to be forgotten."]);
    let item_id = remember_out.split("item=").nth(1).unwrap().trim().to_string();

    let (ok, recall_out, _) = run(db, &["recall", "--query", "forgotten", "--budget", "5"]);
    assert!(ok);
    assert!(recall_out.contains("forgotten"));

    let (ok, forget_out, _) = run(db, &["forget", "--item-id", &item_id]);
    assert!(ok);
    assert_eq!(forget_out.trim(), "ok");

    let (ok, recall_after, _) = run(db, &["recall", "--query", "forgotten", "--budget", "5"]);
    assert!(ok);
    assert_eq!(recall_after.trim(), "no relevant memory found");

    std::fs::remove_file(db).ok();
}
