//! MCP tool implementations — one function per exposed tool.
//!
//! Every tool receives the raw JSON `arguments` object and a shared
//! `CodeBrainHandle`, and returns a `ToolResult` (text content + isError flag).
//!
//! ## Available tools
//!
//! | Name                    | Purpose                                             |
//! |-------------------------|-----------------------------------------------------|
//! | `codebrain_scan`        | Force-rescan project; populate graph                |
//! | `codebrain_context`     | BFS context block for a natural-language query      |
//! | `codebrain_context_dfs` | DFS context block — traces call chains              |
//! | `codebrain_blast_radius`| Files affected if a given path changes              |
//! | `codebrain_routes`      | All detected HTTP routes                            |
//! | `codebrain_schemas`     | All detected database schemas / models              |
//! | `codebrain_god_nodes`   | Top-N most-connected nodes (architectural hot spots)|

use serde_json::{json, Value};

use codebrain::CodeBrainHandle;

use crate::protocol::ToolResult;

// ── Tool registry ─────────────────────────────────────────────────────────────

/// JSON Schema definitions returned to the client via `tools/list`.
pub fn definitions() -> Value {
    json!([
        {
            "name": "codebrain_scan",
            "description": "Scan the project and build the knowledge graph. Runs automatically on first use of any other tool; call this explicitly to force a re-scan after code changes.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": []
            }
        },
        {
            "name": "codebrain_context",
            "description": "BFS query: returns a token-budgeted context block anchored to codebase nodes that match the query. Best for understanding a feature area, module, or component neighbourhood.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Natural language query — e.g. 'authentication flow', 'payment module', 'database layer'"
                    },
                    "max_tokens": {
                        "type": "integer",
                        "description": "Token budget for the returned context block (default: 2000)"
                    }
                },
                "required": ["query"]
            }
        },
        {
            "name": "codebrain_context_dfs",
            "description": "DFS query: traverses call chains depth-first from matching nodes. Better than BFS for tracing how control flows through the codebase.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Natural language query — e.g. 'request handler pipeline', 'error propagation path'"
                    },
                    "max_tokens": {
                        "type": "integer",
                        "description": "Token budget for the returned context block (default: 2000)"
                    }
                },
                "required": ["query"]
            }
        },
        {
            "name": "codebrain_blast_radius",
            "description": "Returns all files and components affected if the specified file changes. Essential context before modifying a shared utility, core interface, or model.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative file path to analyse (e.g. 'src/auth.rs', 'lib/database.ts')"
                    },
                    "depth": {
                        "type": "integer",
                        "description": "Traversal depth — how many hops of dependents to include (default: 3)"
                    }
                },
                "required": ["path"]
            }
        },
        {
            "name": "codebrain_routes",
            "description": "Returns all detected HTTP routes with method, path, handler, and framework. Optionally filter by HTTP method or path prefix.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "method": {
                        "type": "string",
                        "description": "Filter by HTTP method (GET, POST, PUT, DELETE, PATCH). Omit for all methods."
                    },
                    "path_prefix": {
                        "type": "string",
                        "description": "Filter to routes whose path starts with this prefix (e.g. '/api/v2'). Omit for all routes."
                    }
                },
                "required": []
            }
        },
        {
            "name": "codebrain_schemas",
            "description": "Returns all detected database schemas and ORM models with their fields and relations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "model_name": {
                        "type": "string",
                        "description": "Filter to a specific model by name (case-insensitive). Omit for all schemas."
                    }
                },
                "required": []
            }
        },
        {
            "name": "codebrain_god_nodes",
            "description": "Returns the top-N most-connected nodes in the knowledge graph — the load-bearing abstractions every agent should know about before making changes.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "n": {
                        "type": "integer",
                        "description": "Number of nodes to return (default: 10, max: 50)"
                    }
                },
                "required": []
            }
        },
        {
            "name": "codebrain_symbol_callers",
            "description": "Returns all functions and methods that call a specific symbol, walking the call graph up to max_depth hops. More precise than codebrain_blast_radius when you know the specific function or type being changed — only surfaces callers of that symbol, not every importer of its file.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "symbol": {
                        "type": "string",
                        "description": "Symbol name to look up — partial, case-insensitive match against node labels (e.g. 'authenticate', 'DatabasePool', 'process_payment')"
                    },
                    "depth": {
                        "type": "integer",
                        "description": "How many hops of callers to include (default: 3). depth=1 returns direct callers only."
                    }
                },
                "required": ["symbol"]
            }
        },
        {
            "name": "codebrain_communities",
            "description": "Returns the community structure of the knowledge graph — clusters of tightly-connected nodes detected by the Louvain algorithm. Also surfaces cross-community edges (surprising connections between otherwise separate modules).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "min_size": {
                        "type": "integer",
                        "description": "Only show communities with at least this many nodes (default: 2)"
                    }
                },
                "required": []
            }
        }
    ])
}

