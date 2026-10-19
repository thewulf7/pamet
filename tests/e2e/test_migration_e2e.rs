//! US2 (feature 003): `pamet migrate` / `rebuild_into` end-to-end — every
//! event from a SQLite source store reproduces equivalent live state in a
//! fresh Postgres target store. Requires a reachable Postgres+pgvector
//! instance at `PAMET_TEST_POSTGRES_URL`.

#![cfg(feature = "postgres-backend")]

use pamet::{Interaction, Store};

fn test_db_url() -> Option<String> {
    std::env::var("PAMET_TEST_POSTGRES_URL").ok()
}

#[test]
fn migrate_sqlite_source_into_postgres_target_preserves_recall() {
    let Some(url) = test_db_url() else {
        eprintln!("skipping: PAMET_TEST_POSTGRES_URL not set");
        return;
    };

    let source = Store::open_in_memory().unwrap();
    pamet::remember(
        &source,
        &Interaction {
            source_id: "test".into(),
            text: "The migration preserves this fact across backends".into(),
            explicit_remember: true,
        },
    )
    .unwrap();
    pamet::recall(&source, "migration preserves fact", None).unwrap();

    let target = match Store::open(&url) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("skipping: Postgres unreachable: {e}");
            return;
        }
    };

    let source_log_len = pamet::export_log(&source, None).unwrap().len();
    let migrated = pamet::services::rebuild::rebuild_into(&source, &target).unwrap();
    assert_eq!(migrated, source_log_len);

    let result = pamet::recall(&target, "migration preserves fact", None).unwrap();
    assert!(
        result
            .items
            .iter()
            .any(|i| i.content.contains("migration preserves this fact")),
        "migrated item not found via recall on target: {:?}",
        result.items
    );
}
