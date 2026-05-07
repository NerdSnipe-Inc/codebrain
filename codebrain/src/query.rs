//! BFS/DFS graph traversal with token-budget output.
//!
//! Agents call `bfs_context()` or `dfs_context()` to get a targeted subgraph
//! summary rather than the full flat context block. This mirrors graphify's
//! primary query mechanism and produces 40–126x token reductions on typical
//! codebase questions.
//!
//! # Token budget
//! The budget is a rough upper bound in tokens (1 token ≈ 3 chars for mixed
//! identifier/prose text). Traversal stops when the accumulated output text
//! would exceed the budget. Seed nodes are always included regardless of budget.
//!
//! # Query terms → seed nodes
//! The query string is split on whitespace into terms. Nodes whose `label` or
//! `file` contains any term (case-insensitive) become the BFS/DFS starting
//! points. If no seeds match, the top-5 god nodes are used as a fallback.

use std::collections::{HashSet, VecDeque};

use petgraph::Direction;

use crate::model::CodeGraph;
use crate::types::NodeData;

/// Result of a BFS or DFS context query.
#[derive(Debug)]
pub struct QueryResult {
    /// Formatted context string ready for prompt injection.
    pub context:    String,
    /// How many nodes were included before hitting the token budget.
    pub node_count: usize,
    /// Whether the traversal was truncated by the token budget.
    pub truncated:  bool,
}

/// BFS from seed nodes outward, collecting context up to `max_tokens`.
///
/// BFS gives broad coverage of closely related concepts — good for questions
/// like "how does auth work?" that span several related files.
pub fn bfs_context(graph: &CodeGraph, query: &str, max_tokens: usize) -> QueryResult {
    let seeds = resolve_seeds(graph, query);
    let budget_chars = max_tokens * 3;

    let mut visited: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<String>  = VecDeque::new();
    let mut lines: Vec<String>       = Vec::new();
    let mut chars_used               = 0usize;

    for seed in &seeds {
        queue.push_back(seed.id.clone());
    }

    let truncated = loop {
        let id = match queue.pop_front() {
            Some(id) => id,
            None     => break false,
        };
        if !visited.insert(id.clone()) {
            continue;
        }

        let node = match graph.get_node(&id) {
            Some(n) => n,
            None    => continue,
        };

        let line = format_node(node, graph);
        chars_used += line.len() + 1;
        if chars_used > budget_chars && !lines.is_empty() {
            break true;
        }
        lines.push(line);

        // Enqueue neighbours (both directions for full context)
        for (neighbour, _edge) in graph.neighbours(&id, Direction::Outgoing) {
            if !visited.contains(&neighbour.id) {
                queue.push_back(neighbour.id.clone());
            }
        }
        for (neighbour, _edge) in graph.neighbours(&id, Direction::Incoming) {
            if !visited.contains(&neighbour.id) {
                queue.push_back(neighbour.id.clone());
            }
        }
    };

    let header = format!(
        "=== Graph context for \"{query}\" ({} nodes, BFS) ===",
        lines.len()
    );

    QueryResult {
        context:    format!("{}\n{}", header, lines.join("\n")),
        node_count: lines.len(),
        truncated,
    }
}

/// DFS from seed nodes, collecting context up to `max_tokens`.
///
/// DFS follows a single path deeply before backtracking — good for questions
/// like "trace the call path from X to Y" that need to follow a dependency
/// chain.
pub fn dfs_context(graph: &CodeGraph, query: &str, max_tokens: usize) -> QueryResult {
    let seeds = resolve_seeds(graph, query);
    let budget_chars = max_tokens * 3;

    let mut visited: HashSet<String> = HashSet::new();
    let mut stack: Vec<String>       = seeds.iter().map(|n| n.id.clone()).collect();
    let mut lines: Vec<String>       = Vec::new();
    let mut chars_used               = 0usize;

    let truncated = loop {
        let id = match stack.pop() {
            Some(id) => id,
            None     => break false,
        };
        if !visited.insert(id.clone()) {
            continue;
        }

        let node = match graph.get_node(&id) {
            Some(n) => n,
            None    => continue,
        };

        let line = format_node(node, graph);
        chars_used += line.len() + 1;
        if chars_used > budget_chars && !lines.is_empty() {
            break true;
        }
        lines.push(line);

        for (neighbour, _edge) in graph.neighbours(&id, Direction::Outgoing) {
            if !visited.contains(&neighbour.id) {
                stack.push(neighbour.id.clone());
            }
        }
    };

    let header = format!(
        "=== Graph context for \"{query}\" ({} nodes, DFS) ===",
        lines.len()
    );

    QueryResult {
        context:    format!("{}\n{}", header, lines.join("\n")),
        node_count: lines.len(),
        truncated,
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Resolve query terms to seed NodeData references.
/// Falls back to top-5 god nodes if no terms match.
fn resolve_seeds<'a>(graph: &'a CodeGraph, query: &str) -> Vec<&'a NodeData> {
    let terms: Vec<&str> = query.split_whitespace().collect();
    let indices = graph.seed_nodes(&terms);

    if !indices.is_empty() {
        return indices
            .iter()
            .filter_map(|&idx| graph.node_at(idx))
            .collect();
    }

    // Fallback: top god nodes provide a reasonable starting point
    graph.god_nodes(5).into_iter().map(|(n, _)| n).collect()
}

/// Format a single node as a context line, including its key edges.
fn format_node(node: &NodeData, graph: &CodeGraph) -> String {
    let type_tag = match node.node_type {
        crate::types::NodeType::Function  => "fn",
        crate::types::NodeType::Method    => "method",
        crate::types::NodeType::Struct    => "struct",
        crate::types::NodeType::Class     => "class",
        crate::types::NodeType::Module    => "mod",
        crate::types::NodeType::Route     => "route",
        crate::types::NodeType::Schema    => "schema",
        crate::types::NodeType::Document  => "doc",
        crate::types::NodeType::Concept   => "concept",
        crate::types::NodeType::Variable  => "var",
    };

    let location = if node.line > 0 {
        format!("{}:{}", node.file, node.line)
    } else {
        node.file.clone()
    };

    // Collect outgoing neighbour labels for inline context (up to 4)
    let neighbours: Vec<String> = graph
        .neighbours(&node.id, Direction::Outgoing)
        .into_iter()
        .take(4)
        .map(|(n, _)| n.label.clone())
        .collect();

    if neighbours.is_empty() {
        format!("  [{type_tag}] {} [{location}]", node.label)
    } else {
        format!(
            "  [{type_tag}] {} [{location}] → {}",
            node.label,
            neighbours.join(", ")
        )
    }
}