// ── Dispatcher ────────────────────────────────────────────────────────────────

pub fn call(name: &str, args: &Value, handle: &CodeBrainHandle) -> ToolResult {
    match name {
        "codebrain_scan"         => tool_scan(handle),
        "codebrain_context"      => tool_context(args, handle),
        "codebrain_context_dfs"  => tool_context_dfs(args, handle),
        "codebrain_blast_radius" => tool_blast_radius(args, handle),
        "codebrain_routes"       => tool_routes(args, handle),
        "codebrain_schemas"      => tool_schemas(args, handle),
        "codebrain_god_nodes"    => tool_god_nodes(args, handle),
        "codebrain_symbol_callers" => tool_symbol_callers(args, handle),
        "codebrain_communities"    => tool_communities(args, handle),
        _                          => ToolResult::error(format!("Unknown tool: {name}")),
    }
}

// ── Tool implementations ──────────────────────────────────────────────────────

fn tool_scan(handle: &CodeBrainHandle) -> ToolResult {
    match handle.force_scan() {
        Ok(result) => {
            let project = &result.project.name;
            let files   = result.token_stats.file_count;
            let routes  = result.routes.len();
            let schemas = result.schemas.len();
            ToolResult::text(format!(
                "Scan complete.\n\
                 Project: {project}\n\
                 Files scanned: {files}\n\
                 Routes detected: {routes}\n\
                 Schemas detected: {schemas}\n\
                 Graph nodes: {nodes}\n\
                 Graph edges: {edges}",
                nodes = handle.graph().map(|g| g.node_count()).unwrap_or(0),
                edges = handle.graph().map(|g| g.edge_count()).unwrap_or(0),
            ))
        }
        Err(e) => ToolResult::error(format!("Scan failed: {e:#}")),
    }
}

fn tool_context(args: &Value, handle: &CodeBrainHandle) -> ToolResult {
    let query = match args.get("query").and_then(Value::as_str) {
        Some(q) => q.to_string(),
        None    => return ToolResult::error("Missing required argument: query"),
    };
    let max_tokens = args.get("max_tokens").and_then(Value::as_u64).unwrap_or(2000) as usize;

    let ctx = handle.graph_query(&query, max_tokens);
    if ctx.is_empty() {
        ToolResult::text("No matching context found. The project may not have been scanned yet, or no nodes match the query. Try calling codebrain_scan first.")
    } else {
        ToolResult::text(ctx)
    }
}

fn tool_context_dfs(args: &Value, handle: &CodeBrainHandle) -> ToolResult {
    let query = match args.get("query").and_then(Value::as_str) {
        Some(q) => q.to_string(),
        None    => return ToolResult::error("Missing required argument: query"),
    };
    let max_tokens = args.get("max_tokens").and_then(Value::as_u64).unwrap_or(2000) as usize;

    let ctx = handle.graph_query_dfs(&query, max_tokens);
    if ctx.is_empty() {
        ToolResult::text("No matching context found. The project may not have been scanned yet, or no nodes match the query. Try calling codebrain_scan first.")
    } else {
        ToolResult::text(ctx)
    }
}

