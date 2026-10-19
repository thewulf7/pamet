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
            return Err(format!("hindsight recall failed: HTTP {}", resp.status()));
        }
        let parsed: RecallResponse = resp.json().map_err(|e| e.to_string())?;
        Ok(parsed.results)
    }
}
