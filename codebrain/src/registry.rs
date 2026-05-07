//! Per-project CodeBrain registry.
//!
//! Maintains one `CodeBrainHandle` per project UUID. On first access for a
//! project, the registry checks for two persisted files on disk:
//!   - `~/.cache/codebrain/{project_id}.json`       → flat ScanResult
//!   - `~/.cache/codebrain/{project_id}_graph.json` → GraphStore
//!
//! Both are warm-started into a `CodeBrainHandle::new_with_full_cache()` so
//! app restarts do not force a full re-scan or re-extraction.
//!
//! # Typical lifecycle
//! 1. App starts — `CodeBrainRegistry::init()` creates the cache directory.
//! 2. User adds a project — the caller calls `registry.force_reindex(project_id, local_path)`,
//!    which scans, builds the graph, and persists both files.
//! 3. Subsequent requests call `registry.get(project_id)` — the handle returns
//!    cached data instantly.
//! 4. A restart hits step 1 again but warm-starts from disk on first access.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::model::CodeGraph;
use crate::types::{GraphStore, ScanResult};
use crate::{CodeBrainConfig, CodeBrainHandle};

#[derive(Clone)]
pub struct CodeBrainRegistry {
    inner:     Arc<RwLock<HashMap<String, Arc<CodeBrainHandle>>>>,
    cache_dir: PathBuf,
}

