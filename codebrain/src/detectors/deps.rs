/// Extract import strings from file content based on file extension.
/// Returns raw import paths (not yet resolved to file paths).
pub fn extract_imports(content: &str, extension: &str) -> Vec<String> {
    match extension {
        "rs"             => extract_rust_imports(content),
        "ts" | "tsx"     => extract_ts_imports(content),
        "js" | "jsx"     => extract_js_imports(content),
        "py"             => extract_python_imports(content),
        "go"             => extract_go_imports(content),
        _                => Vec::new(),
    }
}

fn extract_rust_imports(content: &str) -> Vec<String> {
    let mut imports = Vec::new();
    for line in content.lines() {
        let t = line.trim();
        // use crate::foo::bar;  or  use super::foo;
        if t.starts_with("use ") || t.starts_with("pub use ") {
            let raw = t.trim_start_matches("pub ").trim_start_matches("use ");
            let raw = raw.trim_end_matches(';');
            // Split multi-imports: use foo::{a, b} → ["foo::a", "foo::b"]
            if raw.contains('{') {
                if let Some(prefix_end) = raw.find("::") {
                    let prefix = &raw[..prefix_end];
                    let inner_start = raw.find('{').unwrap_or(raw.len());
                    let inner_end   = raw.find('}').unwrap_or(raw.len());
                    if inner_start < inner_end {
                        let inner = &raw[inner_start + 1..inner_end];
                        for item in inner.split(',') {
                            let item = item.trim();
                            if !item.is_empty() && item != "_" {
                                imports.push(format!("{}::{}", prefix, item));
                            }
                        }
                    }
                }
            } else {
                imports.push(raw.to_string());
            }
        }
    }
    imports
}

fn extract_ts_imports(content: &str) -> Vec<String> {
    use regex::Regex;
    use std::sync::OnceLock;

    static TS_IMPORT: OnceLock<Regex> = OnceLock::new();
    let re = TS_IMPORT.get_or_init(|| {
        Regex::new(r#"(?:import|export)[^'"]*from\s+['"]([^'"]+)['"]"#).unwrap()
    });

    static TS_REQUIRE: OnceLock<Regex> = OnceLock::new();
    let re_req = TS_REQUIRE.get_or_init(|| {
        Regex::new(r#"require\s*\(\s*['"]([^'"]+)['"]\s*\)"#).unwrap()
    });

    let mut imports: Vec<String> = re.captures_iter(content)
        .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
        .collect();

    imports.extend(
        re_req.captures_iter(content)
            .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
    );

    imports
}

fn extract_js_imports(content: &str) -> Vec<String> {
    // Same patterns as TS
    extract_ts_imports(content)
}

fn extract_python_imports(content: &str) -> Vec<String> {
    let mut imports = Vec::new();
    for line in content.lines() {
        let t = line.trim();
        if t.starts_with("from ") && t.contains(" import ") {
            let module = t.trim_start_matches("from ")
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_string();
            imports.push(module);
        } else if t.starts_with("import ") {
            let module = t.trim_start_matches("import ")
                .split([',', ';'])
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            imports.push(module);
        }
    }
    imports
}

fn extract_go_imports(content: &str) -> Vec<String> {
    use regex::Regex;
    use std::sync::OnceLock;

    static GO_IMPORT: OnceLock<Regex> = OnceLock::new();
    let re = GO_IMPORT.get_or_init(|| {
        Regex::new(r#"["']([^"']+)["']"#).unwrap()
    });

    let mut in_import_block = false;
    let mut imports = Vec::new();

    for line in content.lines() {
        let t = line.trim();
        if t == "import (" { in_import_block = true; continue; }
        if in_import_block && t == ")" { in_import_block = false; continue; }

        if in_import_block {
            if let Some(cap) = re.captures(t) {
                imports.push(cap[1].to_string());
            }
        } else if t.starts_with("import ") {
            if let Some(cap) = re.captures(t) {
                imports.push(cap[1].to_string());
            }
        }
    }

    imports
}

/// Attempt to resolve a raw import string to a relative file path within the project.
/// Returns None if the import is external (e.g. a crate name, npm package).
pub fn resolve_import(
    import:    &str,
    from_file: &str,
    all_files: &[crate::collector::CollectedFile],
) -> Option<String> {
    // Only resolve relative imports (start with . or ../ or are internal Rust paths)
    let is_relative_ts = import.starts_with('.') || import.starts_with('/');
    let is_rust_internal = import.starts_with("crate::") || import.starts_with("super::");

    if !is_relative_ts && !is_rust_internal {
        return None;
    }

    if is_rust_internal {
        // Convert crate::foo::bar → src/foo/bar.rs (heuristic)
        let path = import
            .trim_start_matches("crate::")
            .trim_start_matches("super::")
            .replace("::", "/");

        // Try to find a matching file in the all_files list
        return all_files.iter()
            .find(|f| {
                let without_ext = f.relative.trim_end_matches(".rs");
                without_ext.ends_with(&path) || f.relative.contains(&path)
            })
            .map(|f| f.relative.clone());
    }

    // TypeScript/JS relative import resolution
    let from_dir = std::path::Path::new(from_file)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    let candidate_base = if import.starts_with('/') {
        import.trim_start_matches('/').to_string()
    } else {
        let joined = format!("{}/{}", from_dir, import.trim_start_matches("./"));
        // Normalize: remove ./ and ../
        normalize_path(&joined)
    };

    // Try with common extensions
    let extensions = ["ts", "tsx", "js", "jsx", "py", "go", "rs"];
    for ext in &extensions {
        let candidate = format!("{}.{}", candidate_base, ext);
        if all_files.iter().any(|f| f.relative == candidate) {
            return Some(candidate);
        }
    }

    // Try index files
    for ext in &extensions {
        let candidate = format!("{}/index.{}", candidate_base, ext);
        if all_files.iter().any(|f| f.relative == candidate) {
            return Some(candidate);
        }
    }

    // Direct match
    if all_files.iter().any(|f| f.relative == candidate_base) {
        return Some(candidate_base);
    }

    None
}

fn normalize_path(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            ".."  => { parts.pop(); }
            "."   => {}
            ""    => {}
            other => parts.push(other),
        }
    }
    parts.join("/")
}
