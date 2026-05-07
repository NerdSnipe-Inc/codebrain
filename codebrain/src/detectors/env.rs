use regex::Regex;
use std::sync::OnceLock;

use crate::collector::CollectedFile;
use crate::types::EnvVar;

fn rust_env_var() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"(?:std::env::var|env::var|std::env::var_os)\s*\(\s*"([^"]+)""#).unwrap()
    })
}

fn node_env_var() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"process\.env\.([A-Z_][A-Z0-9_]+)"#).unwrap()
    })
}

fn dotenv_var() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        // matches: SOME_VAR=value in .env files (detected by extension)
        Regex::new(r#"^([A-Z_][A-Z0-9_]+)\s*="#).unwrap()
    })
}

fn python_env_var() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"os\.(?:environ\.get|getenv|environ)\s*\[\s*['"]([^'"]+)['"]|os\.(?:environ\.get|getenv)\s*\(\s*['"]([^'"]+)['"]"#).unwrap()
    })
}

fn go_env_var() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"os\.Getenv\s*\(\s*"([^"]+)""#).unwrap()
    })
}

pub fn detect_all(files: &[CollectedFile]) -> Vec<EnvVar> {
    let mut vars: Vec<EnvVar> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for file in files {
        let content  = &file.content;
        let ext      = &file.extension;
        let rel_path = &file.relative;

        let (pattern_label, captures): (&str, Vec<String>) = match ext.as_str() {
            "rs" => (
                "std::env::var",
                rust_env_var()
                    .captures_iter(content)
                    .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
                    .collect(),
            ),
            "ts" | "tsx" | "js" | "jsx" => (
                "process.env",
                node_env_var()
                    .captures_iter(content)
                    .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
                    .collect(),
            ),
            "py" => (
                "os.getenv",
                python_env_var()
                    .captures_iter(content)
                    .filter_map(|c| {
                        c.get(1).or_else(|| c.get(2)).map(|m| m.as_str().to_string())
                    })
                    .collect(),
            ),
            "go" => (
                "os.Getenv",
                go_env_var()
                    .captures_iter(content)
                    .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
                    .collect(),
            ),
            _ => {
                // Detect .env file contents (e.g. .env, .env.local)
                if rel_path.contains(".env") {
                    (
                        "dotenv",
                        dotenv_var()
                            .captures_iter(content)
                            .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
                            .collect(),
                    )
                } else {
                    continue;
                }
            }
        };

        for name in captures {
            if seen.insert(name.clone()) {
                vars.push(EnvVar {
                    name,
                    file: rel_path.clone(),
                    pattern: pattern_label.to_string(),
                });
            }
        }
    }

    vars.sort_by(|a, b| a.name.cmp(&b.name));
    vars
}
