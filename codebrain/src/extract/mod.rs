//! Extraction layer — turns source files into raw graph data.
//!
//! Two extraction strategies run independently and are merged in `build.rs`:
//!
//! 1. **AST extraction** (`ast.rs` + `lang/`) — deterministic, no LLM, fast.
//!    Uses tree-sitter to walk syntax trees and extract symbols and edges with
//!    `EdgeConfidence::Extracted`.
//!
//! 2. **Semantic extraction** (`semantic.rs`) — LLM-driven, opt-in.
//!    Dispatched only when `config.enable_semantic_extraction = true` and a
//!    `SemanticExtractor` is injected into the `CodeBrainHandle`. Produces
//!    `EdgeConfidence::Inferred` and `EdgeConfidence::Ambiguous` edges.

pub mod ast;
pub mod lang;
pub mod semantic;

use serde::{Deserialize, Serialize};

use crate::types::{GraphEdge, NodeData};

/// The raw output produced by any extractor for a single source file.
/// Multiple `ExtractionResult`s are merged in `build.rs`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExtractionResult {
    pub nodes: Vec<NodeData>,
    pub edges: Vec<GraphEdge>,
}

impl ExtractionResult {
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty() && self.edges.is_empty()
    }

    /// Merge another result into this one (nodes + edges concatenated).
    pub fn merge(&mut self, other: ExtractionResult) {
        self.nodes.extend(other.nodes);
        self.edges.extend(other.edges);
    }
}
