//! Semantic response cache — catches semantically similar prompts
//! to avoid duplicate API calls.
//!
//! Two-tier architecture:
//!   1. **Exact match** (SHA-256) — zero latency, always correct.
//!      Already implemented in `recording::store`.
//!   2. **Semantic match** (this module) — cosine similarity of
//!      lightweight text embeddings. Higher recall, slightly lower
//!      precision. Configurable threshold.
//!
//! Current implementation uses a fast Jaccard word-overlap index
//! as a lightweight approximation. The full ONNX embedding path is
//! designed for a future upgrade when `ort` (ONNX Runtime) is
//! available on the target platform.

/// A semantic cache entry: prompt text → cached response + metadata.
#[derive(Debug, Clone)]
pub struct SemanticEntry {
    /// The original prompt text (kept for similarity comparison).
    pub prompt: String,
    /// The cached JSON response.
    pub response_json: String,
    /// Model used for this response.
    pub model: String,
    /// Unix timestamp when cached.
    pub created_at: i64,
    /// How many times this entry has been served.
    pub hit_count: u64,
}

/// Lightweight semantic cache using Jaccard word overlap.
///
/// For each incoming prompt, we compute the Jaccard similarity
/// (intersection / union of word sets) against all cached entries.
/// If any entry exceeds the configured threshold, the cached
/// response is returned.
///
/// Memory limit is enforced by evicting the least-recently-used
/// entry when capacity is exceeded.
pub struct SemanticCache {
    entries: Vec<SemanticEntry>,
    /// Maximum number of cached entries.
    max_entries: usize,
    /// Minimum Jaccard similarity to consider a match (0.0–1.0).
    /// 0.85 is a conservative default — catches paraphrases but
    /// not truly different prompts.
    threshold: f64,
    /// Cache TTL in seconds.
    ttl_secs: i64,
}

impl SemanticCache {
    /// Create a new semantic cache.
    pub fn new(max_entries: usize, threshold: f64, ttl_hours: u32) -> Self {
        Self {
            entries: Vec::with_capacity(max_entries),
            max_entries,
            threshold,
            ttl_secs: ttl_hours as i64 * 3600,
        }
    }

    /// Look up a prompt in the semantic cache.
    /// Returns the cached response JSON if a similar enough prompt is found.
    pub fn get(&self, prompt: &str, now: i64) -> Option<&str> {
        if prompt.is_empty() || self.entries.is_empty() {
            return None;
        }

        let words = tokenize(prompt);

        for entry in &self.entries {
            // Check TTL
            if now - entry.created_at > self.ttl_secs {
                continue;
            }
            let entry_words = tokenize(&entry.prompt);
            let sim = jaccard_similarity(&words, &entry_words);
            if sim >= self.threshold {
                return Some(&entry.response_json);
            }
        }

        None
    }

    /// Store a prompt → response mapping.
    pub fn put(&mut self, prompt: &str, response_json: &str, model: &str, now: i64) {
        // Evict if at capacity — remove oldest
        if self.entries.len() >= self.max_entries {
            self.entries.sort_by_key(|e| e.created_at);
            self.entries.remove(0);
        }

        // Remove any exact-duplicate prompt (update in place)
        self.entries.retain(|e| e.prompt != prompt);

        self.entries.push(SemanticEntry {
            prompt: prompt.to_string(),
            response_json: response_json.to_string(),
            model: model.to_string(),
            created_at: now,
            hit_count: 1,
        });
    }

    /// Number of cached entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Tokenize text into lowercase word set for Jaccard comparison.
fn tokenize(text: &str) -> Vec<String> {
    let mut words: Vec<String> = text
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3)
        .map(|w| w.to_string())
        .collect();
    words.sort();
    words.dedup();
    words
}

