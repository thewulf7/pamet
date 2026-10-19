//! Thin Hindsight REST client for the baseline comparison (research.md §7,
//! FR-007). Verified directly against the local Hindsight instance's
//! OpenAPI schema and a live round-trip before writing this (not guessed):
//! `POST /v1/default/banks/{bank_id}/memories` (retain) and
//! `POST /v1/default/banks/{bank_id}/memories/recall` (recall).
//!
//! Feature-gated (`baseline-hindsight`) — the mandatory, network-free parts
//! of this phase (consolidation, Pamet-only eval) never pull this in.

use serde::{Deserialize, Serialize};
use std::time::Duration;

const DEFAULT_BASE_URL: &str = "http://localhost:8888";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub struct HindsightClient {
    base_url: String,
    client: reqwest::blocking::Client,
}

#[derive(Debug, Serialize)]
struct RetainItem<'a> {
    content: &'a str,
}

#[derive(Debug, Serialize)]
struct RetainRequest<'a> {
    items: Vec<RetainItem<'a>>,
    #[serde(rename = "async")]
    is_async: bool,
}

#[derive(Debug, Serialize)]
struct RecallRequest<'a> {
    query: &'a str,
    budget: &'a str,
}

#[derive(Debug, Deserialize)]
pub struct RecallResult {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Deserialize)]
pub struct RecallResponse {
    pub results: Vec<RecallResult>,
}

/// One memory as returned by `GET .../memories/list` (feature 004's
/// migration read side) — a superset of what `recall()` returns, since
/// `list` is also where provenance (`document_id`/`tags`/`fact_type`/
/// `state`) for the migration ledger comes from. Verified directly against
/// the live `hermes` bank's real response shape before being hardcoded —
/// fields not needed by the migration are simply not modeled.
#[derive(Debug, Clone, Deserialize)]
pub struct HindsightMemory {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub date: Option<String>,
    pub fact_type: String,
    #[serde(default)]
    pub document_id: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub state: String,
}

#[derive(Debug, Deserialize)]
struct ListResponse {
    items: Vec<HindsightMemory>,
    total: i64,
}

impl HindsightClient {
    /// `base_url`, if given, wins; otherwise `PAMET_HINDSIGHT_BASE_URL` wins
    /// (lets the T049 degraded-path test force an unreachable address
    /// deterministically, regardless of whatever happens to be running on
    /// the real default port on the machine running the test); otherwise
    /// `DEFAULT_BASE_URL`.
    pub fn new(base_url: Option<&str>) -> Self {
        let resolved = base_url
            .map(str::to_string)
            .or_else(|| std::env::var("PAMET_HINDSIGHT_BASE_URL").ok())
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        Self {
            base_url: resolved.trim_end_matches('/').to_string(),
            client: reqwest::blocking::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .build()
                .expect("reqwest client build should never fail with only a timeout configured"),
        }
    }

    /// `true` if Hindsight answered `/health` — used to decide whether the
    /// baseline report's Hindsight column is attempted at all (spec Edge
    /// Cases: unreachable Hindsight must not fail the whole `eval` run).
    pub fn is_reachable(&self) -> bool {
        self.client
            .get(format!("{}/health", self.base_url))
            .send()
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    pub fn retain(&self, bank_id: &str, content: &str) -> Result<(), String> {
        let body = RetainRequest {
            items: vec![RetainItem { content }],
            is_async: false,
        };
        let resp = self
            .client
            .post(format!(
                "{}/v1/default/banks/{}/memories",
                self.base_url, bank_id
            ))
            .json(&body)
            .send()
            .map_err(|e| e.to_string())?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(format!("hindsight retain failed: HTTP {}", resp.status()))
        }
    }

    pub fn recall(&self, bank_id: &str, query: &str) -> Result<Vec<RecallResult>, String> {
        let body = RecallRequest {
            query,
            budget: "mid",
        };
        let resp = self
            .client
            .post(format!(
                "{}/v1/default/banks/{}/memories/recall",
                self.base_url, bank_id
            ))
            .json(&body)
            .send()
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!(
                "hindsight recall failed: HTTP {} — {}",
                resp.status(),
                resp.text().unwrap_or_default()
            ));
        }
        let parsed: RecallResponse = resp.json().map_err(|e| e.to_string())?;
        Ok(parsed.results)
    }

    /// One page of `GET .../memories/list` (feature 004, FR-001). Callers
    /// paginate by bumping `offset` by `limit` until the returned page is
    /// shorter than `limit` (or `offset >= total`).
    pub fn list_memories_page(
        &self,
        bank_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<HindsightMemory>, i64), String> {
        let resp = self
            .client
            .get(format!(
                "{}/v1/default/banks/{}/memories/list",
                self.base_url, bank_id
            ))
            .query(&[("limit", limit), ("offset", offset)])
            .send()
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!(
                "hindsight list failed: HTTP {} — {}",
                resp.status(),
                resp.text().unwrap_or_default()
            ));
        }
        let parsed: ListResponse = resp.json().map_err(|e| e.to_string())?;
        Ok((parsed.items, parsed.total))
    }
}
