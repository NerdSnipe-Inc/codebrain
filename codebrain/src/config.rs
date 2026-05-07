use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeBrainConfig {
    /// Root directory of the project to scan.
    pub project_root: PathBuf,

    /// Directories to exclude from scanning.
    #[serde(default = "default_excludes")]
    pub exclude_dirs: Vec<String>,

    /// File extensions to scan.
    #[serde(default = "default_extensions")]
    pub extensions: Vec<String>,

    /// Maximum file size to scan in bytes (skip larger files).
    #[serde(default = "default_max_file_size")]
    pub max_file_size: u64,

    /// How often to auto-rescan in seconds. 0 = manual only.
    #[serde(default = "default_rescan_secs")]
    pub rescan_interval_secs: u64,

    /// Maximum token budget for the context block injected into agent prompts.
    #[serde(default = "default_token_budget")]
    pub context_token_budget: usize,

    /// If Some, write wiki articles to this vault path after each scan.
    #[serde(default)]
    pub vault_sync_path: Option<String>,

    /// Enable tree-sitter AST extraction to build the knowledge graph.
    /// When false, the graph is built only from detector output (routes, schemas,
    /// import edges) — faster but shallower. Defaults to true.
    #[serde(default = "default_true")]
    pub enable_ast_extraction: bool,

    /// Enable LLM semantic extraction for .md / .txt / .rst doc files.
    /// Requires a SemanticExtractor to be injected into the handle; has no
    /// effect when None is injected. Defaults to false (opt-in).
    #[serde(default)]
    pub enable_semantic_extraction: bool,
}

impl CodeBrainConfig {
    pub fn new(project_root: impl Into<PathBuf>) -> Self {
        Self {
            project_root:              project_root.into(),
            exclude_dirs:              default_excludes(),
            extensions:                default_extensions(),
            max_file_size:             default_max_file_size(),
            rescan_interval_secs:      default_rescan_secs(),
            context_token_budget:      default_token_budget(),
            vault_sync_path:           None,
            enable_ast_extraction:     true,
            enable_semantic_extraction: false,
        }
    }
}

fn default_excludes() -> Vec<String> {
    [
        "target", "node_modules", ".git", "dist", ".next", "__pycache__", ".mypy_cache",
        "vendor", ".build", "DerivedData", "*.xcodeproj", "*.xcworkspace",
    ]
    .iter().map(|s| s.to_string()).collect()
}

fn default_extensions() -> Vec<String> {
    ["rs", "ts", "tsx", "js", "jsx", "py", "go", "swift"]
        .iter().map(|s| s.to_string()).collect()
}

fn default_max_file_size() -> u64   { 500_000 }
fn default_rescan_secs()   -> u64   { 300 }
fn default_token_budget()  -> usize { 4_000 }
fn default_true()          -> bool  { true }
