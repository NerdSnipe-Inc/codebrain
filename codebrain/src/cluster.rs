//! Community detection for the knowledge graph using the Louvain algorithm.
//!
//! Louvain optimises modularity Q over the graph partition in two alternating
//! phases until Q stops improving:
//!
//!   Phase 1 — Local moves: for each node, find the neighbour community that
//!   maximises ΔQ and move the node there. Repeat until no move improves Q.
//!
//!   Phase 2 — Aggregation: collapse each community into a single super-node,
//!   sum edge weights, and run Phase 1 on the meta-graph.
//!
//! Directed edges are treated as undirected: degree(v) = in_degree + out_degree.
//!
//! ΔQ formula (simplified, unweighted):
//!
//! ```text
//!   ΔQ = k_i_in / m  −  (Σ_tot · k_i) / (2m²)
//!
//!   m       = total edge count
//!   k_i     = degree of node i
//!   k_i_in  = edges between i and nodes already in the candidate community
//!   Σ_tot   = sum of degrees of all nodes in the candidate community (before i joins)
//! ```

use std::collections::HashMap;
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use crate::model::CodeGraph;

// ── Public API ───────────────────────────────────────────────────────────────

/// Assign community ids to every node using the Louvain algorithm.
pub fn assign_communities(graph: &mut CodeGraph) {
    let n = graph.node_count();
    if n == 0 { return; }
    let assignments = louvain(graph);
    apply_communities(graph, &assignments);
}

/// Write a community assignment vector back into graph node data.
/// `assignments[i]` is the community id for the node at Vec slot `i`.
fn apply_communities(graph: &mut CodeGraph, assignments: &[u32]) {
    for (i, node) in graph.nodes.iter_mut().enumerate() {
        if let Some(&community) = assignments.get(i) {
            node.community = community;
        }
    }
}

/// Return a human-readable summary of the detected communities.
/// Returns an empty Vec if clustering has not been run (all nodes in community 0).
pub fn community_summary(graph: &CodeGraph) -> Vec<CommunitySummary> {
    let mut groups: HashMap<u32, Vec<&str>> = Default::default();
    for node in &graph.nodes {
        groups.entry(node.community).or_default().push(&node.file);
    }
    if groups.len() <= 1 {
        return Vec::new();
    }
    let mut summaries: Vec<CommunitySummary> = groups
        .into_iter()
        .map(|(id, files)| {
            let size  = files.len();
            let label = dominant_module(&files)
                .map(|m| format!("{m} ({size})"))
                .unwrap_or_else(|| format!("community_{id} ({size})"));
            CommunitySummary { id, size, label }
        })
        .collect();
    summaries.sort_by(|a, b| b.size.cmp(&a.size));
    summaries
}

#[derive(Debug)]
pub struct CommunitySummary {
    pub id:    u32,
    pub size:  usize,
    pub label: String,
}

// ── Louvain driver ───────────────────────────────────────────────────────────

fn louvain(graph: &CodeGraph) -> Vec<u32> {
    let n = graph.node_count();
    if graph.edge_count() == 0 {
        // No edges — every node is its own community.
        return (0..n as u32).collect();
    }

    // Phase 1: local moves on the original graph.
    let mut state = CommunityState::init(graph);
    while phase1_pass(&mut state, graph) {}

    // Compact community ids to 0..k before building the meta-graph.
    let (compacted, k) = compact(&state.assignment);
    if k <= 1 {
        return compacted;
    }

    // Phase 2: optimise on the meta-graph of super-nodes.
    let meta        = MetaGraph::build(&compacted, k, &state, graph);
    let meta_labels = meta.optimise();

    // Map each original node through compacted → meta_labels.
    let mut final_assignment: Vec<u32> = (0..n)
        .map(|i| meta_labels[compacted[i] as usize])
        .collect();
    renumber(&mut final_assignment);
    final_assignment
}

// ── Phase 1 ──────────────────────────────────────────────────────────────────

struct CommunityState {
    assignment: Vec<u32>,
    sigma_in:   HashMap<u32, f64>,  // community → Σ internal edge weight (×2)
    sigma_tot:  HashMap<u32, f64>,  // community → Σ member degrees
    m:          f64,                // total directed edge count
    degrees:    Vec<f64>,           // degrees[node_slot] = in + out
}