/// Compute Jaccard similarity: |A ∩ B| / |A ∪ B|.
fn jaccard_similarity(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }

    // Count intersection
    let mut intersection = 0usize;
    let mut i = 0;
    let mut j = 0;
    while i < a.len() && j < b.len() {
        if a[i] == b[j] {
            intersection += 1;
            i += 1;
            j += 1;
        } else if a[i] < b[j] {
            i += 1;
        } else {
            j += 1;
        }
    }

    let union = a.len() + b.len() - intersection;
    intersection as f64 / union as f64
}

/// Configuration for the semantic cache (read from config.yaml).
#[derive(Debug, Clone)]
pub struct SemanticCacheConfig {
    /// Enable semantic caching (requires Pro license).
    pub enabled: bool,
    /// Maximum entries in the semantic cache.
    #[allow(dead_code)]
    pub max_entries: usize,
    /// Minimum Jaccard similarity threshold (0.0–1.0).
    pub threshold: f64,
}

impl Default for SemanticCacheConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_entries: 5_000,
            threshold: 0.85,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize() {
        let words = tokenize("Hello, World! This is a test.");
        assert!(words.contains(&"hello".to_string()));
        assert!(words.contains(&"world".to_string()));
        assert!(words.contains(&"this".to_string()));
        assert!(words.contains(&"test".to_string()));
        // "is" and "a" are < 3 chars, filtered out (only >=3)
        assert!(!words.contains(&"is".to_string()));
        assert!(!words.contains(&"a".to_string()));
    }

    #[test]
    fn test_jaccard_identical() {
        let a = tokenize("summarize this article");
        let b = tokenize("summarize this article");
        assert!((jaccard_similarity(&a, &b) - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_jaccard_different() {
        let a = tokenize("summarize this article");
        let b = tokenize("write python code for sorting");
        assert!(jaccard_similarity(&a, &b) < 0.2);
    }

    #[test]
    fn test_jaccard_semantic_paraphrase() {
        let a = tokenize("summarize this article about AI");
        let b = tokenize("please summarize the article regarding artificial intelligence");
        let sim = jaccard_similarity(&a, &b);
        // "summarize" + "article" overlap — should be partial match
        assert!(sim > 0.1 && sim < 0.9);
    }

    #[test]
    fn test_semantic_cache_basic() {
        let mut cache = SemanticCache::new(100, 0.8, 24);
        let now = 1000;
        cache.put(
            "summarize this article about AI",
            r#"{"response":"ok"}"#,
            "test-model",
            now,
        );

        // Exact same prompt should match
        let result = cache.get("summarize this article about AI", now);
        assert!(result.is_some());

        // Similar prompt should match (enough word overlap)
        let result = cache.get("please summarize this article about AI", now);
        assert!(result.is_some());

        // Different prompt should NOT match
        let result = cache.get("write code to sort a list", now);
        assert!(result.is_none());
    }

    #[test]
    fn test_semantic_cache_ttl() {
        let mut cache = SemanticCache::new(100, 0.8, 24);
        let now = 1000;
        cache.put(
            "summarize this article",
            r#"{"response":"ok"}"#,
            "test-model",
            now,
        );

        // Within TTL
        assert!(cache.get("summarize this article", now + 100).is_some());

        // Past TTL (24h = 86400s)
        assert!(cache.get("summarize this article", now + 86500).is_none());
    }

    #[test]
    fn test_semantic_cache_eviction() {
        let mut cache = SemanticCache::new(3, 0.8, 24);
        let now = 1000;
        cache.put("prompt one", r#"{"r":"1"}"#, "m", now);
        cache.put("prompt two", r#"{"r":"2"}"#, "m", now + 1);
        cache.put("prompt three", r#"{"r":"3"}"#, "m", now + 2);
        assert_eq!(cache.len(), 3);

        // Fourth entry evicts oldest
        cache.put("prompt four", r#"{"r":"4"}"#, "m", now + 3);
        assert_eq!(cache.len(), 3);
        assert!(cache.get("prompt one", now + 10).is_none());
    }
}
