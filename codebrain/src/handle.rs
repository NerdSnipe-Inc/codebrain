//! Cheap-to-clone handle to a project's CodeBrain state.
//!
//! Stores two caches behind `Arc<RwLock>`:
//!   - `scan_cache`  — the flat `ScanResult` (routes, schemas, env vars, dep graph)
//!   - `graph_cache` — the in-memory `CodeGraph` (AST nodes, edges, BFS queries)
//!
//! The public API is unchanged from v1: `scan()`, `force_scan()`, `context_block()`,
//! `blast_radius()`, `routes()`, `schemas()`, `cached()`.
//!
//! New in v2:
//!   - `graph_query(query, max_tokens)` — targeted BFS subgraph context
//!   - `with_semantic_extractor(extractor)` — inject LLM extraction at runtime
//!   - `graph()` — direct access to the `Arc<CodeGraph>` for advanced queries

use std::sync::{Arc, RwLock};

use crate::config::CodeBrainConfig;
use crate::extract::semantic::SemanticExtractor;
use crate::formatter;
use crate::model::CodeGraph;
use crate::query;
use crate::types::{BlastRadius, RouteInfo, ScanResult, SchemaModel, SymbolCallers};

/// Cheap-to-clone handle to the CodeBrain scan + graph cache.
#[derive(Clone)]
pub struct CodeBrainHandle {
    config:      CodeBrainConfig,
    scan_cache:  Arc<RwLock<Option<ScanResult>>>,
    graph_cache: Arc<RwLock<Option<Arc<CodeGraph>>>>,
    /// Optional injected semantic extractor.
    /// Set via `with_semantic_extractor()` before the first scan.
    extractor:   Option<Arc<dyn SemanticExtractor>>,
}

impl CodeBrainHandle {
    /// Create a cold handle (no cached data).
    pub fn new(config: CodeBrainConfig) -> Self {
        Self {
            config,
            scan_cache:  Arc::new(RwLock::new(None)),
            graph_cache: Arc::new(RwLock::new(None)),
            extractor:   None,
        }
    }

    /// Create a handle pre-seeded with a cached scan result (e.g. loaded from disk).
    pub fn new_with_cache(config: CodeBrainConfig, cached: ScanResult) -> Self {
        Self {
            config,
            scan_cache:  Arc::new(RwLock::new(Some(cached))),
            graph_cache: Arc::new(RwLock::new(None)),
            extractor:   None,
        }
    }

    /// Create a handle pre-seeded with both flat result and graph (full warm-start).
    pub fn new_with_full_cache(
        config: CodeBrainConfig,
        scan:   ScanResult,
        graph:  CodeGraph,
    ) -> Self {
        Self {
            config,
            scan_cache:  Arc::new(RwLock::new(Some(scan))),
            graph_cache: Arc::new(RwLock::new(Some(Arc::new(graph)))),
            extractor:   None,
        }
    }

    /// Attach an LLM semantic extractor. Must be called before the first scan
    /// for semantic extraction to take effect.
    pub fn with_semantic_extractor(mut self, extractor: Arc<dyn SemanticExtractor>) -> Self {
        self.extractor = Some(extractor);
        self
    }

    // ── Core scan API (backward-compatible) ───────────────────────────────────

    /// Return the flat `ScanResult`, using the cache if fresh.
    pub fn scan(&self) -> anyhow::Result<ScanResult> {
        if let Ok(guard) = self.scan_cache.read() {
            if let Some(ref cached) = *guard {
                let age = chrono::Utc::now()
                    .signed_duration_since(cached.scanned_at)
                    .num_seconds() as u64;
                if self.config.rescan_interval_secs > 0
                    && age < self.config.rescan_interval_secs
                {
                    return Ok(cached.clone());
                }
            }
        }
        self.force_scan()
    }

    /// Force an immediate rescan, bypassing the cache interval.
    pub fn force_scan(&self) -> anyhow::Result<ScanResult> {
        let extractor = self.extractor.as_deref();
        let (result, graph) = crate::scanner::scan(&self.config, extractor)?;

        if let Ok(mut guard) = self.scan_cache.write() {
            *guard = Some(result.clone());
        }
        if let Ok(mut guard) = self.graph_cache.write() {
            *guard = Some(Arc::new(graph));
        }

        Ok(result)
    }

    // ── Context block (extended) ──────────────────────────────────────────────

    /// Return the full context block for prompt injection.
    ///
    /// The block consists of:
    ///   1. Flat summary (routes, schemas, hot files, env vars) — from v1
    ///   2. Graph intelligence section (god nodes, stats) — new in v2,
    ///      appended only when the graph has data.
    ///
    /// Returns an empty string if the scan fails.
    pub fn context_block(&self) -> String {
        let result = match self.scan() {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(err = %e, "codebrain scan failed — skipping context block");
                return String::new();
            }
        };

        let flat = formatter::format_context_block(&result, self.config.context_token_budget);

        let graph_section = self
            .graph_cache
            .read()
            .ok()
            .and_then(|g| g.clone())
            .map(|g| formatter::format_graph_section(&g, self.config.context_token_budget))
            .unwrap_or_default();

