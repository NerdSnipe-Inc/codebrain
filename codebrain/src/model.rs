//! In-memory knowledge graph built from a serialisable `GraphStore`.
//!
//! `CodeGraph` wraps a `petgraph::DiGraph` for O(V+E) traversal (BFS/DFS,
//! shortest path, degree centrality). It is never serialised directly —
//! the `GraphStore` in `types.rs` is the durable form, and `CodeGraph` is
//! reconstructed at runtime from it.
//!
//! # Node identity
//! Every node has a stable string `id` (e.g. `"src/auth.rs::authenticate"`)
//! stored in `NodeData`. The `node_index` HashMap maps that id to its
//! `petgraph::NodeIndex` so edge lookups are O(1).

use std::collections::HashMap;

use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction;

use crate::types::{EdgeConfidence, GraphEdge, GraphStore, NodeData, NodeType};

/// In-memory directed knowledge graph.
#[derive(Default)]
pub struct CodeGraph {
    /// petgraph directed graph. Node weights are indices into `self.nodes`;
    /// edge weights are indices into `self.edges`. This indirection lets us
    /// keep all data in contiguous Vecs (fast iteration) while petgraph
    /// handles the topology.
    inner: DiGraph<usize, usize>,
    /// All nodes in insertion order.
    pub nodes: Vec<NodeData>,
    /// All edges in insertion order.
    pub edges: Vec<GraphEdge>,
    /// id → NodeIndex for O(1) lookups.
    node_index: HashMap<String, NodeIndex>,
}

impl CodeGraph {
    /// Build a `CodeGraph` from the serialisable `GraphStore`.
    /// Called on warm-start (loaded from disk) and after each fresh scan.
    pub fn from_store(store: GraphStore) -> Self {
        let mut graph = CodeGraph::default();
        for node in store.nodes {
            graph.add_node(node);
        }
        for edge in store.edges {
            graph.add_edge(edge);
        }
        graph
    }

    /// Convert back to a serialisable `GraphStore` for disk persistence.
    pub fn to_store(&self) -> GraphStore {
        GraphStore {
            nodes: self.nodes.clone(),
            edges: self.edges.clone(),
        }
    }

    // ── Mutation ──────────────────────────────────────────────────────────────

    /// Add a node, deduplicating by id.
    /// If a node with the same id already exists, it is replaced — this lets
    /// AST extraction overwrite placeholder nodes added by detector output.
    pub fn add_node(&mut self, data: NodeData) -> NodeIndex {
        if let Some(&idx) = self.node_index.get(&data.id) {
            let slot = self.inner[idx];
            self.nodes[slot] = data;
            return idx;
        }
        let slot = self.nodes.len();
        self.nodes.push(data.clone());
        let idx = self.inner.add_node(slot);
        self.node_index.insert(data.id, idx);
        idx
    }

    /// Add a directed edge. Silently skips edges whose endpoints are unknown
    /// (can happen when import resolution fails or detectors refer to external
    /// symbols not in the scanned file set).
    pub fn add_edge(&mut self, edge: GraphEdge) {
        let from = match self.node_index.get(&edge.from_id) {
            Some(&i) => i,
            None => return,
        };
        let to = match self.node_index.get(&edge.to_id) {
            Some(&i) => i,
            None => return,
        };
        let slot = self.edges.len();
        self.edges.push(edge);
        self.inner.add_edge(from, to, slot);
    }

    // ── Queries ───────────────────────────────────────────────────────────────

    /// Resolve a node id to its data.
    pub fn get_node(&self, id: &str) -> Option<&NodeData> {
        let idx = self.node_index.get(id)?;
        let slot = self.inner[*idx];
        self.nodes.get(slot)
    }

    /// Neighbours of `id` in the given direction, with their edge data.
    pub fn neighbours(&self, id: &str, dir: Direction) -> Vec<(&NodeData, &GraphEdge)> {
        let idx = match self.node_index.get(id) {
            Some(&i) => i,
            None => return Vec::new(),
        };
        self.inner
            .edges_directed(idx, dir)
            .filter_map(|er| {
                let edge = self.edges.get(*er.weight())?;
                let neighbour_slot = self.inner[er.target()];
                let neighbour = self.nodes.get(neighbour_slot)?;
                Some((neighbour, edge))
            })
            .collect()
    }

