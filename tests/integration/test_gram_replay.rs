//! Integration tests for gram-model event-log replay (feature 008, T020-T021,
//! US3). Proves `cluster_gram_models` rows are byte-for-byte reconstructable
//! by replaying the event log alone (FR-008/SC-004, Constitution Principle
//! I) — against both a synthetic fixture and a real consolidated store.

use pamet::services::rebuild;
use pamet::{Interaction, Store};
use std::sync::Mutex;

/// Shares the DB_LOCK pattern `tests/integration/test_write_policy_scale.rs`
/// established (feature 005) — serializes Postgres-backed tests in this
/// file against races on the one shared database.
static DB_LOCK: Mutex<()> = Mutex::new(());
fn lock_db() -> std::sync::MutexGuard<'static, ()> {
    DB_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn open_target() -> (Store, bool) {
    match std::env::var("PAMET_TEST_POSTGRES_URL") {
        Ok(url) => {
            #[cfg(feature = "postgres-backend")]
            {
                truncate(&url);
                (Store::open(&url).expect("open postgres target"), true)
            }
            #[cfg(not(feature = "postgres-backend"))]
            {
                let _ = url;
                (Store::open_in_memory().expect("open in-memory"), false)
            }
        }
        Err(_) => (Store::open_in_memory().expect("open in-memory"), false),
    }
}

#[cfg(feature = "postgres-backend")]
fn truncate(url: &str) {
    let _ = Store::open(url);
    let mut client = postgres::Client::connect(url, postgres::NoTls).expect("truncate connection");
    let _ = client.batch_execute(
        "TRUNCATE memory_items, events, write_decisions, review_queue, retrieval_events, \
         consolidation_runs, cluster_members, cluster_gram_models, outcome_records, \
         eval_results, baseline_reports RESTART IDENTITY CASCADE",
    );
}

fn remember(store: &Store, text: &str) {
    pamet::remember(
        store,
        &Interaction {
            source_id: "gram_replay_test".into(),
            text: text.into(),
            explicit_remember: true,
        },
    )
    .unwrap();
}

fn gram_model_rows(store: &Store) -> Vec<(String, i64, i64, i64, String)> {
    match &store.backend {
        pamet::storage::Backend::Sqlite(conn) => {
            let mut stmt = conn
                .prepare(
                    "SELECT cluster_id, gram_order, vocab_size, training_token_count, table_json \
                     FROM cluster_gram_models ORDER BY cluster_id",
                )
                .unwrap();
            stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
        }
        #[cfg(feature = "postgres-backend")]
        pamet::storage::Backend::Postgres(pool) => {
            let mut conn = pool.get().unwrap();
            conn.query(
                "SELECT cluster_id, gram_order, vocab_size, training_token_count, table_json \
                 FROM cluster_gram_models ORDER BY cluster_id",
                &[],
            )
            .unwrap()
            .into_iter()
            .map(|r| {
                (
                    r.get(0),
                    r.get::<_, i32>(1) as i64,
                    r.get::<_, i32>(2) as i64,
                    r.get::<_, i32>(3) as i64,
                    r.get(4),
                )
            })
            .collect()
        }
    }
}

/// T020/FR-008/SC-004: synthetic fixture — seed near-duplicate statements,
/// consolidate (producing at least one real gram model), replay the event
/// log into a fresh store, and assert byte-for-byte equality. Runs on both
/// backends via `open_target()`.
#[test]
fn replayed_gram_models_match_source_byte_for_byte_synthetic() {
    let _guard = lock_db();
    let (source, _is_postgres) = open_target();

    remember(&source, "The deploy pipeline failed on build step three");
    remember(
        &source,
        "Deploy pipeline keeps failing at build step three stage",
    );
    pamet::consolidate(&source).unwrap();

    let before = gram_model_rows(&source);
    assert!(
        !before.is_empty(),
        "expected at least one real gram model from consolidation"
    );

    let rebuilt = rebuild::rebuild_from_events(&source).unwrap();
    let after = gram_model_rows(&rebuilt);

    assert_eq!(
        before, after,
        "replayed cluster_gram_models rows must be byte-for-byte identical to the source"
    );
}

/// T021/FR-008/SC-004: the same guarantee, checked against a real
/// consolidated store (a copy of this project's own live Pamet database,
/// post feature-004/005 migration and real `pamet consolidate` runs) rather
/// than only a synthetic fixture — spec.md's own stated bar.
#[test]
fn replayed_gram_models_match_source_byte_for_byte_real_data() {
    let _guard = lock_db();
    let real_db_path = std::env::var("PAMET_REAL_DB_FOR_GRAM_REPLAY_TEST")
        .unwrap_or_else(|_| "/Users/I779886/.hermes/pamet/memory.db".to_string());
    if !std::path::Path::new(&real_db_path).exists() {
        eprintln!("skipping real-data replay test: {real_db_path} not found");
        return;
    }

    // Copy, never touch the live file.
    let tmp_copy = std::env::temp_dir().join(format!(
        "pamet_gram_replay_real_{}.db",
        uuid::Uuid::new_v4()
    ));
    std::fs::copy(&real_db_path, &tmp_copy).expect("copy real db for read-only replay test");

    let source = Store::open(tmp_copy.to_str().unwrap()).unwrap();
    let before = gram_model_rows(&source);
    if before.is_empty() {
        eprintln!("real db has no cluster_gram_models rows yet — nothing to verify, skipping");
        let _ = std::fs::remove_file(&tmp_copy);
        return;
    }

    let rebuilt = rebuild::rebuild_from_events(&source).unwrap();
    let after = gram_model_rows(&rebuilt);

    let _ = std::fs::remove_file(&tmp_copy);

    assert_eq!(
        before, after,
        "replayed cluster_gram_models rows from the real consolidated store must be \
         byte-for-byte identical to the source"
    );
}
