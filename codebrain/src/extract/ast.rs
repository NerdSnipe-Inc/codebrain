//! Tree-sitter AST extraction dispatcher.
//!
//! Routes each collected file to the appropriate language walker based on
//! its extension. Files with unknown extensions are silently skipped.
//!
//! The dispatcher integrates with the SHA-256 extraction cache: unchanged
//! files are loaded from cache without re-running the tree-sitter parser.
//! Only files whose content has changed (or that have never been seen before)
//! incur a parsing cost.

use crate::cache::ExtractionCache;
use crate::collector::CollectedFile;
use crate::extract::ExtractionResult;
use crate::extract::lang::{go, javascript, python, rust, swift, typescript};

/// Run AST extraction over all collected files, using the cache to skip
/// unchanged files. Returns one `ExtractionResult` per file in input order
/// (empty result for files with unsupported extensions).
pub fn extract_all(
    files: &[CollectedFile],
    cache: &ExtractionCache,
) -> Vec<(String, ExtractionResult)> {
    files
        .iter()
        .map(|file| {
            let result = extract_file(file, cache);
            (file.relative.clone(), result)
        })
        .collect()
}

/// Extract (or load from cache) a single file.
fn extract_file(file: &CollectedFile, cache: &ExtractionCache) -> ExtractionResult {
    let hash = ExtractionCache::hash(&file.content);

    // Cache hit — skip parsing entirely
    if let Some(cached) = cache.get(&hash) {
        tracing::trace!(file = %file.relative, "ast: cache hit");
        return cached;
    }

    // Cache miss — run the appropriate language walker
    let result = match file.extension.as_str() {
        "rs"        => rust::extract(&file.content, &file.relative),
        "ts"        => typescript::extract(&file.content, &file.relative, false),
        "tsx"       => typescript::extract(&file.content, &file.relative, true),
        "js" | "jsx"=> javascript::extract(&file.content, &file.relative),
        "py"        => python::extract(&file.content, &file.relative),
        "go"        => go::extract(&file.content, &file.relative),
        "swift"     => swift::extract(&file.content, &file.relative),
        ext         => {
            tracing::trace!(
                file = %file.relative,
                ext,
                "ast: no walker for extension — skipping"
            );
            return ExtractionResult::default();
        }
    };

    // Persist to cache for the next run
    if !result.is_empty() {
        cache.put(&hash, &file.relative, &result);
    }

    tracing::trace!(
        file  = %file.relative,
        nodes = result.nodes.len(),
        edges = result.edges.len(),
        "ast: extracted"
    );

    result
}
