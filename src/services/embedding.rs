//! Local, frozen, offline embedding (T006/T032, research.md Decisions 2-3).
//!
//! Replaces the hashed bag-of-words MVP placeholder with a real frozen
//! sentence-embedding model — `sentence-transformers/all-MiniLM-L6-v2`,
//! run locally via `ort` (ONNX Runtime). Never fine-tuned, never updated
//! from this project's own usage data (Constitution Principle III — a fixed
//! feature extractor is not learning).
//!
//! Model/tokenizer files are **not** bundled in this repo or this binary —
//! `pamet embedding fetch-model` downloads them once into a local cache
//! directory (research.md Decision 3's packaging choice). `embed()` loads
//! them lazily on first use and fails clearly (FR-008) if they are missing
//! or fail to load — it never falls back to a degraded/empty vector.
//!
//! The exact model artifact used is the one `ort`'s own official
//! `sentence-transformers` example ships (<https://cdn.pyke.io/0/pyke:ort-rs/example-models@0.0.0/all-MiniLM-L6-v2.onnx>,
//! paired with that same example's `tokenizer.json>`) rather than a generic
//! HuggingFace ONNX export: this specific artifact already bakes mean-pooling
//! into a second model output (`outputs[1]`), so `embed()` doesn't need to
//! hand-implement attention-mask-weighted mean pooling itself — a real
//! correctness risk area research.md's original "any HF export" framing
//! didn't anticipate. Confirmed via the example's real source at
//! `pykeio/ort` tag `v2.0.0-rc.15`.

use crate::error::{Error, Result};
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::TensorRef;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use tokenizers::Tokenizer;

/// Stable identifier for this frozen model/version (FR-009, data-model.md's
/// `embedding_models.id`).
pub const MODEL_ID: &str = "all-MiniLM-L6-v2-onnx-fp32-v1";

/// Real output dimension of this model (data-model.md, `memory_items_vec`'s
/// `FLOAT[384]` column) — not assumed, confirmed against the actual ONNX
/// model's output shape during implementation.
pub const DIM: usize = 384;

const MODEL_URL: &str =
    "https://cdn.pyke.io/0/pyke:ort-rs/example-models@0.0.0/all-MiniLM-L6-v2.onnx";
const TOKENIZER_URL: &str =
    "https://raw.githubusercontent.com/pykeio/ort/v2.0.0-rc.15/examples/sentence-transformers/data/tokenizer.json";
// Checksums captured from a real download during implementation (T006/T033) —
// `pamet embedding fetch-model` verifies against these so a corrupt/partial
// download fails loudly instead of silently loading a broken model.
const MODEL_SHA256: &str = "c01c3fb6f89996e15c1cb92ea3fa9cc3c76e9f385ed0b88ca71aebfad2be1547";
const TOKENIZER_SHA256: &str = "be50c3628f2bf5bb5e3a7f17b1f74611b2561a3a27eeab05e5aa30f411572037";

/// The local cache directory `pamet embedding fetch-model` downloads into
/// and `embed()` loads from — `$HOME/.cache/pamet/models`. No `dirs` crate
/// dependency: this project has no existing cache-dir convention to match
/// (the live Hermes-backing DB path is supplied externally via `--db`, never
/// computed by pamet itself), so a plain `$HOME`-relative path keeps this
/// self-contained.
pub fn model_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .map_err(|_| Error::Embedding("HOME environment variable not set".into()))?;
    Ok(PathBuf::from(home)
        .join(".cache")
        .join("pamet")
        .join("models"))
}

pub fn model_path() -> Result<PathBuf> {
    Ok(model_dir()?.join("all-MiniLM-L6-v2.onnx"))
}

pub fn tokenizer_path() -> Result<PathBuf> {
    Ok(model_dir()?.join("tokenizer.json"))
}

/// URLs + expected checksums `pamet embedding fetch-model` (T007/T033)
/// downloads and verifies. Exposed here, not duplicated in `bin/pamet.rs`,
/// so the model source is defined in exactly one place.
pub struct FetchTarget {
    pub url: &'static str,
    pub sha256: &'static str,
    pub dest: Result<PathBuf>,
}

