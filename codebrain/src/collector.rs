use crate::config::CodeBrainConfig;
use std::path::PathBuf;
use walkdir::WalkDir;

/// A file that has been collected and its content loaded.
#[derive(Debug, Clone)]
pub struct CollectedFile {
    pub path:      PathBuf,
    pub relative:  String,
    pub extension: String,
    pub size:      u64,
    pub content:   String,
}

/// Walk the project root and return all scannable files,
/// filtered by extension and exclude_dirs, with content loaded.
pub fn collect_files(config: &CodeBrainConfig) -> Vec<CollectedFile> {
    let root = &config.project_root;
    let mut files = Vec::new();

    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();

        // Skip directories
        if path.is_dir() {
            let dir_name = path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            if config.exclude_dirs.iter().any(|ex| ex == dir_name) {
                // Note: walkdir doesn't support pruning via filter_map easily,
                // but parent-dir exclusion below handles it
            }
            continue;
        }

        // Check if any ancestor is an excluded directory
        let relative = match path.strip_prefix(root) {
            Ok(r) => r.to_string_lossy().to_string(),
            Err(_) => continue,
        };

        let is_excluded = relative.split(std::path::MAIN_SEPARATOR).any(|segment| {
            config.exclude_dirs.iter().any(|ex| ex == segment)
        });
        if is_excluded {
            continue;
        }

        let ext = path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_string();

        if !config.extensions.iter().any(|e| e == &ext) {
            continue;
        }

        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        if size > config.max_file_size {
            continue;
        }

        let content = std::fs::read_to_string(path).unwrap_or_default();

        files.push(CollectedFile {
            path: path.to_path_buf(),
            relative,
            extension: ext,
            size,
            content,
        });
    }

    files
}

/// Detect the primary language from the collected file set.
pub fn detect_language(files: &[CollectedFile]) -> crate::types::Language {
    use crate::types::Language;

    let mut rs    = 0usize;
    let mut ts    = 0usize;
    let mut js    = 0usize;
    let mut py    = 0usize;
    let mut go    = 0usize;
    let mut swift = 0usize;

    for f in files {
        match f.extension.as_str() {
            "rs"             => rs    += 1,
            "ts" | "tsx"     => ts    += 1,
            "js" | "jsx"     => js    += 1,
            "py"             => py    += 1,
            "go"             => go    += 1,
            "swift"          => swift += 1,
            _ => {}
        }
    }

    let total = rs + ts + js + py + go + swift;
    if total == 0 { return Language::Unknown; }

    let dominant_threshold = total * 2 / 3;

    if rs    >= dominant_threshold { return Language::Rust; }
    if py    >= dominant_threshold { return Language::Python; }
    if go    >= dominant_threshold { return Language::Go; }
    if swift >= dominant_threshold { return Language::Swift; }
    if ts    >= dominant_threshold { return Language::TypeScript; }
    if js    >= dominant_threshold { return Language::JavaScript; }

    // TypeScript + JavaScript together
    if ts + js >= dominant_threshold { return Language::TypeScript; }

    Language::Mixed
}
