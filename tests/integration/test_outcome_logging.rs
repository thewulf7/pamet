//! Integration test for User Story 3 (T030, T035): quickstart.md Scenario 3
//! — outcome attaches to the existing retrieval event, and the full log is
//! complete with no silent drops (spec SC-006).

use pamet::{Interaction, Store};

#[test]
fn outcome_attaches_to_existing_event_not_a_new_one() {
    let store = Store::open_in_memory().unwrap();
    pamet::remember(
        &store,
        &Interaction {
            source_id: "t".into(),
            text: "Pamet is an agent memory layer.".into(),
            explicit_remember: true,
        },
    )
    .unwrap();
    let result = pamet::recall(&store, "Pamet memory layer", Some(5)).unwrap();
    assert!(!result.items.is_empty());
    let item_id = result.items[0].id.clone();

    pamet::report_outcome(&store, result.event_id, &item_id, true).unwrap();

    let events = pamet::export_log(&store, None).unwrap();
    let outcome_events: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == "outcome_update")
        .collect();
    assert_eq!(outcome_events.len(), 1);

    let payload: serde_json::Value = serde_json::from_str(&outcome_events[0].payload_json).unwrap();
    assert_eq!(
        payload["retrieval_event_id"].as_i64().unwrap(),
        result.event_id
    );
}

#[test]
fn full_day_of_mixed_writes_and_retrievals_has_no_gaps_in_the_log() {
    let store = Store::open_in_memory().unwrap();
    let mut write_count = 0;
    let mut retrieval_count = 0;

    for text in ["Fact one.", "Fact two.", "lol", "Fact three about Rust."] {
        pamet::remember(
            &store,
            &Interaction {
                source_id: "t".into(),
                text: text.into(),
                explicit_remember: false,
            },
        )
        .unwrap();
        write_count += 1;
    }
    for q in ["Fact", "Rust", "nonexistent topic xyz"] {
        pamet::recall(&store, q, Some(5)).unwrap();
        retrieval_count += 1;
    }

    let events = pamet::export_log(&store, None).unwrap();
    let logged_writes = events
        .iter()
        .filter(|e| e.event_type == "write_decision")
        .count();
    let logged_retrievals = events
        .iter()
        .filter(|e| e.event_type == "retrieval")
        .count();

    assert_eq!(
        logged_writes, write_count,
        "every write must be logged, no drops (SC-006)"
    );
    assert_eq!(
        logged_retrievals, retrieval_count,
        "every retrieval must be logged, no drops (SC-006)"
    );
}
