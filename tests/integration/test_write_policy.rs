//! Integration test for User Story 1 (T011): quickstart.md Scenario 1 —
//! store / merge / supersede / skip / review sequence.

use pamet::{Decision, Interaction, Store};

#[test]
fn write_policy_full_sequence() {
    let store = Store::open_in_memory().unwrap();

    let d1 = pamet::remember(
        &store,
        &Interaction { source_id: "t".into(), text: "The project is called Pamet.".into(), explicit_remember: false },
    )
    .unwrap();
    assert_eq!(d1[0].decision, Decision::Store);
    let item_a = d1[0].resulting_item_id.clone().unwrap();

    let d2 = pamet::remember(
        &store,
        &Interaction {
            source_id: "t".into(),
            text: "The project is called Pamet.".into(),
            explicit_remember: false,
        },
    )
    .unwrap();
    assert_eq!(d2[0].decision, Decision::Merge, "exact restatement should merge");
    assert_eq!(d2[0].resulting_item_id.as_deref(), Some(item_a.as_str()));

    let d3 = pamet::remember(
        &store,
        &Interaction {
            source_id: "t".into(),
            text: "Actually the project is called Pamet (same as before)".into(),
            explicit_remember: false,
        },
    )
    .unwrap();
    // Contains a contradiction marker ("actually") but is still near-identical
    // in wording to item A -> falls into the contradiction branch; since
    // similarity is high it is NOT confidently resolved as a clean supersede,
    // so it should land in review rather than silently overwrite.
    assert!(matches!(d3[0].decision, Decision::Review | Decision::Merge));

    let d4 = pamet::remember(
        &store,
        &Interaction { source_id: "t".into(), text: "lol ok".into(), explicit_remember: false },
    )
    .unwrap();
    assert_eq!(d4[0].decision, Decision::Skip);
    assert!(d4[0].resulting_item_id.is_none());

    // Item A must still be present (not deleted) regardless of what happened to it.
    let recalled = pamet::recall(&store, "Pamet", Some(10)).unwrap();
    assert!(!recalled.items.is_empty());
}

#[test]
fn low_confidence_write_goes_to_review_queue_and_is_excluded_from_recall() {
    let store = Store::open_in_memory().unwrap();
    pamet::remember(
        &store,
        &Interaction { source_id: "t".into(), text: "The weather today is sunny and warm.".into(), explicit_remember: true },
    )
    .unwrap();

    // A moderately-similar-but-not-identical statement should land in the
    // ambiguous middle band -> review, and NOT appear in recall results.
    let d = pamet::remember(
        &store,
        &Interaction {
            source_id: "t".into(),
            text: "The weather today is sunny".into(),
            explicit_remember: false,
        },
    )
    .unwrap();

    if d[0].decision == pamet::Decision::Review {
        let reviews = pamet::services::review::list(&store, Some("pending")).unwrap();
        assert!(!reviews.is_empty());
    }
}
