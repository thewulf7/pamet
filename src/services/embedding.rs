//! Local, frozen, offline embedding (T007).
//!
//! Deviation from research.md's original choice of `ort` + a downloaded ONNX
//! transformer model: this MVP uses a deterministic hashed bag-of-words
//! embedding instead. It is still local, offline, frozen (no training/fine-
//! tuning — Constitution Principle III), and requires no model download or
//! loadable C extension, which keeps the MVP buildable/testable without a
//! network-fetched model artifact. Swapping in a transformer embedder later
//! is a drop-in replacement behind this same `embed()` function.

const DIM: usize = 4096;

/// Embed `text` into a fixed-size, L2-normalized vector using hashed
/// bag-of-words features (frozen, deterministic — no learned weights).
pub fn embed(text: &str) -> Vec<f32> {
    let mut v = vec![0f32; DIM];
    for token in tokenize(text) {
        let bucket = (hash(&token) as usize) % DIM;
        v[bucket] += 1.0;
    }
    normalize(&mut v);
    v
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

pub fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

fn hash(s: &str) -> u64 {
    // FNV-1a — simple, deterministic, no external crate needed.
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn normalize(v: &mut [f32]) {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_text_same_embedding() {
        assert_eq!(embed("hello world"), embed("hello world"));
    }

    #[test]
    fn similar_text_scores_higher_than_unrelated() {
        let a = embed("the project is called pamet");
        let b = embed("what is the project called");
        let c = embed("completely unrelated query about weather");
        assert!(cosine_similarity(&a, &b) > cosine_similarity(&a, &c));
    }
}
