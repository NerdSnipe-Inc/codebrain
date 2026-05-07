//! SHA-256–keyed per-file extraction cache.
//!
//! Each source file that goes through AST or semantic extraction is hashed
//! and the result stored as `{hash}.json` in a cache directory. On the next
//! scan, unchanged files are loaded from cache without re-running the extractor.
//!
//! # Cache key
//! SHA-256 of the file's byte content. Content-addressable: renames and moves
//! do not invalidate the cache; only actual content changes do.
//!
//! # Cache format
//! Each entry is a JSON-serialised `CachedExtraction` written by the extractor
//! and read back by this module.
//!
//! # Thread safety
//! `ExtractionCache` is cheap to clone (Arc-wrapped inner) and safe to share
//! across threads for concurrent extraction tasks.

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::extract::ExtractionResult;

/// Serialisable form of one file's extraction output, stored on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedExtraction {
    /// The SHA-256 hex digest that keys this entry.
    pub content_hash: String,
    /// Relative file path at extraction time (informational only — the key is
    /// the hash, so path changes don't invalidate the entry).
    pub relative_path: String,
    /// The extraction result stored for reuse.
    pub result: ExtractionResult,
}

/// Manages a directory of per-file extraction cache entries.
#[derive(Clone)]
pub struct ExtractionCache {
    inner: Arc<CacheInner>,
}

struct CacheInner {
    dir: PathBuf,
}

impl ExtractionCache {
    /// Create or open a cache at `dir`. Creates the directory if it does not
    /// exist (logs a warning and degrades to no-op on failure).
    pub fn open(dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        if let Err(e) = std::fs::create_dir_all(&dir) {
            tracing::warn!(
                err  = %e,
                path = %dir.display(),
                "codebrain cache: could not create cache directory — running without cache"
            );
        }
        Self { inner: Arc::new(CacheInner { dir }) }
    }

    /// Default cache location: `~/.cache/codebrain/ast-cache/`.
    pub fn default_location() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".cache")
            .join("codebrain")
            .join("ast-cache")
    }

    // ── Public API ────────────────────────────────────────────────────────────

    /// SHA-256 hex digest of `content`.
    pub fn hash(content: &str) -> String {
        let mut h = Sha256::new();
        h.update(content.as_bytes());
        format!("{:x}", h.finalize())
    }

    /// Look up a cached extraction by content hash. Returns `None` on miss or
    /// any deserialisation error (treated as a cache miss — extractor reruns).
    pub fn get(&self, content_hash: &str) -> Option<ExtractionResult> {
        let path = self.entry_path(content_hash);
        let json = std::fs::read_to_string(&path).ok()?;
        let entry: CachedExtraction = serde_json::from_str(&json)
            .map_err(|e| {
                tracing::debug!(
                    err  = %e,
                    hash = content_hash,
                    "codebrain cache: corrupt entry — treating as miss"
                );
                e
            })
            .ok()?;
        Some(entry.result)
    }

    /// Persist an extraction result for future runs.
    pub fn put(&self, content_hash: &str, relative_path: &str, result: &ExtractionResult) {
        let entry = CachedExtraction {
            content_hash:  content_hash.to_string(),
            relative_path: relative_path.to_string(),
            result:        result.clone(),
        };
        let path = self.entry_path(content_hash);
        match serde_json::to_string(&entry) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&path, &json) {
                    tracing::debug!(
                        err  = %e,
                        path = %path.display(),
                        "codebrain cache: write failed"
                    );
                }
            }
            Err(e) => tracing::debug!(err = %e, "codebrain cache: serialise failed"),
        }
    }

    /// Split a file list into (cache_hits, cache_misses) by content hash.
    /// Callers run extraction only on the misses.
    pub fn partition<'a>(
        &self,
        files: &'a [(String, String)], // (relative_path, content)
    ) -> (Vec<(String, ExtractionResult)>, Vec<&'a (String, String)>) {
        let mut hits   = Vec::new();
        let mut misses = Vec::new();

        for item in files {
            let (path, content) = item;
            let hash = Self::hash(content);
            match self.get(&hash) {
                Some(result) => hits.push((path.clone(), result)),
                None         => misses.push(item),
            }
        }

        (hits, misses)
    }

    // ── Private ───────────────────────────────────────────────────────────────

    fn entry_path(&self, hash: &str) -> PathBuf {
        self.inner.dir.join(format!("{hash}.json"))
    }
}
