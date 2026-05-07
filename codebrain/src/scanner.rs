//! Full project scan pipeline.
//!
//! `scan()` orchestrates all extraction stages and returns a `ScanResult`
//! (flat summary, backward-compatible) plus a `CodeGraph` (rich in-memory
//! graph) as a tuple.
//!
//! # Pipeline order
//! 1. Collect files  (`collector`)
//! 2. Detect language, frameworks, ORMs  (`collector` + `detectors`)
//! 3. Detect routes, schemas, env vars  (`detectors`)
//! 4. Build dependency graph  (`graph`)            — existing flat graph
//! 5. AST extraction → ExtractionResult per file  (`extract::ast`)
//! 6. Semantic extraction → ExtractionResult per doc file  (`extract::semantic`)
//! 7. Assemble CodeGraph from all results + detector overlays  (`build`)
//! 8. Assign communities  (`cluster`)              — no-op until Louvain lands
//! 9. Compute token stats + project info  → ScanResult

use crate::cache::ExtractionCache;
use crate::cluster;
use crate::collector;
use crate::config::CodeBrainConfig;
use crate::extract::semantic::SemanticExtractor;
use crate::extract::{ast, semantic};
use crate::formatter;
use crate::model::CodeGraph;
use crate::types::{ProjectInfo, ScanResult};
use crate::{build, detectors, graph};

/// Run a full scan. Returns `(ScanResult, CodeGraph)`.
///
/// `ScanResult` is the flat, serialisable summary used for backward-compatible
/// disk caching and the existing prompt formatter.
///
/// `CodeGraph` is the in-memory knowledge graph used for BFS/DFS queries and
/// the enriched graph context section.
///
/// This function is synchronous and CPU-bound. Call it from
/// `tokio::task::spawn_blocking` in the registry / handle.
pub fn scan(
    config:    &CodeBrainConfig,
    extractor: Option<&dyn SemanticExtractor>,
) -> anyhow::Result<(ScanResult, CodeGraph)> {
    let root = &config.project_root;
    if !root.exists() {
        anyhow::bail!("project root does not exist: {}", root.display());
    }

    tracing::debug!(root = %root.display(), "codebrain scan started");

    // ── 1. Collect files ──────────────────────────────────────────────────────
    let files     = collector::collect_files(config);
    let language  = collector::detect_language(&files);

    // ── 2. Framework / ORM detection ─────────────────────────────────────────
    let frameworks = detectors::routes::detect_frameworks(&files);
    let orms       = detectors::schema::detect_orms(&files);

    // ── 3. Route / schema / env detection (flat detector pass) ───────────────
    let routes  = detectors::routes::detect_all(&files, &frameworks);
    let schemas = detectors::schema::detect_all(&files, &orms);
    let env     = detectors::env::detect_all(&files);

    // ── 4. Flat dependency graph (backward-compat) ────────────────────────────
    let dep_graph = graph::build(&files);

    // ── 5. AST extraction ─────────────────────────────────────────────────────
    let ast_results = if config.enable_ast_extraction {
        let cache_dir = ExtractionCache::default_location();
        let cache     = ExtractionCache::open(cache_dir);
        ast::extract_all(&files, &cache)
    } else {
        Vec::new()
    };

    // ── 6. Semantic extraction (doc files) ────────────────────────────────────
    let semantic_results = if config.enable_semantic_extraction {
        let doc_files = semantic::doc_files(&files);
        semantic::extract_docs(&doc_files, extractor)
    } else {
        Vec::new()
    };

    // ── 7. Assemble CodeGraph ─────────────────────────────────────────────────
    let mut code_graph = build::build(ast_results, semantic_results, &routes, &schemas);

    // ── 8. Community assignment (stub — see cluster.rs) ───────────────────────
    cluster::assign_communities(&mut code_graph);

    // ── 9. Wrap up flat ScanResult ────────────────────────────────────────────
    let mut token_stats = formatter::estimate_tokens(&routes, &schemas, &dep_graph);
    token_stats.file_count = files.len();

    let project = ProjectInfo {
        root: root.clone(),
        name: project_name(root),
        frameworks,
        orms,
        language,
    };

    tracing::debug!(
        files   = files.len(),
        routes  = routes.len(),
        schemas = schemas.len(),
        nodes   = code_graph.node_count(),
        edges   = code_graph.edge_count(),
        "codebrain scan complete"
    );

    let scan_result = ScanResult {
        project,
        routes,
        schemas,
        graph: dep_graph,
        env_vars: env,
        token_stats,
        scanned_at: chrono::Utc::now(),
    };

    Ok((scan_result, code_graph))
}

// ── Project name helpers ──────────────────────────────────────────────────────

fn project_name(root: &std::path::Path) -> String {
    if let Some(name) = read_cargo_name(root)       { return name; }
    if let Some(name) = read_package_json_name(root) { return name; }
    root.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string()
}

fn read_cargo_name(root: &std::path::Path) -> Option<String> {
    let content = std::fs::read_to_string(root.join("Cargo.toml")).ok()?;
    for line in content.lines() {
        let t = line.trim();
        if t.starts_with("name") && t.contains('=') {
            let val = t.splitn(2, '=').nth(1)?.trim().trim_matches('"').trim_matches('\'');
            if !val.is_empty() { return Some(val.to_string()); }
        }
    }
    None
}

fn read_package_json_name(root: &std::path::Path) -> Option<String> {
    let content = std::fs::read_to_string(root.join("package.json")).ok()?;
    let needle  = "\"name\"";
    let pos     = content.find(needle)?;
    let after   = &content[pos + needle.len()..];
    let colon   = after.find(':')? + 1;
    let val     = after[colon..].trim().trim_start_matches('"');
    let end     = val.find('"').unwrap_or(val.len());
    let name    = val[..end].trim();
    if name.is_empty() { None } else { Some(name.to_string()) }
}