fn tool_blast_radius(args: &Value, handle: &CodeBrainHandle) -> ToolResult {
    let path = match args.get("path").and_then(Value::as_str) {
        Some(p) => p.to_string(),
        None    => return ToolResult::error("Missing required argument: path"),
    };
    let depth = args.get("depth").and_then(Value::as_u64).unwrap_or(3) as usize;

    match handle.blast_radius(&[path.clone()], depth) {
        Ok(br) => {
            if br.affected_files.is_empty() && br.affected_routes.is_empty() {
                return ToolResult::text(format!(
                    "No dependents found for '{path}' within depth {depth}.\n\
                     This file may be a leaf node, or the project needs a re-scan."
                ));
            }

            let mut out = vec![
                format!("Blast radius for '{path}' (depth {depth}):\n"),
                format!("Affected files ({}):", br.affected_files.len()),
            ];
            for f in &br.affected_files {
                out.push(format!("  {f}"));
            }
            if !br.affected_routes.is_empty() {
                out.push(format!("\nAffected routes ({}):", br.affected_routes.len()));
                for r in &br.affected_routes {
                    out.push(format!("  {} {} ({})", r.method, r.path, r.file));
                }
            }
            if !br.affected_models.is_empty() {
                out.push(format!("\nAffected models ({}):", br.affected_models.len()));
                for m in &br.affected_models {
                    out.push(format!("  {m}"));
                }
            }
            ToolResult::text(out.join("\n"))
        }
        Err(e) => ToolResult::error(format!("Blast radius query failed: {e:#}")),
    }
}

fn tool_routes(args: &Value, handle: &CodeBrainHandle) -> ToolResult {
    let method      = args.get("method").and_then(Value::as_str);
    let path_prefix = args.get("path_prefix").and_then(Value::as_str);

    match handle.routes(method, path_prefix) {
        Ok(routes) => {
            if routes.is_empty() {
                let filter_desc = match (method, path_prefix) {
                    (Some(m), Some(p)) => format!(" matching method={m} prefix={p}"),
                    (Some(m), None)    => format!(" for method={m}"),
                    (None, Some(p))    => format!(" with prefix={p}"),
                    (None, None)       => String::new(),
                };
                return ToolResult::text(format!(
                    "No routes detected{filter_desc}. \
                     Ensure the project has been scanned and contains supported framework code \
                     (Axum, Actix, Express, Fastify, Gin, FastAPI, etc.)."
                ));
            }

            let mut lines = vec![format!("{} route(s) detected:\n", routes.len())];
            for r in &routes {
                lines.push(format!(
                    "  {method:6} {path:<40} [{framework:?}]  {file}",
                    method   = r.method,
                    path     = r.path,
                    framework = r.framework,
                    file     = r.file,
                ));
            }
            ToolResult::text(lines.join("\n"))
        }
        Err(e) => ToolResult::error(format!("Routes query failed: {e:#}")),
    }
}

fn tool_schemas(args: &Value, handle: &CodeBrainHandle) -> ToolResult {
    let model_name = args.get("model_name").and_then(Value::as_str);

    match handle.schemas(model_name) {
        Ok(schemas) => {
            if schemas.is_empty() {
                return ToolResult::text(
                    "No schemas detected. Ensure the project has been scanned and contains \
                     supported ORM code (SeaORM, Prisma, SQLAlchemy, Drizzle, TypeORM, etc.)."
                        .to_string(),
                );
            }

            let mut lines = vec![format!("{} schema(s) detected:\n", schemas.len())];
            for s in &schemas {
                let table = s.table_name.as_deref().unwrap_or("(no table)");
                lines.push(format!("  {} [{:?}]  table: {}", s.name, s.orm, table));
                if !s.fields.is_empty() {
                    let field_names: Vec<&str> = s.fields.iter().map(|f| f.name.as_str()).collect();
                    lines.push(format!("    fields: {}", field_names.join(", ")));
                }
                if !s.relations.is_empty() {
                    lines.push(format!("    relations: {}", s.relations.join(", ")));
                }
            }
            ToolResult::text(lines.join("\n"))
        }
        Err(e) => ToolResult::error(format!("Schemas query failed: {e:#}")),
    }
}

