//! Integration test for tombstone()/forget (T044): SC-005 / FR-014.

use pamet::{Interaction, Store};

#[test]
fn tombstoned_item_disappears_from_recall_but_remains_inspectable() {
    let store = Store::open_in_memory().unwrap();
    let decisions = pamet::remember(
        &store,
        &Interaction { source_id: "t".into(), text: "Pamet is written in Rust".into(), explicit_remember: true },
    )
    .unwrap();
    let item_id = decisions[0].resulting_item_id.clone().unwrap();

    // Before tombstoning, it is retrievable.
    let before = pamet::recall(&store, "Pamet Rust", Some(5)).unwrap();
    assert!(before.items.iter().any(|i| i.id == item_id));

    pamet::tombstone(&store, &item_id).unwrap();

    // After tombstoning, it must not surface via hybrid retrieval (vector OR
    // keyword) — FR-014 / data-model.md validation rule.
    let after = pamet::recall(&store, "Pamet Rust", Some(5)).unwrap();
    assert!(
        !after.items.iter().any(|i| i.id == item_id),
        "tombstoned item must not appear in recall results"
    );

    // But its history remains inspectable (SC-005): a tombstone event exists
    // referencing it, and the row itself is not hard-deleted.
    let events = pamet::export_log(&store, None).unwrap();
    let tombstone_events: Vec<_> = events.iter().filter(|e| e.event_type == "tombstone").collect();
    assert_eq!(tombstone_events.len(), 1);
    let payload: serde_json::Value = serde_json::from_str(&tombstone_events[0].payload_json).unwrap();
    assert_eq!(payload["item_id"].as_str().unwrap(), item_id);

    let still_there: i64 = store
        .conn
        .query_row(
            "SELECT COUNT(*) FROM memory_items WHERE id = ?1 AND tombstoned_at IS NOT NULL",
            rusqlite::params![item_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(still_there, 1, "tombstoned row must remain in memory_items, not be deleted");
}

#[test]
fn tombstoning_unknown_item_is_an_error_not_a_silent_noop() {
    let store = Store::open_in_memory().unwrap();
    let err = pamet::tombstone(&store, "does-not-exist");
    assert!(err.is_err());
}