        if graph_section.is_empty() {
            flat
        } else {
            format!("{}\n\n{}", flat, graph_section)
        }
    }

    // ── Graph query (new in v2) ───────────────────────────────────────────────

    /// BFS subgraph context for a natural-language query.
    ///
    /// Triggers a scan if no graph is cached. Returns an empty string if the
    /// scan fails or AST extraction is disabled.
    ///
    /// # Example
    /// ```ignore
    /// let ctx = handle.graph_query("authentication flow", 1500);
    /// // ctx contains the BFS subgraph starting from auth-related nodes,
    /// // trimmed to ~1500 tokens.
    /// ```
    pub fn graph_query(&self, query: &str, max_tokens: usize) -> String {
        // Trigger scan to populate graph if needed
        if let Err(e) = self.scan() {
            tracing::warn!(err = %e, "codebrain: graph_query scan failed");
            return String::new();
        }

        let graph = match self.graph_cache.read().ok().and_then(|g| g.clone()) {
            Some(g) => g,
            None    => return String::new(),
        };

        query::bfs_context(&graph, query, max_tokens).context
    }

    /// DFS variant of `graph_query` — better for tracing call chains.
    pub fn graph_query_dfs(&self, query: &str, max_tokens: usize) -> String {
        if let Err(e) = self.scan() {
            tracing::warn!(err = %e, "codebrain: graph_query_dfs scan failed");
            return String::new();
        }

        let graph = match self.graph_cache.read().ok().and_then(|g| g.clone()) {
            Some(g) => g,
            None    => return String::new(),
        };

        query::dfs_context(&graph, query, max_tokens).context
    }

    // ── Blast radius (improved in v2) ─────────────────────────────────────────

    /// Blast radius for a set of changed source files.
    ///
    /// Uses the flat dep graph from `ScanResult` (same as v1 — fast, reliable).
    pub fn blast_radius(&self, files: &[String], max_depth: usize) -> anyhow::Result<BlastRadius> {
        let result = self.scan()?;
        Ok(crate::graph::blast_radius(
            files,
            &result.graph,
            &result.routes,
            &result.schemas,
            max_depth,
        ))
    }

    /// Symbol-level caller query.
    ///
    /// Walks `Calls` edges backwards from the best-matching node for `symbol`
    /// up to `max_depth` hops. More precise than `blast_radius` when you know
    /// the specific function or type being changed.
    pub fn symbol_callers(&self, symbol: &str, max_depth: usize) -> Option<SymbolCallers> {
        if let Err(e) = self.scan() {
            tracing::warn!(err = %e, "codebrain: symbol_callers scan failed");
            return None;
        }
        let graph = self.graph()?;
        let routes = self.cached().map(|r| r.routes).unwrap_or_default();
        crate::analyze::symbol_callers(&graph, symbol, max_depth, &routes)
    }

    // ── Filtered query helpers (unchanged from v1) ────────────────────────────

    /// Filtered route list by HTTP method and/or path prefix.
    pub fn routes(
        &self,
        method:      Option<&str>,
        path_prefix: Option<&str>,
    ) -> anyhow::Result<Vec<RouteInfo>> {
        let result = self.scan()?;
        Ok(result
            .routes
            .into_iter()
            .filter(|r| method.map(|m| r.method.eq_ignore_ascii_case(m)).unwrap_or(true))
            .filter(|r| path_prefix.map(|p| r.path.starts_with(p)).unwrap_or(true))
            .collect())
    }

    /// Filtered schema list by model name.
    pub fn schemas(&self, model_name: Option<&str>) -> anyhow::Result<Vec<SchemaModel>> {
        let result = self.scan()?;
        Ok(result
            .schemas
            .into_iter()
            .filter(|s| {
                model_name
                    .map(|n| s.name.eq_ignore_ascii_case(n))
                    .unwrap_or(true)
            })
            .collect())
    }

    // ── Cache access ──────────────────────────────────────────────────────────

    /// Return the last cached `ScanResult` without triggering a scan.
    pub fn cached(&self) -> Option<ScanResult> {
        self.scan_cache.read().ok()?.clone()
    }

    /// Return the last cached `CodeGraph` without triggering a scan.
    pub fn graph(&self) -> Option<Arc<CodeGraph>> {
        self.graph_cache.read().ok()?.clone()
    }

    pub fn config(&self) -> &CodeBrainConfig {
        &self.config
    }

    // ── Community detection ───────────────────────────────────────────────────

    /// Return Louvain community summaries and cross-community edges.
    ///
    /// Returns `None` if no graph is cached (call `scan()` first).
    pub fn communities(
        &self,
    ) -> Option<(Vec<crate::cluster::CommunitySummary>, Vec<crate::analyze::SurprisingEdge>)> {
        let graph = self.graph()?;
        let summaries = crate::cluster::community_summary(&graph);
        let edges = crate::analyze::surprising_connections(&graph);
        Some((summaries, edges))
    }
}
