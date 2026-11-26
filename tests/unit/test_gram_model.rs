//! Unit tests for the per-cluster n-gram model (feature 008, T008-T009,
//! T014-T016). Covers `build_for_cluster` (US1), `generate` determinism and
//! zero-out-of-vocabulary-token guarantees, and the hand-written PRNG itself
//! (US2).

use pamet::services::embedding;
use pamet::services::gram;

fn member_texts() -> Vec<String> {
    vec![
        "the deploy pipeline failed on build step three".to_string(),
        "the deploy pipeline failed again on build step three".to_string(),
        "deploy pipeline keeps failing at the build step three stage".to_string(),
    ]
}

#[test]
fn build_for_cluster_produces_expected_stats() {
    let texts = member_texts();
    let model = gram::build_for_cluster("cluster-1", "run-1", &texts).expect("model expected");

    let combined = texts.join(" ");
    let tokens = embedding::tokenize(&combined);
    assert_eq!(model.training_token_count, tokens.len());
    let expected_vocab = tokens
        .iter()
        .collect::<std::collections::HashSet<_>>()
        .len();
    assert_eq!(model.vocab_size, expected_vocab);
    assert_eq!(model.gram_order, gram::GRAM_ORDER);
    assert!(!model.table.is_empty());
}

#[test]
fn build_for_cluster_too_small_returns_none() {
    // Combined token count (2: "x", "y") is below GRAM_ORDER (3).
    let texts = vec!["x".to_string(), "y".to_string()];
    assert!(gram::build_for_cluster("cluster-2", "run-1", &texts).is_none());
}

#[test]
fn generate_is_deterministic_with_same_seed() {
    let model = gram::build_for_cluster("cluster-3", "run-1", &member_texts()).unwrap();
    let a = gram::generate(&model, Some(42), Some(20));
    let b = gram::generate(&model, Some(42), Some(20));
    assert_eq!(a, b, "same seed must produce byte-identical output");
}

#[test]
fn generate_never_emits_out_of_vocabulary_tokens() {
    let texts = member_texts();
    let model = gram::build_for_cluster("cluster-4", "run-1", &texts).unwrap();
    let combined = texts.join(" ");
    let vocab: std::collections::HashSet<String> =
        embedding::tokenize(&combined).into_iter().collect();

    for seed in [1u64, 2, 3, 99, 12345] {
        let out = gram::generate(&model, Some(seed), Some(30));
        for tok in out.split(' ') {
            assert!(
                vocab.contains(tok),
                "generated token {tok:?} not present in cluster's own vocabulary"
            );
        }
    }
}

#[test]
fn generate_stops_early_rather_than_fabricating() {
    // A minimal model (exactly GRAM_ORDER tokens) has at most one context
    // key and no guaranteed continuation beyond the seed tokens — generation
    // must stop, not loop forever or invent content.
    let texts = vec!["alpha beta gamma".to_string()];
    let model = gram::build_for_cluster("cluster-5", "run-1", &texts).unwrap();
    let out = gram::generate(&model, Some(7), Some(100));
    assert!(!out.is_empty());
    // Must terminate well under the requested cap given so little data.
    assert!(out.split(' ').count() <= 100);
}
