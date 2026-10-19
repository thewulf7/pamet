//! Unit tests for write-policy rule thresholds (T038).

use pamet::services::write_policy::{evaluate, Candidate};
use pamet::{Decision, ItemType, Store};

#[test]
fn novel_text_stores() {
    let store = Store::open_in_memory().unwrap();
    let outcome = evaluate(
        &store,
        &Candidate {
            content: "a brand new fact".into(),
            item_type: ItemType::Fact,
            explicit_remember: false,
        },
    )
    .unwrap();
    assert_eq!(outcome.decision, Decision::Store);
    assert_eq!(outcome.rule_name, "novel_store");
}

#[test]
fn low_value_chatter_skips() {
    let store = Store::open_in_memory().unwrap();
    let outcome = evaluate(
        &store,
        &Candidate {
            content: "hi".into(),
            item_type: ItemType::Fact,
            explicit_remember: false,
        },
    )
    .unwrap();
    assert_eq!(outcome.decision, Decision::Skip);
}

#[test]
fn explicit_remember_overrides_low_value_skip() {
    let store = Store::open_in_memory().unwrap();
    let outcome = evaluate(
        &store,
        &Candidate {
            content: "hi".into(),
            item_type: ItemType::Fact,
            explicit_remember: true,
        },
    )
    .unwrap();
    assert_ne!(
        outcome.decision,
        Decision::Skip,
        "explicit remember must override the low-value heuristic"
    );
}
