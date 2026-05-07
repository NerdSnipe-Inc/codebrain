use regex::Regex;
use std::sync::OnceLock;

use crate::collector::CollectedFile;
use crate::types::{Confidence, Framework, RouteInfo};

// ── Static regexes ────────────────────────────────────────────────────────────

fn axum_route() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"\.route\(\s*"(/[^"]*)"[^)]*?(get|post|put|delete|patch|head)"#).unwrap()
    })
}

fn actix_macro() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"#\[(get|post|put|delete|patch)\s*\(\s*"(/[^"]*)""#).unwrap()
    })
}

fn actix_route() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"\.route\s*\(\s*"(/[^"]*)".*?web::(get|post|put|delete|patch)"#).unwrap()
    })
}

fn express_route() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"(?:app|router)\.(get|post|put|delete|patch)\s*\(\s*['"]([^'"]+)['"]"#)
            .unwrap()
    })
}

fn hono_route() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"(?:app|hono)\.(get|post|put|delete|patch)\s*\(\s*['"]([^'"]+)['"]"#)
            .unwrap()
    })
}

fn fastapi_route() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"@(?:app|router)\.(get|post|put|delete|patch)\s*\(\s*['"]([^'"]+)['"]"#)
            .unwrap()
    })
}

fn flask_route() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"@(?:app|bp|blueprint)\s*\.route\s*\(\s*['"]([^'"]+)['"][^)]*methods\s*=\s*\[([^\]]+)\]"#)
            .unwrap()
    })
}

fn flask_simple_route() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"@(?:app|bp|blueprint)\s*\.route\s*\(\s*['"]([^'"]+)['"]"#).unwrap()
    })
}

fn nextjs_handler() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"export\s+(?:async\s+)?function\s+(GET|POST|PUT|DELETE|PATCH)\s*\("#)
            .unwrap()
    })
}

fn gin_route() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"(?:r|router|group)\.(GET|POST|PUT|DELETE|PATCH)\s*\(\s*"([^"]+)""#)
            .unwrap()
    })
}

fn fiber_route() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"app\.(Get|Post|Put|Delete|Patch)\s*\(\s*"([^"]+)""#).unwrap()
    })
}

fn nestjs_decorator() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"@(Get|Post|Put|Delete|Patch)\s*\(\s*['"]([^'"]+)['"]"#).unwrap()
    })
}

// ── Framework detection ───────────────────────────────────────────────────────

pub fn detect_frameworks(files: &[CollectedFile]) -> Vec<Framework> {
    let mut found = std::collections::HashSet::new();

    for file in files {
        let c = &file.content;
        let ext = &file.extension;

        match ext.as_str() {
            "rs" => {
                if c.contains("axum") { found.insert(Framework::Axum); }
                if c.contains("actix_web") || c.contains("actix-web") { found.insert(Framework::Actix); }
            }
            "ts" | "tsx" => {
                if c.contains("from \"hono\"") || c.contains("from 'hono'") {
                    found.insert(Framework::Hono);
                }
                if c.contains("from \"express\"") || c.contains("from 'express'") {
                    found.insert(Framework::Express);
                }
                if c.contains("from \"fastify\"") || c.contains("from 'fastify'") {
                    found.insert(Framework::Fastify);
                }
                if c.contains("@nestjs/common") || c.contains("@nestjs/core") {
                    found.insert(Framework::NestJs);
                }
                // Next.js: look for route.ts in app/api directory
                if file.relative.contains("app/api") && (file.relative.ends_with("route.ts") || file.relative.ends_with("route.tsx")) {
                    found.insert(Framework::NextJs);
                }
                if c.contains("next/server") || c.contains("next/navigation") {
                    found.insert(Framework::NextJs);
                }
            }
            "js" | "jsx" => {
                if c.contains("require('express')") || c.contains("require(\"express\")") {
                    found.insert(Framework::Express);
                }
                if c.contains("require('hono')") || c.contains("require(\"hono\")") {
                    found.insert(Framework::Hono);
                }
            }
            "py" => {
                if c.contains("from fastapi") || c.contains("import fastapi") {
                    found.insert(Framework::FastApi);
                }
                if c.contains("from flask") || c.contains("import flask") {
                    found.insert(Framework::Flask);
                }
                if c.contains("from django") || c.contains("import django") {
                    found.insert(Framework::Django);
                }
            }
            "go" => {
                if c.contains("\"github.com/gin-gonic/gin\"") { found.insert(Framework::Gin); }
                if c.contains("\"github.com/gofiber/fiber") { found.insert(Framework::Fiber); }
                if c.contains("\"github.com/labstack/echo") { found.insert(Framework::Echo); }
            }
            _ => {}
        }
    }

    if found.is_empty() {
        found.insert(Framework::Unknown);
    }

    let mut v: Vec<Framework> = found.into_iter().collect();
    v.sort_by_key(|f| format!("{:?}", f));
    v
}

