//! Contract test for recall() (T020): budget-bounded, empty result != error.

use pamet::{Interaction, Store};

#[test]
fn recall_respects_budget_and_empty_is_not_an_error() {
    let store = Store::open_in_memory().unwrap();
    for text in ["Pamet is a memory layer.", "Pamet uses Rust.", "Pamet uses SQLite."] {
        pamet::remember(
            &store,
            &Interaction { source_id: "t".into(), text: text.into(), explicit_remember: true },
        )
        .unwrap();
    }

    let result = pamet::recall(&store, "Pamet", Some(2)).unwrap();
    assert!(result.items.len() <= 2, "result must respect the budget (FR-007)");

    let empty = pamet::recall(&store, "zzz_totally_unrelated_zzz", Some(5)).unwrap();
    assert!(empty.items.is_empty(), "no match must return empty, not an error (FR-008)");
}
