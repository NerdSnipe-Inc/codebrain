use crate::collector::CollectedFile;
use crate::detectors::deps;
use crate::types::{BlastRadius, DependencyGraph, HotFile, ImportEdge, RouteInfo, SchemaModel};

/// Build the dependency graph from the collected file set.
pub fn build(files: &[CollectedFile]) -> DependencyGraph {
    let mut edges: Vec<ImportEdge> = Vec::new();

    for file in files {
        let imports = deps::extract_imports(&file.content, &file.extension);

        for import in imports {
            if let Some(resolved) = deps::resolve_import(&import, &file.relative, files) {
                // Avoid self-loops
                if resolved != file.relative {
                    edges.push(ImportEdge {
                        from: file.relative.clone(),
                        to:   resolved,
                    });
                }
            }
        }
    }

    // Deduplicate
    edges.sort_by(|a, b| a.from.cmp(&b.from).then(a.to.cmp(&b.to)));
    edges.dedup_by(|a, b| a.from == b.from && a.to == b.to);

    let hot_files = compute_hot_files(&edges);
    DependencyGraph { edges, hot_files }
}

fn compute_hot_files(edges: &[ImportEdge]) -> Vec<HotFile> {
    let mut counts: std::collections::HashMap<&str, usize> = Default::default();
    for edge in edges {
        *counts.entry(edge.to.as_str()).or_default() += 1;
    }
    let mut hot: Vec<HotFile> = counts
        .into_iter()
        .map(|(file, count)| HotFile {
            file:        file.to_string(),
            imported_by: count,
        })
        .collect();
    hot.sort_by(|a, b| b.imported_by.cmp(&a.imported_by));
    hot.truncate(20);
    hot
}

/// BFS from source_files through the import graph (reverse edges — who imports us?).
pub fn blast_radius(
    source_files: &[String],
    graph:        &DependencyGraph,
    routes:       &[RouteInfo],
    schemas:      &[SchemaModel],
    max_depth:    usize,
) -> BlastRadius {
    let mut affected: std::collections::HashSet<String> = Default::default();
    let mut queue: std::collections::VecDeque<(String, usize)> = Default::default();

    for f in source_files {
        queue.push_back((f.clone(), 0));
    }

    while let Some((file, depth)) = queue.pop_front() {
        if depth >= max_depth || !affected.insert(file.clone()) {
            continue;
        }
        // Find all files that import this one (reverse edges)
        for edge in &graph.edges {
            if edge.to == file {
                queue.push_back((edge.from.clone(), depth + 1));
            }
        }
    }

    let affected_routes: Vec<RouteInfo> = routes
        .iter()
        .filter(|r| affected.contains(&r.file))
        .cloned()
        .collect();

    let affected_models: Vec<String> = schemas
        .iter()
        .filter(|s| {
            affected
                .iter()
                .any(|f| f.to_lowercase().contains(&s.name.to_lowercase()))
        })
        .map(|s| s.name.clone())
        .collect();

    let mut affected_vec: Vec<String> = affected.into_iter().collect();
    affected_vec.sort();

    BlastRadius {
        source_files:    source_files.to_vec(),
        affected_files:  affected_vec,
        affected_routes,
        affected_models,
        depth: max_depth,
    }
}
