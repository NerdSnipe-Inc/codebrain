//! Context block formatting for agent prompt injection.
//!
//! Two public functions:
//!
//! - `format_context_block(&ScanResult, budget)` — unchanged from v1.
//!   Produces the flat route/schema/hot-files/env summary. Existing tests
//!   and harness injection code call this directly.
//!
//! - `format_graph_section(&CodeGraph, budget)` — new in v2.
//!   Produces a compact graph-intelligence header: node/edge counts, top god
//!   nodes, and a note that BFS queries are available for deeper context.
//!   The `CodeBrainHandle::context_block()` appends this after the flat block.

use crate::analyze;
use crate::model::CodeGraph;
use crate::types::{DependencyGraph, Framework, RouteInfo, ScanResult, SchemaModel, TokenStats};

// ── Flat context block (unchanged public API) ─────────────────────────────────

/// Generate the flat context block injected into agent system prompts.
/// Output is trimmed to stay within the configured token budget.
/// Signature is unchanged from v1 — existing tests pass through here.
pub fn format_context_block(result: &ScanResult, budget: usize) -> String {
    let mut lines: Vec<String> = Vec::new();

    let framework = result.project.frameworks.first().unwrap_or(&Framework::Unknown);

    lines.push(format!(
        "== Codebase: {} ({} / {}) ==",
        result.project.name,
        result.project.language,
        framework,
    ));

    // Routes summary
    lines.push(format!("Routes ({}):", result.routes.len()));
    for route in result.routes.iter().take(15) {
        lines.push(format!("  {} {} [{}]", route.method, route.path, route.file));
    }
    if result.routes.len() > 15 {
        lines.push(format!("  ... and {} more", result.routes.len() - 15));
    }

    // Schema summary
    lines.push(format!("Schemas ({}):", result.schemas.len()));
    for schema in result.schemas.iter().take(10) {
        let fields: Vec<&str> = schema.fields.iter().take(5).map(|f| f.name.as_str()).collect();
        if fields.is_empty() {
            lines.push(format!("  {}", schema.name));
        } else {
            lines.push(format!("  {} {{ {} }}", schema.name, fields.join(", ")));
        }
    }
    if result.schemas.len() > 10 {
        lines.push(format!("  ... and {} more", result.schemas.len() - 10));
    }

    // Hot files
    if !result.graph.hot_files.is_empty() {
        lines.push("Hot files (most imported):".to_string());
        for f in result.graph.hot_files.iter().take(5) {
            lines.push(format!("  {} (imported by {} files)", f.file, f.imported_by));
        }
    }

    // Env vars
    if !result.env_vars.is_empty() {
        let names: Vec<&str> = result.env_vars.iter().map(|e| e.name.as_str()).collect();
        lines.push(format!("Env vars: {}", names.join(", ")));
    }

    lines.push(format!(
        "Scanned: {} | {} files | ~{} tokens",
        result.scanned_at.format("%H:%M UTC"),
        result.token_stats.file_count,
        result.token_stats.estimated_context_tokens,
    ));
    lines.push("== End codebase context ==".to_string());

    // Trim to token budget (rough: 1 token ≈ 4 chars)
    let budget_chars = budget * 4;
    let full = lines.join("\n");
    if full.len() <= budget_chars {
        full
    } else {
        let truncated = &full[..budget_chars];
        format!("{}\n... (truncated to token budget)", truncated)
    }
}

/// Estimate token counts for the flat context block.
pub fn estimate_tokens(
    routes:  &[RouteInfo],
    schemas: &[SchemaModel],
    graph:   &DependencyGraph,
) -> TokenStats {
    let route_tokens  = routes.len().min(15) * 15;
    let schema_tokens = schemas.len().min(10) * 20;
    let hot_tokens    = graph.hot_files.len().min(5) * 10;
    let base          = 40;

    let estimated = base + route_tokens + schema_tokens + hot_tokens;

    TokenStats {
        estimated_context_tokens: estimated,
        route_count:   routes.len(),
        schema_count:  schemas.len(),
        file_count:    0, // filled in by scanner
        hot_file_count: graph.hot_files.len(),
    }
}

// ── Graph intelligence section (new in v2) ────────────────────────────────────

/// Generate a compact graph-intelligence section to append after the flat block.
///
/// Example output:
/// ```text
/// == Knowledge Graph: 342 nodes, 891 edges ==
/// Key abstractions (by connections):
///   AuthService [src/auth/service.rs:12] — 18 connections
///   UserRepository [src/user/repository.rs:5] — 14 connections
///   DatabasePool [src/db/pool.rs:3] — 11 connections
/// Use graph queries for targeted context: handle.graph_query("auth flow", 1000)
/// == End graph context ==
/// ```
///
/// Returns an empty string when the graph has no nodes (AST extraction
/// disabled or project has no recognised source files).
pub fn format_graph_section(graph: &CodeGraph, budget: usize) -> String {
    if graph.node_count() == 0 {
        return String::new();
    }

    let stats    = analyze::graph_stats(graph);
    let gods     = analyze::god_nodes(graph, 8);

    let mut lines: Vec<String> = Vec::new();

    lines.push(format!(
        "== Knowledge Graph: {} nodes, {} edges ({} fns, {} routes, {} schemas) ==",
        stats.node_count,
        stats.edge_count,
        stats.function_count,
        stats.route_count,
        stats.schema_count,
    ));

    if !gods.is_empty() {
        lines.push("Key abstractions (by connections):".to_string());
        for g in &gods {
            let loc = if g.line > 0 {
                format!("{}:{}", g.file, g.line)
            } else {
                g.file.clone()
            };
            lines.push(format!(
                "  {} [{}] — {} connections",
                g.label, loc, g.degree
            ));
        }
    }

    // Community summary (empty until Louvain is implemented)
    let communities = crate::cluster::community_summary(graph);
    if !communities.is_empty() {
        lines.push("Communities:".to_string());
        for c in communities.iter().take(5) {
            lines.push(format!("  {} ({} nodes)", c.label, c.size));
        }
    }

    lines.push(
        "Use handle.graph_query(\"<topic>\", tokens) for targeted subgraph context.".to_string()
    );
    lines.push("== End graph context ==".to_string());

    // Trim to half the budget (the flat block gets the other half)
    let budget_chars = (budget / 2) * 4;
    let full = lines.join("\n");
    if full.len() <= budget_chars {
        full
    } else {
        full[..budget_chars].to_string()
    }
}
