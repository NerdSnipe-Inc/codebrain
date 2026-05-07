pub mod analyze;
pub mod build;
pub mod cache;
pub mod cluster;
pub mod collector;
pub mod config;
pub mod detectors;
pub mod error;
pub mod extract;
pub mod formatter;
pub mod graph;
pub mod handle;
pub mod model;
pub mod query;
pub mod registry;
pub mod scanner;
pub mod types;

// ── Primary public API (unchanged from v1) ────────────────────────────────────

pub use config::CodeBrainConfig;
pub use error::CodeBrainError;
pub use handle::CodeBrainHandle;
pub use registry::CodeBrainRegistry;
pub use types::{
    BlastRadius, Confidence, DependencyGraph, EnvVar, FieldFlag, Framework, HotFile, ImportEdge,
    Language, Orm, ProjectInfo, RouteInfo, ScanResult, SchemaField, SchemaModel, TokenStats,
};

// ── Graph types (new in v2) ───────────────────────────────────────────────────

pub use model::CodeGraph;
pub use types::{
    EdgeConfidence, EdgeKind, GraphEdge, GraphStore, NodeData, NodeType,
};

// ── Semantic extraction trait (injectable, no ai_provider dep) ───────────────

pub use extract::semantic::SemanticExtractor;
