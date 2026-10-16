//! Decay computation (research.md §3, FR-004). T012.
//!
//! Decay is NOT a stored per-item column — it's a pure function over fields
//! `MemoryItem` already has (`last_used_at`, `use_count`), computed at read
//! time. Purely a retrieval-ranking adjustment; never deletes or tombstones
//! anything (Constitution Principle II).

use chrono::{DateTime, Utc};

/// Items unused this long (and never used at all) get the decay penalty.
pub const DECAY_AFTER_DAYS: i64 = 30;
/// Score multiplier applied to a decayed item's fused retrieval score.
pub const DECAY_PENALTY: f32 = 0.5;

/// Compute the decay multiplier for one item (1.0 = no decay).
///
/// - A never-used item (`use_count == 0`) with no `last_used_at` decays
///   relative to its `created_at`.
/// - A used item decays relative to its most recent `last_used_at`.
/// - Any unparseable/missing timestamp is treated as "not decayed" (fail
///   open on ranking only — never on data integrity).
pub fn multiplier(
    created_at: &str,
    last_used_at: Option<&str>,
    use_count: i64,
    now: DateTime<Utc>,
    after_days: i64,
    penalty: f32,
) -> f32 {
    let reference = last_used_at.unwrap_or(created_at);
    let Ok(parsed) = DateTime::parse_from_rfc3339(reference) else {
        return 1.0;
    };
    let age_days = (now - parsed.with_timezone(&Utc)).num_days();
    let _ = use_count; // reserved: a future rule could weight by use_count too
    if age_days >= after_days {
        penalty
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn fresh_item_not_decayed() {
        let now = Utc::now();
        let created = now.to_rfc3339();
        assert_eq!(multiplier(&created, None, 0, now, DECAY_AFTER_DAYS, DECAY_PENALTY), 1.0);
    }

    #[test]
    fn stale_item_decayed() {
        let now = Utc::now();
        let created = (now - Duration::days(DECAY_AFTER_DAYS + 1)).to_rfc3339();
        assert_eq!(
            multiplier(&created, None, 0, now, DECAY_AFTER_DAYS, DECAY_PENALTY),
            DECAY_PENALTY
        );
    }

    #[test]
    fn recently_used_resets_decay() {
        let now = Utc::now();
        let created = (now - Duration::days(DECAY_AFTER_DAYS + 10)).to_rfc3339();
        let last_used = now.to_rfc3339();
        assert_eq!(
            multiplier(&created, Some(&last_used), 3, now, DECAY_AFTER_DAYS, DECAY_PENALTY),
            1.0
        );
    }
}
