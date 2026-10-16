//! Integration test for User Story 2 (T021): quickstart.md Scenario 2.

use pamet::{Interaction, Store};

#[test]
fn hybrid_retrieval_ranks_relevant_item_and_handles_no_match() {
    let store = Store::open_in_memory().unwrap();
    pamet::remember(
        &store,
        &Interaction { source_id: "t".into(), text: "The project is called Pamet.".into(), explicit_remember: true },
    )
    .unwrap();
    pamet::remember(
        &store,
        &Interaction { source_id: "t".into(), text: "Bananas are a good source of potassium.".into(), explicit_remember: true },
    )
    .unwrap();

    let relevant = pamet::recall(&store, "what is the project called", Some(2)).unwrap();
    assert!(!relevant.items.is_empty(), "relevant query must surface at least one item (SC-002)");
    assert!(relevant.items.iter().any(|i| i.content.contains("Pamet")));

    let unrelated = pamet::recall(&store, "completely unrelated query about weather", Some(2)).unwrap();
    assert!(unrelated.items.is_empty(), "no-match query must return empty (SC-003/FR-008)");
}
