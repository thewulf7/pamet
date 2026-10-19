//! Contract test for remember() (T010): one WriteDecision per candidate.

use pamet::{Decision, Interaction, Store};

#[test]
fn remember_produces_exactly_one_decision_per_candidate() {
    let store = Store::open_in_memory().unwrap();
    let interaction = Interaction {
        source_id: "test".into(),
        text: "The project is called Pamet.".into(),
        explicit_remember: false,
    };
    let decisions = pamet::remember(&store, &interaction).unwrap();
    assert_eq!(
        decisions.len(),
        1,
        "exactly one WriteDecision per candidate (spec SC-001)"
    );
    assert_eq!(decisions[0].decision, Decision::Store);
    assert!(decisions[0].resulting_item_id.is_some());
}

#[test]
fn skip_decision_has_no_resulting_item() {
    let store = Store::open_in_memory().unwrap();
    let interaction = Interaction {
        source_id: "test".into(),
        text: "lol".into(),
        explicit_remember: false,
    };
    let decisions = pamet::remember(&store, &interaction).unwrap();
    assert_eq!(decisions[0].decision, Decision::Skip);
    assert!(decisions[0].resulting_item_id.is_none());
}
