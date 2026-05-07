use codebrain::formatter::{estimate_tokens, format_context_block};
use codebrain::types::{
    Confidence, DependencyGraph, EnvVar, Framework, HotFile, Language, Orm, ProjectInfo,
    RouteInfo, ScanResult, SchemaModel, TokenStats,
};
use std::path::PathBuf;

fn make_scan_result() -> ScanResult {
    ScanResult {
        project: ProjectInfo {
            root:       PathBuf::from("/project"),
            name:       "test-project".to_string(),
            frameworks: vec![Framework::Axum],
            orms:       vec![Orm::SeaOrm],
            language:   Language::Rust,
        },
        routes: vec![
            RouteInfo {
                method:     "GET".to_string(),
                path:       "/api/users".to_string(),
                file:       "src/routes.rs".to_string(),
                framework:  Framework::Axum,
                tags:       vec!["api".to_string(), "users".to_string()],
                confidence: Confidence::Regex,
            },
            RouteInfo {
                method:     "POST".to_string(),
                path:       "/api/users".to_string(),
                file:       "src/routes.rs".to_string(),
                framework:  Framework::Axum,
                tags:       vec!["api".to_string(), "users".to_string()],
                confidence: Confidence::Regex,
            },
        ],
        schemas: vec![SchemaModel {
            name:       "User".to_string(),
            table_name: Some("users".to_string()),
            fields:     Vec::new(),
            relations:  Vec::new(),
            orm:        Orm::SeaOrm,
            confidence: Confidence::Regex,
        }],
        graph: DependencyGraph {
            edges:     Vec::new(),
            hot_files: vec![HotFile {
                file:        "src/db.rs".to_string(),
                imported_by: 5,
            }],
        },
        env_vars: vec![
            EnvVar {
                name:    "DATABASE_URL".to_string(),
                file:    "src/main.rs".to_string(),
                pattern: "std::env::var".to_string(),
            },
            EnvVar {
                name:    "PORT".to_string(),
                file:    "src/main.rs".to_string(),
                pattern: "std::env::var".to_string(),
            },
        ],
        token_stats: TokenStats {
            estimated_context_tokens: 150,
            route_count:              2,
            schema_count:             1,
            file_count:               10,
            hot_file_count:           1,
        },
        scanned_at: chrono::Utc::now(),
    }
}

#[test]
fn format_block_contains_key_sections() {
    let result = make_scan_result();
    let block  = format_context_block(&result, 4_000);

    assert!(block.contains("test-project"), "should include project name");
    assert!(block.contains("Routes"), "should include routes section");
    assert!(block.contains("GET /api/users"), "should include specific route");
    assert!(block.contains("Schemas"), "should include schemas section");
    assert!(block.contains("User"), "should include model name");
    assert!(block.contains("Hot files"), "should include hot files");
    assert!(block.contains("DATABASE_URL"), "should include env vars");
    assert!(block.starts_with("== Codebase:"), "should start with header");
    assert!(block.contains("== End codebase context =="), "should end with footer");
}

#[test]
fn format_block_respects_token_budget() {
    let result = make_scan_result();

    // Very tight budget (10 tokens = ~40 chars)
    let tiny = format_context_block(&result, 10);
    assert!(tiny.len() <= 40 + 50, "should be near the budget");
    assert!(tiny.contains("truncated"), "should indicate truncation");

    // Generous budget
    let full = format_context_block(&result, 10_000);
    assert!(!full.contains("truncated"), "should not truncate with large budget");
}

#[test]
fn estimate_tokens_returns_reasonable_values() {
    let result = make_scan_result();
    let stats  = estimate_tokens(&result.routes, &result.schemas, &result.graph);

    assert!(stats.estimated_context_tokens > 0);
    assert!(stats.estimated_context_tokens < 10_000);
    assert_eq!(stats.route_count, 2);
    assert_eq!(stats.schema_count, 1);
}
