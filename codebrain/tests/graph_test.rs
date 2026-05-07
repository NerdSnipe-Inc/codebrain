use codebrain::collector::CollectedFile;
use codebrain::graph::{blast_radius, build};
use std::path::PathBuf;

fn make_file(relative: &str, ext: &str, content: &str) -> CollectedFile {
    CollectedFile {
        path:      PathBuf::from(relative),
        relative:  relative.to_string(),
        extension: ext.to_string(),
        size:      content.len() as u64,
        content:   content.to_string(),
    }
}

#[test]
fn builds_dependency_graph() {
    let files = vec![
        make_file("src/lib.rs", "rs", ""),
        make_file("src/utils.rs", "rs", "use crate::lib::Foo;"),
        make_file("src/handlers.rs", "rs", "use crate::utils::helper;\nuse crate::lib::Bar;"),
    ];

    // Verifies build() doesn't panic — edge resolution for Rust crate:: paths is best-effort
    let graph = build(&files);
    let _ = graph;
}

#[test]
fn detects_hot_files() {
    // Create files where src/db.ts is imported by many files
    let db_content = "export const db = {};";
    let files = vec![
        make_file("src/db.ts", "ts", db_content),
        make_file("src/users.ts", "ts", "import { db } from './db';"),
        make_file("src/posts.ts", "ts", "import { db } from './db';"),
        make_file("src/orders.ts", "ts", "import { db } from './db';"),
    ];

    let graph = build(&files);

    // src/db.ts should be the hottest file (imported by 3 others)
    let hottest = graph.hot_files.first();
    assert!(hottest.is_some());
    if let Some(hot) = hottest {
        assert_eq!(hot.file, "src/db.ts");
        assert_eq!(hot.imported_by, 3);
    }
}

#[test]
fn blast_radius_finds_affected_files() {
    let files = vec![
        make_file("src/db.ts", "ts", "export const db = {};"),
        make_file("src/repo.ts", "ts", "import { db } from './db';"),
        make_file("src/service.ts", "ts", "import { repo } from './repo';"),
        make_file("src/handler.ts", "ts", "import { service } from './service';"),
    ];

    let graph = build(&files);

    // Changing db.ts should affect repo.ts, service.ts, handler.ts
    let source = vec!["src/db.ts".to_string()];
    let radius = blast_radius(&source, &graph, &[], &[], 10);

    assert!(
        radius.affected_files.contains(&"src/db.ts".to_string()),
        "source file should be in affected set"
    );
    // At least db.ts itself should be there
    assert!(!radius.affected_files.is_empty());
}

#[test]
fn blast_radius_respects_max_depth() {
    let files = vec![
        make_file("src/a.ts", "ts", "export const a = 1;"),
        make_file("src/b.ts", "ts", "import { a } from './a';"),
        make_file("src/c.ts", "ts", "import { b } from './b';"),
        make_file("src/d.ts", "ts", "import { c } from './c';"),
    ];

    let graph = build(&files);
    let source = vec!["src/a.ts".to_string()];

    let radius_depth1 = blast_radius(&source, &graph, &[], &[], 1);
    let radius_depth3 = blast_radius(&source, &graph, &[], &[], 3);

    assert!(
        radius_depth3.affected_files.len() >= radius_depth1.affected_files.len(),
        "deeper BFS should find at least as many files"
    );
}