// ── Route detection ───────────────────────────────────────────────────────────

pub fn detect_all(files: &[CollectedFile], frameworks: &[Framework]) -> Vec<RouteInfo> {
    let mut routes = Vec::new();

    for file in files {
        let content  = &file.content;
        let ext      = &file.extension;
        let rel_path = &file.relative;

        if frameworks.contains(&Framework::Axum) && ext == "rs" {
            detect_axum(content, rel_path, &mut routes);
        }
        if frameworks.contains(&Framework::Actix) && ext == "rs" {
            detect_actix(content, rel_path, &mut routes);
        }
        if (frameworks.contains(&Framework::Express) || frameworks.contains(&Framework::Hono))
            && matches!(ext.as_str(), "ts" | "tsx" | "js" | "jsx")
        {
            detect_express(content, rel_path, &mut routes);
            detect_hono(content, rel_path, &mut routes);
        }
        if frameworks.contains(&Framework::NestJs) && matches!(ext.as_str(), "ts" | "tsx") {
            detect_nestjs(content, rel_path, &mut routes);
        }
        if frameworks.contains(&Framework::NextJs) && matches!(ext.as_str(), "ts" | "tsx") {
            detect_nextjs(content, rel_path, &mut routes);
        }
        if (frameworks.contains(&Framework::FastApi) || frameworks.contains(&Framework::Flask))
            && ext == "py"
        {
            detect_fastapi(content, rel_path, &mut routes);
            detect_flask(content, rel_path, &mut routes);
        }
        if (frameworks.contains(&Framework::Gin) || frameworks.contains(&Framework::Fiber))
            && ext == "go"
        {
            detect_gin(content, rel_path, &mut routes);
            detect_fiber(content, rel_path, &mut routes);
        }
    }

    routes.dedup_by(|a, b| a.method == b.method && a.path == b.path && a.file == b.file);
    routes
}

fn tags_from_path(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|s| !s.is_empty() && !s.starts_with(':') && !s.starts_with('{'))
        .map(|s| s.to_string())
        .collect()
}

fn detect_axum(content: &str, file: &str, routes: &mut Vec<RouteInfo>) {
    for cap in axum_route().captures_iter(content) {
        let path   = cap.get(1).map(|m| m.as_str()).unwrap_or("").to_string();
        let method = cap.get(2).map(|m| m.as_str().to_uppercase()).unwrap_or_default();
        routes.push(RouteInfo {
            method,
            path: path.clone(),
            file: file.to_string(),
            framework: Framework::Axum,
            tags: tags_from_path(&path),
            confidence: Confidence::Regex,
        });
    }
}

fn detect_actix(content: &str, file: &str, routes: &mut Vec<RouteInfo>) {
    // Attribute macros: #[get("/path")]
    for cap in actix_macro().captures_iter(content) {
        let method = cap.get(1).map(|m| m.as_str().to_uppercase()).unwrap_or_default();
        let path   = cap.get(2).map(|m| m.as_str()).unwrap_or("").to_string();
        routes.push(RouteInfo {
            method,
            path: path.clone(),
            file: file.to_string(),
            framework: Framework::Actix,
            tags: tags_from_path(&path),
            confidence: Confidence::Regex,
        });
    }
    // .route() style
    for cap in actix_route().captures_iter(content) {
        let path   = cap.get(1).map(|m| m.as_str()).unwrap_or("").to_string();
        let method = cap.get(2).map(|m| m.as_str().to_uppercase()).unwrap_or_default();
        routes.push(RouteInfo {
            method,
            path: path.clone(),
            file: file.to_string(),
            framework: Framework::Actix,
            tags: tags_from_path(&path),
            confidence: Confidence::Regex,
        });
    }
}

fn detect_express(content: &str, file: &str, routes: &mut Vec<RouteInfo>) {
    for cap in express_route().captures_iter(content) {
        let method = cap.get(1).map(|m| m.as_str().to_uppercase()).unwrap_or_default();
        let path   = cap.get(2).map(|m| m.as_str()).unwrap_or("").to_string();
        routes.push(RouteInfo {
            method,
            path: path.clone(),
            file: file.to_string(),
            framework: Framework::Express,
            tags: tags_from_path(&path),
            confidence: Confidence::Regex,
        });
    }
}

fn detect_hono(content: &str, file: &str, routes: &mut Vec<RouteInfo>) {
    for cap in hono_route().captures_iter(content) {
        let method = cap.get(1).map(|m| m.as_str().to_uppercase()).unwrap_or_default();
        let path   = cap.get(2).map(|m| m.as_str()).unwrap_or("").to_string();
        routes.push(RouteInfo {
            method,
            path: path.clone(),
            file: file.to_string(),
            framework: Framework::Hono,
            tags: tags_from_path(&path),
            confidence: Confidence::Regex,
        });
    }
}