pub fn fetch_targets() -> Vec<FetchTarget> {
    vec![
        FetchTarget {
            url: MODEL_URL,
            sha256: MODEL_SHA256,
            dest: model_path(),
        },
        FetchTarget {
            url: TOKENIZER_URL,
            sha256: TOKENIZER_SHA256,
            dest: tokenizer_path(),
        },
    ]
}

/// Per-target outcome for `pamet embedding fetch-model` (contracts/cli-contract.md).
pub struct FetchOutcome {
    pub dest: PathBuf,
    /// `true` if a file matching the expected checksum was already present
    /// (no download performed — idempotent re-run).
    pub already_current: bool,
}

fn sha256_hex(path: &std::path::Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path)
        .map_err(|e| Error::Embedding(format!("reading {}: {e}", path.display())))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(hex_encode(&hasher.finalize()))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Downloads (via `curl` — the one command in this CLI that needs network
/// access; kept as a subprocess call rather than pulling in an always-on
/// HTTP client crate like `reqwest` for every other command that never
/// touches the network) and checksum-verifies every model artifact into the
/// local cache directory. Idempotent: a file already present with the
/// correct checksum is left alone and reported as `already_current`, never
/// re-downloaded. A checksum mismatch after download is a hard error — this
/// function never leaves a corrupt/partial file in place silently.
pub fn fetch_model() -> Result<Vec<FetchOutcome>> {
    let dir = model_dir()?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| Error::Embedding(format!("creating {}: {e}", dir.display())))?;

    let mut outcomes = Vec::new();
    for target in fetch_targets() {
        let dest = target.dest?;
        if dest.exists() {
            let actual = sha256_hex(&dest)?;
            if actual == target.sha256 {
                outcomes.push(FetchOutcome {
                    dest,
                    already_current: true,
                });
                continue;
            }
            // Stale/corrupt — re-download below rather than trusting it.
        }
        let status = std::process::Command::new("curl")
            .args(["-sL", "--fail", "-o"])
            .arg(&dest)
            .arg(target.url)
            .status()
            .map_err(|e| Error::Embedding(format!("running curl: {e}")))?;
        if !status.success() {
            return Err(Error::Embedding(format!(
                "curl failed fetching {} (exit {:?})",
                target.url,
                status.code()
            )));
        }
        let actual = sha256_hex(&dest)?;
        if actual != target.sha256 {
            // Never leave a corrupt file in place for a later call to
            // silently treat as valid.
            let _ = std::fs::remove_file(&dest);
            return Err(Error::Embedding(format!(
                "checksum mismatch for {}: expected {}, got {actual} — download corrupted, \
                 removed the partial file, please retry",
                target.url, target.sha256
            )));
        }
        outcomes.push(FetchOutcome {
            dest,
            already_current: false,
        });
    }
    Ok(outcomes)
}

struct Embedder {
    session: Session,
    tokenizer: Tokenizer,
}

static EMBEDDER: OnceLock<Result<Mutex<Embedder>>> = OnceLock::new();

fn load_embedder() -> Result<Mutex<Embedder>> {
    let model_path = model_path()?;
    let tokenizer_path = tokenizer_path()?;
    if !model_path.exists() || !tokenizer_path.exists() {
        return Err(Error::Embedding(format!(
            "embedding model not found at {} — run `pamet embedding fetch-model` first \
             (this is a one-time setup step, never an implicit network call)",
            model_path.display()
        )));
    }
    let environment = ort::init()
        .build()
        .map_err(|e| Error::Embedding(format!("ort environment init: {e}")))?;
    let session = Session::builder(&environment)
        .map_err(|e| Error::Embedding(format!("ort session builder: {e}")))?
        .with_optimization_level(GraphOptimizationLevel::Level1)
        .map_err(|e| Error::Embedding(format!("ort optimization level: {e}")))?
        .with_intra_threads(1)
        .map_err(|e| Error::Embedding(format!("ort intra threads: {e}")))?
        .commit_from_file(&model_path)
        .map_err(|e| Error::Embedding(format!("loading ONNX model: {e}")))?;
    let tokenizer = Tokenizer::from_file(&tokenizer_path)
        .map_err(|e| Error::Embedding(format!("loading tokenizer: {e}")))?;
    Ok(Mutex::new(Embedder { session, tokenizer }))
}