fn tool_god_nodes(args: &Value, handle: &CodeBrainHandle) -> ToolResult {
    let n = args
        .get("n")
        .and_then(Value::as_u64)
        .unwrap_or(10)
        .min(50) as usize;

    // Ensure graph is populated before querying
    if let Err(e) = handle.scan() {
        return ToolResult::error(format!("Scan failed: {e:#}"));
    }

    let graph = match handle.graph() {
        Some(g) => g,
        None    => return ToolResult::text(
            "No knowledge graph available. AST extraction may be disabled \
             or the project has not been scanned yet."
        ),
    };

    let nodes = graph.god_nodes(n);
    if nodes.is_empty() {
        return ToolResult::text(
            "No nodes in the knowledge graph yet. Call codebrain_scan first."
        );
    }

    let mut lines = vec![
        format!("Top {} most-connected nodes (architectural hot spots):\n", nodes.len()),
    ];
    for (node, degree) in &nodes {
        lines.push(format!(
            "  [{type_:?}] {label}  ({degree} edges)\n    {file}:{line}",
            type_  = node.node_type,
            label  = node.label,
            degree = degree,
            file   = node.file,
            line   = node.line,
        ));
    }
    lines.push(String::new());
    lines.push(format!(
        "Total graph: {} nodes, {} edges",
        graph.node_count(),
        graph.edge_count(),
    ));

    ToolResult::text(lines.join("\n"))
}

fn tool_symbol_callers(args: &Value, handle: &CodeBrainHandle) -> ToolResult {
    let symbol = match args.get("symbol").and_then(Value::as_str) {
        Some(s) => s.to_string(),
        None    => return ToolResult::error("Missing required argument: symbol"),
    };
    let depth = args.get("depth").and_then(Value::as_u64).unwrap_or(3) as usize;

    match handle.symbol_callers(&symbol, depth) {
        None => ToolResult::text(format!(
            "No symbol matching '{symbol}' found in the knowledge graph.\n\
             Try a shorter or different term, or call codebrain_scan first.\n\
             Tip: use codebrain_context to explore what's in the graph."
        )),
        Some(sc) => {
            let mut lines = vec![
                format!(
                    "Callers of '{}' [{} {}:{}] (depth {}):\n",
                    sc.target_label, sc.target_file, sc.target_file, sc.target_line, depth
                ),
            ];

            if sc.callers.is_empty() {
                lines.push("  No callers found — this symbol may be a top-level entry point.".to_string());
            } else {
                let mut last_depth = 0;
                for c in &sc.callers {
                    if c.depth != last_depth {
                        lines.push(format!("\nDepth {} callers:", c.depth));
                        last_depth = c.depth;
                    }
                    lines.push(format!("  [{}:{}]  {}", c.file, c.line, c.label));
                }
            }

            if !sc.affected_files.is_empty() {
                lines.push(String::new());
                lines.push(format!("Affected files ({}):", sc.affected_files.len()));
                for f in &sc.affected_files {
                    lines.push(format!("  {f}"));
                }
            }

            ToolResult::text(lines.join("\n"))
        }
    }
}

fn tool_communities(args: &Value, handle: &CodeBrainHandle) -> ToolResult {
    let min_size = args.get("min_size").and_then(Value::as_u64).unwrap_or(2) as usize;

    if let Err(e) = handle.scan() {
        return ToolResult::error(format!("Scan failed: {e:#}"));
    }

    let (summaries, edges) = match handle.communities() {
        Some(data) => data,
        None       => return ToolResult::text(
            "No knowledge graph available. AST extraction may be disabled \
             or the project has not been scanned yet."
        ),
    };

    let filtered: Vec<_> = summaries.iter().filter(|c| c.size >= min_size).collect();

    if filtered.is_empty() {
        return ToolResult::text(
            "Community clustering has not run or the graph is empty. \
             Try calling codebrain_scan first."
        );
    }

    let mut lines = vec![
        format!("Communities detected by Louvain algorithm ({} total):\n", filtered.len()),
    ];
    for c in &filtered {
        lines.push(format!("  community_{} ({} nodes) — {}", c.id, c.size, c.label));
    }

    if !edges.is_empty() {
        lines.push(String::new());
        lines.push(format!("Cross-community connections ({} total):", edges.len()));
        for e in &edges {
            lines.push(format!(
                "  {} [{}] → {} [{}]",
                e.from_label, e.from_file, e.to_label, e.to_file,
            ));
        }
    }

    ToolResult::text(lines.join("\n"))
}