fn detect_nestjs(content: &str, file: &str, routes: &mut Vec<RouteInfo>) {
    for cap in nestjs_decorator().captures_iter(content) {
        let method = cap.get(1).map(|m| m.as_str().to_uppercase()).unwrap_or_default();
        let path   = cap.get(2).map(|m| m.as_str()).unwrap_or("").to_string();
        routes.push(RouteInfo {
            method,
            path: if path.starts_with('/') { path.clone() } else { format!("/{}", path) },
            file: file.to_string(),
            framework: Framework::NestJs,
            tags: tags_from_path(&path),
            confidence: Confidence::Regex,
        });
    }
}

fn detect_nextjs(content: &str, file: &str, routes: &mut Vec<RouteInfo>) {
    // Infer path from file location: app/api/payments/route.ts → /api/payments
    let inferred_path = if file.contains("app/api") {
        let stripped = file
            .trim_end_matches("/route.ts")
            .trim_end_matches("/route.tsx");
        let api_idx = stripped.find("app/api")
            .map(|i| i + 3) // skip "app"
            .unwrap_or(0);
        stripped[api_idx..].to_string()
    } else {
        String::from("/unknown")
    };

    for cap in nextjs_handler().captures_iter(content) {
        let method = cap.get(1).map(|m| m.as_str().to_uppercase()).unwrap_or_default();
        routes.push(RouteInfo {
            method,
            path: inferred_path.clone(),
            file: file.to_string(),
            framework: Framework::NextJs,
            tags: tags_from_path(&inferred_path),
            confidence: Confidence::Regex,
        });
    }
}

fn detect_fastapi(content: &str, file: &str, routes: &mut Vec<RouteInfo>) {
    for cap in fastapi_route().captures_iter(content) {
        let method = cap.get(1).map(|m| m.as_str().to_uppercase()).unwrap_or_default();
        let path   = cap.get(2).map(|m| m.as_str()).unwrap_or("").to_string();
        routes.push(RouteInfo {
            method,
            path: path.clone(),
            file: file.to_string(),
            framework: Framework::FastApi,
            tags: tags_from_path(&path),
            confidence: Confidence::Regex,
        });
    }
}

fn detect_flask(content: &str, file: &str, routes: &mut Vec<RouteInfo>) {
    // With methods= annotation
    for cap in flask_route().captures_iter(content) {
        let path    = cap.get(1).map(|m| m.as_str()).unwrap_or("").to_string();
        let methods = cap.get(2).map(|m| m.as_str()).unwrap_or("GET");
        for method in methods.split(',') {
            let m = method.trim().trim_matches('\'').trim_matches('"').to_uppercase();
            if !m.is_empty() {
                routes.push(RouteInfo {
                    method: m,
                    path: path.clone(),
                    file: file.to_string(),
                    framework: Framework::Flask,
                    tags: tags_from_path(&path),
                    confidence: Confidence::Regex,
                });
            }
        }
    }
    // Simple @app.route without methods — assume GET
    for cap in flask_simple_route().captures_iter(content) {
        let path = cap.get(1).map(|m| m.as_str()).unwrap_or("").to_string();
        // Avoid double-detecting routes that already had methods=
        if !routes.iter().any(|r| r.path == path && r.file == file) {
            routes.push(RouteInfo {
                method: "GET".to_string(),
                path: path.clone(),
                file: file.to_string(),
                framework: Framework::Flask,
                tags: tags_from_path(&path),
                confidence: Confidence::Regex,
            });
        }
    }
}

fn detect_gin(content: &str, file: &str, routes: &mut Vec<RouteInfo>) {
    for cap in gin_route().captures_iter(content) {
        let method = cap.get(1).map(|m| m.as_str().to_uppercase()).unwrap_or_default();
        let path   = cap.get(2).map(|m| m.as_str()).unwrap_or("").to_string();
        routes.push(RouteInfo {
            method,
            path: path.clone(),
            file: file.to_string(),
            framework: Framework::Gin,
            tags: tags_from_path(&path),
            confidence: Confidence::Regex,
        });
    }
}

fn detect_fiber(content: &str, file: &str, routes: &mut Vec<RouteInfo>) {
    for cap in fiber_route().captures_iter(content) {
        let method = cap.get(1).map(|m| m.as_str().to_uppercase()).unwrap_or_default();
        let path   = cap.get(2).map(|m| m.as_str()).unwrap_or("").to_string();
        routes.push(RouteInfo {
            method,
            path: path.clone(),
            file: file.to_string(),
            framework: Framework::Fiber,
            tags: tags_from_path(&path),
            confidence: Confidence::Regex,
        });
    }
}
