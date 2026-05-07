//! Graph analysis: god nodes, knowledge gaps, surprising connections.
//!
//! These functions interpret the topology of a `CodeGraph` to surface insights
//! for the formatter and for direct agent queries.

use crate::model::CodeGraph;
use crate::types::{EdgeConfidence, NodeType};

/// The top most-connected nodes in the graph ("god nodes" in graphify's
/// terminology). High degree = central abstraction that many parts of the
/// codebase depend on.
///
/// Returns `(label, file, degree)` tuples sorted by degree descending.
pub fn god_nodes(graph: &CodeGraph, n: usize) -> Vec<GodNode> {
    graph
        .god_nodes(n)
        .into_iter()
        .map(|(node, degree)| GodNode {
            label:  node.label.clone(),
            file:   node.file.clone(),
            line:   node.line,
            degree,
        })
        .collect()
}

/// A high-centrality node.
#[derive(Debug)]
pub struct GodNode {
    pub label:  String,
    pub file:   String,
    pub line:   usize,
    pub degree: usize,
}

/// Nodes that have only Ambiguous incoming edges — these are concepts the
/// graph knows about but whose connections are uncertain. Useful for
/// identifying areas that need more documentation or clearer imports.
pub fn knowledge_gaps(graph: &CodeGraph) -> Vec<GapNode> {
    graph
        .ambiguous_nodes()
        .into_iter()
        .map(|node| GapNode {
            label: node.label.clone(),
            file:  node.file.clone(),
        })
        .collect()
}

#[derive(Debug)]
pub struct GapNode {
    pub label: String,
    pub file:  String,
}

/// Orphan nodes: nodes with zero edges in either direction.
/// These are often dead code, copy-paste artifacts, or files the import
/// resolver couldn't connect to the rest of the graph.
pub fn orphan_nodes(graph: &CodeGraph) -> Vec<&crate::types::NodeData> {
    use petgraph::Direction;
    graph
        .nodes
        .iter()
        .filter(|node| {
            // Skip route and schema overlay nodes — they are intentionally
            // unlinked until the heuristic Contains edges are proven
            if matches!(node.node_type, NodeType::Route | NodeType::Schema) {
                return false;
            }
            let idx = match graph.index_of(&node.id) {
                Some(i) => i,
                None    => return false,
            };
            let in_deg  = graph.inner().edges_directed(idx, Direction::Incoming).count();
            let out_deg = graph.inner().edges_directed(idx, Direction::Outgoing).count();
            in_deg == 0 && out_deg == 0
        })
        .collect()
}

/// Edges that cross community boundaries — "surprising connections" in graphify's
/// terminology.  Always empty until Louvain is implemented (all communities are 0).
pub fn surprising_connections(
    graph: &CodeGraph,
) -> Vec<SurprisingEdge> {
    graph
        .cross_community_edges()
        .into_iter()
        .map(|(from, to, edge)| SurprisingEdge {
            from_label: from.label.clone(),
            from_file:  from.file.clone(),
            to_label:   to.label.clone(),
            to_file:    to.file.clone(),
            confidence: edge.confidence,
        })
        .collect()
}

#[derive(Debug)]
pub struct SurprisingEdge {
    pub from_label: String,
    pub from_file:  String,
    pub to_label:   String,
    pub to_file:    String,
    pub confidence: EdgeConfidence,
}

/// Quick stats used by the formatter's graph section header.
#[derive(Debug)]
pub struct GraphStats {
    pub node_count:      usize,
    pub edge_count:      usize,
    pub function_count:  usize,
    pub route_count:     usize,
    pub schema_count:    usize,
    pub community_count: usize,
}

pub fn graph_stats(graph: &CodeGraph) -> GraphStats {
    let communities: std::collections::HashSet<u32> =
        graph.nodes.iter().map(|n| n.community).collect();

    GraphStats {
        node_count:      graph.node_count(),
        edge_count:      graph.edge_count(),
        function_count:  graph.nodes_of_type(NodeType::Function).len()
                       + graph.nodes_of_type(NodeType::Method).len(),
        route_count:     graph.nodes_of_type(NodeType::Route).len(),
        schema_count:    graph.nodes_of_type(NodeType::Schema).len(),
        community_count: if communities.len() <= 1 { 0 } else { communities.len() },
    }
}
