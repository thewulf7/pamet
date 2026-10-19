//! Contract test for report_outcome() / export_log() (T029).

use pamet::{Interaction, Store};

#[test]
fn report_outcome_rejects_item_not_returned_by_event() {
    let store = Store::open_in_memory().unwrap();
    pamet::remember(
        &store,
        &Interaction {
            source_id: "t".into(),
            text: "Pamet remembers things.".into(),
            explicit_remember: true,
        },
    )
    .unwrap();
    let result = pamet::recall(&store, "Pamet", Some(5)).unwrap();

    let err = pamet::report_outcome(&store, result.event_id, "not-a-real-item-id", true);
    assert!(err.is_err(), "unknown item-for-event must be rejected");
}

#[test]
fn export_log_is_ordered_and_complete() {
    let store = Store::open_in_memory().unwrap();
    pamet::remember(
        &store,
        &Interaction {
            source_id: "t".into(),
            text: "Pamet logs everything.".into(),
            explicit_remember: true,
        },
    )
    .unwrap();
    pamet::recall(&store, "Pamet", Some(5)).unwrap();

    let events = pamet::export_log(&store, None).unwrap();
    assert!(
        events.len() >= 2,
        "both the write_decision and retrieval events must be present"
    );
    let ids: Vec<i64> = events.iter().map(|e| e.id).collect();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted, "export_log must be in replay order");
}