    /// Total node count.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Total edge count.
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Return the top-N nodes sorted by total degree (in + out).
    /// These are the "god nodes" — the most-connected abstractions.
    /// Equivalent to graphify's god_nodes analysis.
    pub fn god_nodes(&self, n: usize) -> Vec<(&NodeData, usize)> {
        let mut degrees: Vec<(NodeIndex, usize)> = self
            .inner
            .node_indices()
            .map(|idx| {
                let deg = self.inner.edges_directed(idx, Direction::Incoming).count()
                    + self.inner.edges_directed(idx, Direction::Outgoing).count();
                (idx, deg)
            })
            .collect();

        degrees.sort_by(|a, b| b.1.cmp(&a.1));
        degrees.truncate(n);

        degrees
            .into_iter()
            .filter_map(|(idx, deg)| {
                let slot = self.inner[idx];
                self.nodes.get(slot).map(|n| (n, deg))
            })
            .collect()
    }

    /// All nodes of a given type.
    pub fn nodes_of_type(&self, t: NodeType) -> Vec<&NodeData> {
        self.nodes.iter().filter(|n| n.node_type == t).collect()
    }

    /// Return the petgraph reference for use by query.rs traversals.
    pub(crate) fn inner(&self) -> &DiGraph<usize, usize> {
        &self.inner
    }

    /// Resolve a petgraph NodeIndex to NodeData.
    pub(crate) fn node_at(&self, idx: NodeIndex) -> Option<&NodeData> {
        self.nodes.get(self.inner[idx])
    }

    /// Resolve a NodeIndex to its id string.
    #[allow(dead_code)]
    pub(crate) fn id_at(&self, idx: NodeIndex) -> Option<&str> {
        self.node_at(idx).map(|n| n.id.as_str())
    }

    /// Look up a NodeIndex by id.
    pub(crate) fn index_of(&self, id: &str) -> Option<NodeIndex> {
        self.node_index.get(id).copied()
    }

    /// All nodes with label or file containing any of the query terms (case-insensitive).
    /// Used as BFS seed nodes in query.rs.
    pub(crate) fn seed_nodes(&self, terms: &[&str]) -> Vec<NodeIndex> {
        self.inner
            .node_indices()
            .filter(|&idx| {
                if let Some(data) = self.node_at(idx) {
                    let label_lc = data.label.to_lowercase();
                    let file_lc  = data.file.to_lowercase();
                    terms.iter().any(|t| {
                        let t = t.to_lowercase();
                        label_lc.contains(&t) || file_lc.contains(&t)
                    })
                } else {
                    false
                }
            })
            .collect()
    }

    /// Nodes whose `confidence` on at least one incoming edge is `Ambiguous`.
    /// Surfaces in the report as "knowledge gaps".
    pub fn ambiguous_nodes(&self) -> Vec<&NodeData> {
        let mut ambiguous_indices = std::collections::HashSet::new();
        for er in self.inner.edge_references() {
            if let Some(edge) = self.edges.get(*er.weight()) {
                if matches!(edge.confidence, EdgeConfidence::Ambiguous(_)) {
                    ambiguous_indices.insert(er.target());
                }
            }
        }
        ambiguous_indices
            .into_iter()
            .filter_map(|idx| self.node_at(idx))
            .collect()
    }

    /// BFS over incoming `Calls` edges from a set of seed nodes, up to `max_depth`.
    ///
    /// Returns `(NodeIndex, depth)` pairs for every reachable caller, excluding
    /// the seeds themselves. Nodes already visited are not revisited.
    pub fn callers_bfs(&self, seeds: &[NodeIndex], max_depth: usize) -> Vec<(NodeIndex, usize)> {
        use std::collections::{HashSet, VecDeque};
        use crate::types::EdgeKind;

        let mut visited: HashSet<NodeIndex> = seeds.iter().copied().collect();
        let mut queue: VecDeque<(NodeIndex, usize)> = seeds.iter().map(|&s| (s, 0)).collect();
        let mut result = Vec::new();

        while let Some((node, depth)) = queue.pop_front() {
            if depth >= max_depth { continue; }
            for er in self.inner.edges_directed(node, Direction::Incoming) {
                let slot = *er.weight();
                let kind = self.edges.get(slot).map(|e| e.kind);
                if !matches!(kind, Some(EdgeKind::Calls)) { continue; }
                let caller = er.source();
                if visited.insert(caller) {
                    result.push((caller, depth + 1));
                    queue.push_back((caller, depth + 1));
                }
            }
        }
        result
    }

    /// Edges that cross community boundaries — "surprising connections".
    /// Always empty until Louvain assigns non-zero community ids; included
    /// now so the formatter can call it without conditional compilation.
    pub fn cross_community_edges(&self) -> Vec<(&NodeData, &NodeData, &GraphEdge)> {
        self.inner
            .edge_references()
            .filter_map(|er| {
                let from = self.node_at(er.source())?;
                let to   = self.node_at(er.target())?;
                if from.community != to.community && from.community != 0 {
                    let edge = self.edges.get(*er.weight())?;
                    Some((from, to, edge))
                } else {
                    None
                }
            })
            .collect()
    }
}