/// Ensures `embedding_models` has a row for [`MODEL_ID`] (FR-009) — called
/// before the first `memory_items.embedding_model_id` reference to it, so
/// the FK is always satisfiable. Idempotent (`INSERT OR IGNORE`).
pub fn ensure_embedding_model_row(conn: &rusqlite::Connection) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO embedding_models (id, dimension, introduced_at) \
         VALUES (?1, ?2, ?3)",
        rusqlite::params![MODEL_ID, DIM as i64, chrono::Utc::now().to_rfc3339()],
    )?;
    Ok(())
}

/// Writes/overwrites `item_id`'s row in `memory_items_vec` (data-model.md).
/// `INSERT OR REPLACE` so re-embedding an already-backfilled item (T019/T037)
/// is naturally idempotent, not an error.
pub fn upsert_vector_row(conn: &rusqlite::Connection, item_id: &str, vector: &[f32]) -> Result<()> {
    use zerocopy::IntoBytes;
    conn.execute(
        "INSERT OR REPLACE INTO memory_items_vec (item_id, embedding) VALUES (?1, ?2)",
        rusqlite::params![item_id, vector.as_bytes()],
    )?;
    Ok(())
}

/// Embed `text` into a real, frozen 384-dim semantic vector via the local
/// ONNX MiniLM model. Fails clearly (FR-008) if the model hasn't been
/// fetched yet or fails to load — never a silent degraded/empty fallback.
pub fn embed(text: &str) -> Result<Vec<f32>> {
    let lock = EMBEDDER.get_or_init(load_embedder);
    let guard = match lock {
        Ok(mutex) => mutex,
        Err(e) => return Err(Error::Embedding(e.to_string())),
    };
    let mut embedder = guard
        .lock()
        .map_err(|_| Error::Embedding("embedding model mutex poisoned".into()))?;

    let encoding = embedder
        .tokenizer
        .encode(text, false)
        .map_err(|e| Error::Embedding(format!("tokenizing input: {e}")))?;
    let ids: Vec<i64> = encoding.get_ids().iter().map(|&i| i as i64).collect();
    let mask: Vec<i64> = encoding
        .get_attention_mask()
        .iter()
        .map(|&i| i as i64)
        .collect();
    let seq_len = ids.len();

    let a_ids = TensorRef::from_array_view(([1usize, seq_len], &*ids))
        .map_err(|e| Error::Embedding(format!("building input tensor: {e}")))?;
    let a_mask = TensorRef::from_array_view(([1usize, seq_len], &*mask))
        .map_err(|e| Error::Embedding(format!("building mask tensor: {e}")))?;

    let outputs = embedder
        .session
        .run(ort::inputs![a_ids, a_mask])
        .map_err(|e| Error::Embedding(format!("running inference: {e}")))?;

    // outputs[1] is this model's baked-in mean-pooled sentence embedding
    // (see module doc comment) — [1, DIM], not the per-token [1, seq_len, DIM]
    // output at index 0.
    let (_shape, data) = outputs[1]
        .try_extract_tensor::<f32>()
        .map_err(|e| Error::Embedding(format!("extracting output tensor: {e}")))?;
    if data.len() != DIM {
        return Err(Error::Embedding(format!(
            "model output dimension {} does not match expected {DIM} — wrong model file?",
            data.len()
        )));
    }
    Ok(data.to_vec())
}

/// Cosine similarity between two equal-length vectors (0.0 if either is zero).
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}

/// Plain word-splitting tokenizer used by `write_policy.rs`'s *keyword*
/// overlap prefilter — unrelated to the ONNX model's own WordPiece
/// tokenizer above (kept for that existing, independent caller).
pub fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real model-dependent tests live in tests/unit/test_embedding.rs (run
    // against the real fetched model, not mocked) — see T008/T038.

    #[test]
    fn tokenize_is_pure_word_splitting_unrelated_to_the_model() {
        assert_eq!(
            tokenize("Hello, World!"),
            vec!["hello".to_string(), "world".to_string()]
        );
    }

    #[test]
    fn cosine_similarity_handles_zero_vectors() {
        assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
    }
}
