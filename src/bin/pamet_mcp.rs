//! `pamet-mcp`: exposes the pamet memory engine as MCP tools over stdio so
//! a real agent (e.g. Claude Code) can call `remember`/`recall`/`outcome`
//! directly during a session, instead of relying only on its own context
//! window or built-in memory.
//!
//! Build/run: `cargo run --features mcp --bin pamet-mcp -- --db <path>`

use pamet::{Decision, Interaction, Store};
use rmcp::{
    ServerHandler, ServiceExt, model::{ServerCapabilities, ServerInfo},
    schemars, tool, transport::stdio,
};
use schemars::JsonSchema;
use serde::Deserialize;
use std::sync::Mutex;

#[derive(Debug, Deserialize, JsonSchema)]
struct RememberRequest {
    #[schemars(description = "The text to consider remembering (one or more statements).")]
    text: String,
    #[schemars(description = "Who/what produced this text, e.g. 'claude-code-session'.")]
    source_id: Option<String>,
    #[schemars(description = "Force-remember even if the write policy would otherwise skip it.")]
    explicit_remember: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RecallRequest {
    #[schemars(description = "The query to find relevant memories for.")]
    query: String,
    #[schemars(description = "Maximum number of memories to return (default 5).")]
    budget: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct OutcomeRequest {
    #[schemars(description = "The event_id returned by a prior recall call.")]
    event_id: i64,
    #[schemars(description = "The memory item id that was (or wasn't) actually used.")]
    item_id: String,
    #[schemars(description = "Whether the returned memory was actually used/cited.")]
    used: bool,
}

#[derive(Clone)]
struct PametMcp {
    store: std::sync::Arc<Mutex<Store>>,
}

#[tool(tool_box)]
impl PametMcp {
    #[tool(description = "Store a new memory, merge/supersede an existing one, or skip it — \
        decided by pamet's rule-based write policy. Call this whenever the conversation \
        contains a fact, preference, or decision worth remembering across sessions.")]
    fn remember(&self, #[tool(aggr)] req: RememberRequest) -> String {
        let store = self.store.lock().unwrap();
        let interaction = Interaction {
            source_id: req.source_id.unwrap_or_else(|| "mcp".to_string()),
            text: req.text,
            explicit_remember: req.explicit_remember.unwrap_or(false),
        };
        match pamet::remember(&store, &interaction) {
            Ok(decisions) => {
                let rendered: Vec<serde_json::Value> = decisions
                    .iter()
                    .map(|d| {
                        serde_json::json!({
                            "decision": decision_str(d.decision),
                            "rule_name": d.rule_name,
                            "item_id": d.resulting_item_id,
                        })
                    })
                    .collect();
                serde_json::to_string(&rendered).unwrap_or_else(|e| format!("error: {e}"))
            }
            Err(e) => format!("error: {e}"),
        }
    }

    #[tool(description = "Retrieve memories relevant to a query, ranked by hybrid \
        (semantic + keyword) search. Call this before answering anything that might \
        depend on something remembered earlier in a prior session.")]
    fn recall(&self, #[tool(aggr)] req: RecallRequest) -> String {
        let store = self.store.lock().unwrap();
        match pamet::recall(&store, &req.query, req.budget) {
            Ok(result) => {
                let items: Vec<serde_json::Value> = result
                    .items
                    .iter()
                    .map(|i| serde_json::json!({ "item_id": i.id, "content": i.content }))
                    .collect();
                serde_json::to_string(&serde_json::json!({
                    "event_id": result.event_id,
                    "items": items,
                }))
                .unwrap_or_else(|e| format!("error: {e}"))
            }
            Err(e) => format!("error: {e}"),
        }
    }

    #[tool(description = "Report back whether a memory returned by `recall` was actually \
        used in the answer. This closes the loop that future learned components will be \
        trained on (pamet's whole MVP exists to generate this training signal).")]
    fn outcome(&self, #[tool(aggr)] req: OutcomeRequest) -> String {
        let store = self.store.lock().unwrap();
        match pamet::report_outcome(&store, req.event_id, &req.item_id, req.used) {
            Ok(()) => "ok".to_string(),
            Err(e) => format!("error: {e}"),
        }
    }
}

fn decision_str(d: Decision) -> &'static str {
    match d {
        Decision::Store => "store",
        Decision::Merge => "merge",
        Decision::Supersede => "supersede",
        Decision::Skip => "skip",
        Decision::Review => "review",
    }
}

#[tool(tool_box)]
impl ServerHandler for PametMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "Pamet: a persistent, rule-based agent memory store. Use `remember` to save \
                 facts/preferences/decisions across sessions, `recall` to retrieve them, and \
                 `outcome` to report whether a recalled memory was actually used."
                    .into(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let db_path = args
        .iter()
        .position(|a| a == "--db")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| "./pamet.db".to_string());

    let store = Store::open(&db_path).map_err(|e| anyhow::anyhow!("failed to open store: {e}"))?;
    let server = PametMcp { store: std::sync::Arc::new(Mutex::new(store)) };
    let running = server.serve(stdio()).await?;
    running.waiting().await?;
    Ok(())
}