impl CommunityState {
    fn init(graph: &CodeGraph) -> Self {
        let inner = graph.inner();
        let n     = graph.node_count();
        let m     = graph.edge_count() as f64;

        let mut degrees = vec![0.0f64; n];
        for idx in inner.node_indices() {
            degrees[idx.index()] =
                (inner.edges_directed(idx, Direction::Incoming).count()
               + inner.edges_directed(idx, Direction::Outgoing).count()) as f64;
        }

        // Each node starts in its own singleton community.
        let assignment: Vec<u32> = (0..n as u32).collect();
        let sigma_in:  HashMap<u32, f64> = (0..n as u32).map(|c| (c, 0.0)).collect();
        let sigma_tot: HashMap<u32, f64> = (0..n as u32).map(|c| (c, degrees[c as usize])).collect();

        CommunityState { assignment, sigma_in, sigma_tot, m, degrees }
    }
}

/// One full pass over all nodes. Returns true if any node moved.
fn phase1_pass(state: &mut CommunityState, graph: &CodeGraph) -> bool {
    let inner = graph.inner();
    let indices: Vec<NodeIndex> = inner.node_indices().collect();
    let mut moved = false;

    for idx in indices {
        let i     = idx.index();
        let c_old = state.assignment[i];
        let k_i   = state.degrees[i];
        let m     = state.m;
        let m2    = 2.0 * m;

        // Sum edge weights to each neighbour community (both directions).
        let mut nbr: HashMap<u32, f64> = HashMap::new();
        for er in inner.edges_directed(idx, Direction::Outgoing) {
            let c_j = state.assignment[er.target().index()];
            *nbr.entry(c_j).or_insert(0.0) += 1.0;
        }
        for er in inner.edges_directed(idx, Direction::Incoming) {
            let c_j = state.assignment[er.source().index()];
            *nbr.entry(c_j).or_insert(0.0) += 1.0;
        }

        let k_i_in_old = nbr.get(&c_old).copied().unwrap_or(0.0);

        // Temporarily remove i from c_old.
        *state.sigma_tot.entry(c_old).or_insert(0.0) -= k_i;
        *state.sigma_in.entry(c_old).or_insert(0.0)  -= 2.0 * k_i_in_old;

        // ΔQ of rejoining c_old is the baseline — we only move if another
        // community strictly beats it.
        let s_tot_old = *state.sigma_tot.get(&c_old).unwrap_or(&0.0);
        let mut best_dq = k_i_in_old / m - (s_tot_old * k_i) / (m * m2);
        let mut best_c  = c_old;

        for (&c_nbr, &k_i_in) in &nbr {
            if c_nbr == c_old { continue; }
            let s_tot = *state.sigma_tot.get(&c_nbr).unwrap_or(&0.0);
            let dq    = k_i_in / m - (s_tot * k_i) / (m * m2);
            if dq > best_dq {
                best_dq = dq;
                best_c  = c_nbr;
            }
        }

        // Re-insert into the winning community (may be c_old).
        let k_i_in_best = nbr.get(&best_c).copied().unwrap_or(0.0);
        state.assignment[i] = best_c;
        *state.sigma_tot.entry(best_c).or_insert(0.0) += k_i;
        *state.sigma_in.entry(best_c).or_insert(0.0)  += 2.0 * k_i_in_best;

        if best_c != c_old {
            moved = true;
        }
    }

    moved
}

// ── Phase 2 (MetaGraph) ──────────────────────────────────────────────────────

struct MetaGraph {
    n:          usize,
    m:          f64,
    degrees:    Vec<f64>,           // degrees[super_node]
    self_loops: Vec<f64>,           // self_loops[super_node]
    adj:        HashMap<(u32, u32), f64>,  // (min, max) → total weight
}

