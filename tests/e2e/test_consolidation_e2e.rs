//! E2E: consolidation never blocks or interferes with the live path (T010).
//! spec.md User Story 1, FR constraint "must never block the live remember/recall path".

use pamet::{Interaction, Store};
use std::time::Instant;

const FACT_A: &str = "the quarterly report is due friday";
const FACT_B: &str = "remember the quarterly report deadline is this friday afternoon";

#[test]
fn consolidation_runs_independently_of_live_path() {
    let store = Store::open_in_memory().unwrap();

    // Live writes before any consolidation — near-duplicate wording
    // (verified sim ≈ 0.68) so they land as two separate active items for
    // consolidation to actually have something to cluster.
    pamet::remember(&store, &Interaction {
        source_id: "e2e".into(), text: FACT_A.into(), explicit_remember: true,
    }).unwrap();
    pamet::remember(&store, &Interaction {
        source_id: "e2e".into(), text: FACT_B.into(), explicit_remember: true,
    }).unwrap();

    let t0 = Instant::now();
    let result_before = pamet::recall(&store, "quarterly report deadline", None).unwrap();
    let recall_latency_before = t0.elapsed();
    assert!(!result_before.items.is_empty());

    // Run consolidation — a separate call, not triggered by remember/recall.
    let summary = pamet::consolidate(&store).unwrap();
    assert_eq!(summary.clusters_formed, 1);

    // Live path still works after consolidation, and more writes still work.
    pamet::remember(&store, &Interaction {
        source_id: "e2e".into(), text: "The license is MIT.".into(), explicit_remember: true,
    }).unwrap();

    let t1 = Instant::now();
    let result_after = pamet::recall(&store, "quarterly report deadline", None).unwrap();
    let recall_latency_after = t1.elapsed();
    assert!(!result_after.items.is_empty());

    let result_license = pamet::recall(&store, "what is the license", None).unwrap();
    assert!(result_license.items.iter().any(|i| i.content.contains("MIT")));

    // Not a hard perf assertion (shared test machine), just a sanity bound
    // that consolidation didn't make recall order-of-magnitude slower.
    assert!(recall_latency_after < recall_latency_before * 10 + std::time::Duration::from_millis(50));
}
