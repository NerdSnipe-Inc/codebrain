//! LLM-driven semantic extraction for non-code files (.md, .txt, .rst).
//!
//! # Current status: STUB
//!
//! Semantic extraction is architecturally wired but produces empty results
//! in v1. The `SemanticExtractor` trait is defined here so that the
//! `CodeBrainHandle` can accept an injected implementation without importing
//! `ai_provider` (which would create a circular dependency).
//!
//! # How to enable
//!
//! 1. Implement `SemanticExtractor` in `agent_core` or `app`, using the
//!    existing `AiProvider::complete_with_tools()` API with a JSON Schema
//!    describing the `ExtractionResult` structure (see schema below).
//!
//! 2. Inject the implementation via `CodeBrainHandle::with_semantic_extractor()`.
//!
//! 3. Set `config.enable_semantic_extraction = true`.
//!
//! # Intended LLM extraction schema
//!
//! The LLM is called with `complete_with_tools()` and a single tool whose
//! `input_schema` mirrors `ExtractionResult`:
//!
//! ```json
//! {
//!   "name": "record_graph_extraction",
//!   "description": "Record extracted nodes and edges from a document.",
//!   "input_schema": {
//!     "type": "object",
//!     "properties": {
//!       "nodes": {
//!         "type": "array",
//!         "items": {
//!           "type": "object",
//!           "required": ["id", "label", "node_type"],
//!           "properties": {
//!             "id":        { "type": "string" },
//!             "label":     { "type": "string" },
//!             "node_type": {
//!               "type": "string",
//!               "enum": ["Function", "Class", "Module", "Document", "Concept"]
//!             },
//!             "line": { "type": "integer", "default": 0 }
//!           }
//!         }
//!       },
//!       "edges": {
//!         "type": "array",
//!         "items": {
//!           "type": "object",
//!           "required": ["from_id", "to_id", "kind"],
//!           "properties": {
//!             "from_id":    { "type": "string" },
//!             "to_id":      { "type": "string" },
//!             "kind":       {
//!               "type": "string",
//!               "enum": ["Imports", "Calls", "Contains", "SemanticallySimilarTo"]
//!             },
//!             "confidence": { "type": "number", "minimum": 0.0, "maximum": 1.0,
//!                             "description": "0.0-0.39 = Ambiguous, 0.4-0.95 = Inferred, 1.0 = Extracted" }
//!           }
//!         }
//!       }
//!     }
//!   }
//! }
//! ```
//!
//! # Batching strategy (graphify approach)
//!
//! - Chunk non-code files into groups of 20-25 (group by directory).
//! - Dispatch all chunks concurrently via `tokio::task::spawn` or
//!   `futures::future::join_all`.
//! - Merge results in `build.rs` after all futures complete.
//! - Cache each file's result by SHA-256 hash so re-runs only extract changed files.
//!
//! # Confidence assignment
//!
//! Parse the LLM's `confidence` float and map:
//!   - 1.0         → `EdgeConfidence::Extracted`  (the LLM is saying it's definitive)
//!   - 0.4 – 0.99  → `EdgeConfidence::Inferred(score)`
//!   - 0.0 – 0.39  → `EdgeConfidence::Ambiguous(score)` (flag for review)

use crate::collector::CollectedFile;
use crate::extract::ExtractionResult;

/// Trait for injecting LLM-driven semantic extraction without importing ai_provider.
///
/// The `app` crate implements this using `AiProvider::complete_with_tools()`.
/// When no extractor is injected, semantic extraction is silently skipped.
pub trait SemanticExtractor: Send + Sync {
    /// Extract nodes and edges from a batch of non-code files.
    ///
    /// `files` is a slice of (relative_path, content) pairs.
    /// Returns one `ExtractionResult` per file in input order.
    fn extract_batch(&self, files: &[(&str, &str)]) -> Vec<ExtractionResult>;
}

/// Document file extensions eligible for semantic extraction.
pub const DOC_EXTENSIONS: &[&str] = &["md", "txt", "rst"];

/// Filter a file list to just document files.
pub fn doc_files(files: &[CollectedFile]) -> Vec<&CollectedFile> {
    files
        .iter()
        .filter(|f| DOC_EXTENSIONS.contains(&f.extension.as_str()))
        .collect()
}

/// Run semantic extraction over all doc files using the injected extractor.
///
/// When `extractor` is `None` this is a no-op — returns empty results for
/// every file. That is the v1 behaviour.
///
/// Batching: files are processed in chunks of `BATCH_SIZE` to stay within
/// typical LLM context windows (~50K tokens per call).
pub fn extract_docs(
    files:     &[&CollectedFile],
    extractor: Option<&dyn SemanticExtractor>,
) -> Vec<(String, ExtractionResult)> {
    let extractor = match extractor {
        Some(e) => e,
        None    => {
            // No extractor injected — return empty results for all doc files.
            // This is expected in v1.
            return files
                .iter()
                .map(|f| (f.relative.clone(), ExtractionResult::default()))
                .collect();
        }
    };

    const BATCH_SIZE: usize = 20;
    let mut all_results: Vec<(String, ExtractionResult)> = Vec::new();

    for chunk in files.chunks(BATCH_SIZE) {
        let pairs: Vec<(&str, &str)> = chunk
            .iter()
            .map(|f| (f.relative.as_str(), f.content.as_str()))
            .collect();

        let batch_results = extractor.extract_batch(&pairs);

        for (file, result) in chunk.iter().zip(batch_results.into_iter()) {
            all_results.push((file.relative.clone(), result));
        }
    }

    all_results
}