impl MetaGraph {
    fn build(compacted: &[u32], k: usize, state: &CommunityState, graph: &CodeGraph) -> Self {
        let inner = graph.inner();
        let mut degrees    = vec![0.0f64; k];
        let mut self_loops = vec![0.0f64; k];
        let mut adj: HashMap<(u32, u32), f64> = HashMap::new();

        for idx in inner.node_indices() {
            let ci = compacted[idx.index()] as usize;
            degrees[ci] += state.degrees[idx.index()];
        }

        for er in inner.edge_references() {
            let ci = compacted[er.source().index()];
            let cj = compacted[er.target().index()];
            if ci == cj {
                self_loops[ci as usize] += 1.0;
            } else {
                let key = if ci < cj { (ci, cj) } else { (cj, ci) };
                *adj.entry(key).or_insert(0.0) += 1.0;
            }
        }

        MetaGraph {
            n: k,
            m: graph.edge_count() as f64,
            degrees,
            self_loops,
            adj,
        }
    }

    fn optimise(&self) -> Vec<u32> {
        let mut assignment: Vec<u32> = (0..self.n as u32).collect();
        let mut sigma_in: HashMap<u32, f64> =
            (0..self.n as u32).map(|i| (i, self.self_loops[i as usize] * 2.0)).collect();
        let mut sigma_tot: HashMap<u32, f64> =
            (0..self.n as u32).map(|i| (i, self.degrees[i as usize])).collect();

        let m  = self.m;
        let m2 = 2.0 * m;
        let mut moved = true;

        while moved {
            moved = false;
            for i in 0..self.n {
                let i_u32 = i as u32;
                let c_old = assignment[i];
                let k_i   = self.degrees[i];

                // Build neighbour community weights from the symmetric adjacency map.
                let mut nbr: HashMap<u32, f64> = HashMap::new();
                for (&(ca, cb), &w) in &self.adj {
                    if ca == i_u32 {
                        let cj = assignment[cb as usize];
                        *nbr.entry(cj).or_insert(0.0) += w;
                    } else if cb == i_u32 {
                        let cj = assignment[ca as usize];
                        *nbr.entry(cj).or_insert(0.0) += w;
                    }
                }
                if self.self_loops[i] > 0.0 {
                    *nbr.entry(c_old).or_insert(0.0) += self.self_loops[i];
                }

                let k_i_in_old = nbr.get(&c_old).copied().unwrap_or(0.0);
                *sigma_tot.entry(c_old).or_insert(0.0) -= k_i;
                *sigma_in.entry(c_old).or_insert(0.0)  -= 2.0 * k_i_in_old;

                let s_tot_old = *sigma_tot.get(&c_old).unwrap_or(&0.0);
                let mut best_dq = k_i_in_old / m - (s_tot_old * k_i) / (m * m2);
                let mut best_c  = c_old;

                for (&c_nbr, &k_i_in) in &nbr {
                    if c_nbr == c_old { continue; }
                    let s_tot = *sigma_tot.get(&c_nbr).unwrap_or(&0.0);
                    let dq    = k_i_in / m - (s_tot * k_i) / (m * m2);
                    if dq > best_dq {
                        best_dq = dq;
                        best_c  = c_nbr;
                    }
                }

                let k_i_in_best = nbr.get(&best_c).copied().unwrap_or(0.0);
                assignment[i] = best_c;
                *sigma_tot.entry(best_c).or_insert(0.0) += k_i;
                *sigma_in.entry(best_c).or_insert(0.0)  += 2.0 * k_i_in_best;

                if best_c != c_old {
                    moved = true;
                }
            }
        }

        assignment
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Map sparse community ids to contiguous 0..k.
fn compact(assignment: &[u32]) -> (Vec<u32>, usize) {
    let mut map: HashMap<u32, u32> = HashMap::new();
    let mut next = 0u32;
    let out: Vec<u32> = assignment.iter().map(|&c| {
        *map.entry(c).or_insert_with(|| { let v = next; next += 1; v })
    }).collect();
    (out, next as usize)
}

/// Renumber community ids in-place to contiguous 0..k.
fn renumber(assignment: &mut Vec<u32>) {
    let (out, _) = compact(assignment);
    *assignment = out;
}

/// Guess a human label from the most common top-level source directory.
fn dominant_module(files: &[&str]) -> Option<String> {
    let mut freq: HashMap<&str, usize> = HashMap::new();
    for f in files {
        let top = f.split('/').next().unwrap_or(f);
        *freq.entry(top).or_insert(0) += 1;
    }
    freq.into_iter().max_by_key(|(_, c)| *c).map(|(m, _)| m.to_string())
}