impl CodeBrainRegistry {
    /// Initialise using `~/.cache/codebrain/`.
    pub async fn init() -> Self {
        let cache_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".cache")
            .join("codebrain");
        Self::init_at(cache_dir).await
    }

    /// Initialise with an explicit cache directory (useful for tests).
    pub async fn init_at(cache_dir: PathBuf) -> Self {
        if let Err(e) = tokio::fs::create_dir_all(&cache_dir).await {
            tracing::warn!(
                err  = %e,
                path = %cache_dir.display(),
                "codebrain: could not create cache directory"
            );
        }
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            cache_dir,
        }
    }

    // ── Public API ────────────────────────────────────────────────────────────

    /// Return the handle for an already-initialised project, or `None`.
    pub async fn get(&self, project_id: &str) -> Option<Arc<CodeBrainHandle>> {
        self.inner.read().await.get(project_id).cloned()
    }

    /// Return the handle for a project, warm-starting from disk if needed.
    pub async fn get_or_init(&self, project_id: &str, local_path: &Path) -> Arc<CodeBrainHandle> {
        {
            let map = self.inner.read().await;
            if let Some(h) = map.get(project_id) {
                return Arc::clone(h);
            }
        }

        let handle = Arc::new(match self.load_cached(project_id, local_path) {
            Some(h) => {
                tracing::info!(
                    project_id,
                    path = %local_path.display(),
                    "codebrain: warm-started from disk cache"
                );
                h
            }
            None => {
                tracing::debug!(
                    project_id,
                    path = %local_path.display(),
                    "codebrain: no cache — cold handle created"
                );
                CodeBrainHandle::new(CodeBrainConfig::new(local_path))
            }
        });

        let mut map = self.inner.write().await;
        map.entry(project_id.to_string())
            .or_insert_with(|| Arc::clone(&handle))
            .clone()
    }

    /// Force a full rescan, persist results, and register the handle.
    /// Safe to call from async context — scan runs in a blocking thread.
    pub async fn force_reindex(
        &self,
        project_id: &str,
        local_path: &Path,
    ) -> anyhow::Result<()> {
        let path_str   = local_path.to_string_lossy().to_string();
        let config     = CodeBrainConfig::new(local_path);
        let project_id = project_id.to_string();
        let cache_dir  = self.cache_dir.clone();
        let registry   = Arc::clone(&self.inner);

        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let handle = CodeBrainHandle::new(config.clone());
            // force_scan() calls scanner::scan() and populates both caches
            let scan_result = handle.force_scan()?;
            let graph       = handle.graph();

            tracing::info!(
                project_id = %project_id,
                path       = %path_str,
                routes     = scan_result.routes.len(),
                schemas    = scan_result.schemas.len(),
                nodes      = graph.as_ref().map(|g| g.node_count()).unwrap_or(0),
                edges      = graph.as_ref().map(|g| g.edge_count()).unwrap_or(0),
                "codebrain: indexed project"
            );

            // Persist flat scan result
            let scan_path = cache_dir.join(format!("{project_id}.json"));
            persist_json(&scan_result, &scan_path, "scan result");

            // Persist graph store (separate file so old clients can load scan
            // without parsing the graph)
            if let Some(g) = &graph {
                let graph_store = g.to_store();
                let graph_path  = cache_dir.join(format!("{project_id}_graph.json"));
                persist_json(&graph_store, &graph_path, "graph store");
            }

            // Register warm handle in memory
            let warm = Arc::new(match graph {
                Some(g) => CodeBrainHandle::new_with_full_cache(
                    CodeBrainConfig::new(&path_str),
                    scan_result,
                    Arc::try_unwrap(g).unwrap_or_else(|arc| (*arc).clone()),
                ),
                None => CodeBrainHandle::new_with_cache(
                    CodeBrainConfig::new(&path_str),
                    scan_result,
                ),
            });

            tokio::runtime::Handle::current().block_on(async {
                registry.write().await.insert(project_id, warm);
            });

            Ok(())
        })
        .await
        .map_err(|e| anyhow::anyhow!("codebrain index task panicked: {e}"))??;

        Ok(())
    }

    /// List all project IDs currently loaded in memory.
    pub async fn project_ids(&self) -> Vec<String> {
        self.inner.read().await.keys().cloned().collect()
    }

    /// Remove a project's handle and delete its disk cache files.
    pub async fn invalidate(&self, project_id: &str) {
        self.inner.write().await.remove(project_id);
        let _ = tokio::fs::remove_file(
            self.cache_dir.join(format!("{project_id}.json"))
        ).await;
        let _ = tokio::fs::remove_file(
            self.cache_dir.join(format!("{project_id}_graph.json"))
        ).await;
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    /// Try to deserialise both cache files and return a fully warm handle.
    fn load_cached(&self, project_id: &str, local_path: &Path) -> Option<CodeBrainHandle> {
        let scan_path  = self.cache_dir.join(format!("{project_id}.json"));
        let graph_path = self.cache_dir.join(format!("{project_id}_graph.json"));

        let scan_json = std::fs::read_to_string(&scan_path).ok()?;
        let scan: ScanResult = serde_json::from_str(&scan_json)
            .map_err(|e| tracing::warn!(err = %e, project_id, "codebrain: corrupt scan cache"))
            .ok()?;

        // Graph cache is optional — a missing or corrupt graph file degrades
        // gracefully to a handle with only the flat scan result.
        let graph: Option<CodeGraph> = std::fs::read_to_string(&graph_path)
            .ok()
            .and_then(|json| {
                serde_json::from_str::<GraphStore>(&json)
                    .map_err(|e| tracing::warn!(err = %e, project_id, "codebrain: corrupt graph cache"))
                    .ok()
            })
            .map(CodeGraph::from_store);

        let config = CodeBrainConfig::new(local_path);
        Some(match graph {
            Some(g) => CodeBrainHandle::new_with_full_cache(config, scan, g),
            None    => CodeBrainHandle::new_with_cache(config, scan),
        })
    }
}

// ── Shared helper ─────────────────────────────────────────────────────────────

fn persist_json<T: serde::Serialize>(value: &T, path: &Path, label: &str) {
    match serde_json::to_string(value) {
        Ok(json) => {
            if let Err(e) = std::fs::write(path, &json) {
                tracing::warn!(err = %e, path = %path.display(), "codebrain: failed to write {label}");
            }
        }
        Err(e) => tracing::warn!(err = %e, "codebrain: failed to serialise {label}"),
    }
}

// ── CodeGraph clone helper ────────────────────────────────────────────────────
// CodeGraph doesn't derive Clone because petgraph::DiGraph is large but cloneable.
// We implement it manually so registry can unwrap Arc<CodeGraph> when needed.

impl Clone for CodeGraph {
    fn clone(&self) -> Self {
        CodeGraph::from_store(self.to_store())
    }
}
