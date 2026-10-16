//! Unit/integration test for rebuild_from_events() (T041): Constitution
//! Principle I — the event log alone must be sufficient to reconstruct
//! derived state.

use pamet::{Interaction, Store};

#[test]
fn rebuilt_store_matches_live_store_after_a_mixed_sequence() {
    let live = Store::open_in_memory().unwrap();

    let d1 = pamet::remember(
        &live,
        &Interaction { source_id: "t".into(), text: "Pamet is a memory layer for agents".into(), explicit_remember: true },
    )
    .unwrap();
    let item_a = d1[0].resulting_item_id.clone().unwrap();

    pamet::remember(
        &live,
        &Interaction { source_id: "t".into(), text: "Bananas are yellow".into(), explicit_remember: true },
    )
    .unwrap();

    let retrieval = pamet::recall(&live, "memory layer", Some(5)).unwrap();
    assert!(!retrieval.items.is_empty());
    pamet::report_outcome(&live, retrieval.event_id, &item_a, true).unwrap();

    pamet::remember(
        &live,
        &Interaction { source_id: "t".into(), text: "lol ok".into(), explicit_remember: false },
    )
    .unwrap();

    let rebuilt = pamet::rebuild_from_events(&live).unwrap();

    // Same events, in the same order, with the same ids.
    let live_events = pamet::export_log(&live, None).unwrap();
    let rebuilt_events = pamet::export_log(&rebuilt, None).unwrap();
    assert_eq!(live_events.len(), rebuilt_events.len());
    for (a, b) in live_events.iter().zip(rebuilt_events.iter()) {
        assert_eq!(a.id, b.id);
        assert_eq!(a.event_type, b.event_type);
        assert_eq!(a.payload_json, b.payload_json);
    }

    // Same memory_items (ids + content + tombstone/supersede state).
    let live_items: Vec<(String, String)> = query_all_items(&live);
    let rebuilt_items: Vec<(String, String)> = query_all_items(&rebuilt);
    assert_eq!(live_items, rebuilt_items);

    // Same write_decisions count and decisions.
    let live_wd: Vec<String> = query_decisions(&live);
    let rebuilt_wd: Vec<String> = query_decisions(&rebuilt);
    assert_eq!(live_wd, rebuilt_wd);

    // Same outcome_records.
    let live_outcomes: i64 = live.conn.query_row("SELECT COUNT(*) FROM outcome_records", [], |r| r.get(0)).unwrap();
    let rebuilt_outcomes: i64 = rebuilt.conn.query_row("SELECT COUNT(*) FROM outcome_records", [], |r| r.get(0)).unwrap();
    assert_eq!(live_outcomes, rebuilt_outcomes);
    assert_eq!(live_outcomes, 1);
}

fn query_all_items(store: &Store) -> Vec<(String, String)> {
    let mut stmt = store.conn.prepare("SELECT id, content FROM memory_items ORDER BY id").unwrap();
    stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect()
}

fn query_decisions(store: &Store) -> Vec<String> {
    let mut stmt = store.conn.prepare("SELECT decision FROM write_decisions ORDER BY id").unwrap();
    stmt.query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect()
}
