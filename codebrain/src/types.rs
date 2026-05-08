use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Complete scan result for one project root.
/// Cached after first scan; refreshed on interval or on demand.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub project:     ProjectInfo,
    pub routes:      Vec<RouteInfo>,
    pub schemas:     Vec<SchemaModel>,
    pub graph:       DependencyGraph,
    pub env_vars:    Vec<EnvVar>,
    pub token_stats: TokenStats,
    pub scanned_at:  chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub root:       PathBuf,
    pub name:       String,
    pub frameworks: Vec<Framework>,
    pub orms:       Vec<Orm>,
    pub language:   Language,
}

/// A detected API route in the codebase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteInfo {
    pub method:     String,
    pub path:       String,
    pub file:       String,
    pub framework:  Framework,
    pub tags:       Vec<String>,
    pub confidence: Confidence,
}

/// A detected database model or schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaModel {
    pub name:       String,
    pub table_name: Option<String>,
    pub fields:     Vec<SchemaField>,
    pub relations:  Vec<String>,
    pub orm:        Orm,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaField {
    pub name:       String,
    pub field_type: String,
    pub flags:      Vec<FieldFlag>,
}

/// Import dependency graph across all scanned files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyGraph {
    pub edges:     Vec<ImportEdge>,
    pub hot_files: Vec<HotFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportEdge {
    pub from: String,
    pub to:   String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotFile {
    pub file:        String,
    pub imported_by: usize,
}

/// Result of a blast radius query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlastRadius {
    pub source_files:    Vec<String>,
    pub affected_files:  Vec<String>,
    pub affected_routes: Vec<RouteInfo>,
    pub affected_models: Vec<String>,
    pub depth:           usize,
}

/// Result of a symbol-level caller query.
///
/// Unlike `BlastRadius` (which operates on file-level import edges), this
/// walks `Calls` edges in the knowledge graph to return the specific symbols
/// that call the target — not every importer of its containing file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolCallers {
    pub target_id:      String,
    pub target_label:   String,
    pub target_file:    String,
    pub target_line:    usize,
    /// Callers at all depths, sorted by depth then file.
    pub callers:        Vec<CallerInfo>,
    /// Deduplicated files that contain at least one caller.
    pub affected_files: Vec<String>,
    pub depth:          usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallerInfo {
    pub id:    String,
    pub label: String,
    pub file:  String,
    pub line:  usize,
    /// BFS depth from the target (1 = direct caller).
    pub depth: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvVar {
    pub name:    String,
    pub file:    String,
    pub pattern: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenStats {
    pub estimated_context_tokens: usize,
    pub route_count:              usize,
    pub schema_count:             usize,
    pub file_count:               usize,
    pub hot_file_count:           usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Framework {
    // Rust
    Axum, Actix,
    // TypeScript / JavaScript
    NextJs, Express, Hono, Fastify, NestJs, SvelteKit, Remix,
    // Python
    FastApi, Flask, Django,
    // Go
    Gin, Fiber, Echo,
    // Generic
    Unknown,
}

impl std::fmt::Display for Framework {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Orm {
    // Rust
    Diesel, SeaOrm, Sqlx,
    // TypeScript / JavaScript
    Prisma, Drizzle, TypeOrm,
    // Python
    SqlAlchemy,
    // Go
    Gorm,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    Rust, TypeScript, JavaScript, Python, Go, Swift, Mixed, Unknown
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Confidence { Ast, Regex }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldFlag { Pk, Fk, Unique, Required, Default, Nullable }

// ═══════════════════════════════════════════════════════════════════════════════
// Graph types — the graphify-style knowledge graph layered on top of the
// existing flat scan results.
//
// Design overview:
//   GraphStore   — serialisable (Vec<NodeData> + Vec<GraphEdge>), persisted to
//                  disk alongside ScanResult as {project_id}_graph.json.
//   CodeGraph    — in-memory petgraph DiGraph built from GraphStore; not
//                  serialised directly, rebuilt from GraphStore on warm-start.
//
// Relationships to existing types:
//   RouteInfo  → NodeData with node_type: NodeType::Route
//   SchemaModel → NodeData with node_type: NodeType::Schema
//   ImportEdge  → GraphEdge with kind: EdgeKind::Imports
//   (HotFile is replaced by degree-centrality queries on CodeGraph)
// ═══════════════════════════════════════════════════════════════════════════════

/// Serialisable graph snapshot stored next to the flat ScanResult cache.
/// Deserialized on warm-start and passed to CodeGraph::from_store().
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GraphStore {
    pub nodes: Vec<NodeData>,
    pub edges: Vec<GraphEdge>,
}

/// A node in the codebase knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeData {
    /// Unique stable ID: "{relative_file}::{symbol}" or
    /// "{relative_file}::route::{METHOD}::{path}" for route nodes.
    pub id:        String,
    /// Human-readable label shown in context blocks and reports.
    pub label:     String,
    pub node_type: NodeType,
    /// Relative file path (same convention as CollectedFile::relative).
    pub file:      String,
    /// 1-indexed source line of the definition.
    pub line:      usize,
    /// Community membership assigned by clustering.
    /// Always 0 until Louvain is implemented — see cluster.rs.
    pub community: u32,
}

/// A directed edge in the knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub from_id:    String,
    pub to_id:      String,
    pub kind:       EdgeKind,
    pub confidence: EdgeConfidence,
}

/// What kind of code relationship this edge represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeKind {
    /// `use` / `import` / `require` — file-level dependency.
    Imports,
    /// Direct function/method call at a known call-site.
    Calls,
    /// Structural containment: module→function, class→method, struct→field.
    Contains,
    /// `impl Trait for Type` or class `implements Interface`.
    Implements,
    /// Class/struct inheritance (`extends` in TS, trait blanket impls in Rust).
    InheritsFrom,
    /// LLM-inferred conceptual similarity across files.
    /// Reserved for semantic.rs — not produced by AST extraction.
    SemanticallySimilarTo,
}

/// How confident we are that this edge is real.
///
/// Mirrors graphify's EXTRACTED / INFERRED / AMBIGUOUS model with an explicit
/// numeric score on the uncertain cases.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum EdgeConfidence {
    /// Deterministically extracted from AST — score is implicitly 1.0.
    Extracted,
    /// Inferred by heuristic or LLM reasoning. Score range: 0.4 – 0.95.
    Inferred(f32),
    /// Uncertain; flagged for review. Score range: 0.0 – 0.39.
    Ambiguous(f32),
}

/// What kind of symbol or concept a node represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeType {
    // ── Code symbols (from AST) ───────────────────────────────────────────────
    Function,
    Method,
    Struct,
    Class,
    Module,
    Variable,
    // ── Semantic overlays (from detectors) ───────────────────────────────────
    /// HTTP route handler — label is "METHOD /path", file is the handler file.
    Route,
    /// ORM model / database schema — label is the model name.
    Schema,
    // ── Document nodes (from semantic extraction, future) ─────────────────────
    /// A documentation file (.md, .txt, .rst).
    Document,
    /// An LLM-extracted concept from documentation.
    /// Reserved for semantic.rs — not yet produced.
    Concept,
}
