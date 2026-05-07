use crate::collector::CollectedFile;

/// A detected middleware registration.
#[derive(Debug, Clone)]
pub struct MiddlewareInfo {
    pub name:      String,
    pub file:      String,
    pub framework: String,
}

/// Detect common middleware patterns across the codebase.
pub fn detect_all(files: &[CollectedFile]) -> Vec<MiddlewareInfo> {
    let mut results = Vec::new();

    for file in files {
        let content  = &file.content;
        let ext      = &file.extension;
        let rel_path = &file.relative;

        match ext.as_str() {
            "rs" => detect_rust(content, rel_path, &mut results),
            "ts" | "tsx" | "js" | "jsx" => detect_node(content, rel_path, &mut results),
            "py" => detect_python(content, rel_path, &mut results),
            "go" => detect_go(content, rel_path, &mut results),
            _ => {}
        }
    }

    results
}

fn detect_rust(content: &str, file: &str, results: &mut Vec<MiddlewareInfo>) {
    // Axum layers: .layer(CorsLayer::...) .layer(TraceLayer::...)
    for line in content.lines() {
        let t = line.trim();
        if t.starts_with(".layer(") || t.contains(".layer(") {
            if let Some(name) = extract_layer_name(t) {
                results.push(MiddlewareInfo {
                    name,
                    file: file.to_string(),
                    framework: "axum".to_string(),
                });
            }
        }
        // Actix wrap: .wrap(middleware::Logger::default())
        if t.contains(".wrap(") {
            if let Some(name) = extract_wrap_name(t) {
                results.push(MiddlewareInfo {
                    name,
                    file: file.to_string(),
                    framework: "actix".to_string(),
                });
            }
        }
    }
}

fn detect_node(content: &str, file: &str, results: &mut Vec<MiddlewareInfo>) {
    for line in content.lines() {
        let t = line.trim();
        // Express/Hono: app.use(cors()) app.use(helmet()) etc.
        if t.starts_with("app.use(") || t.contains("app.use(") {
            if let Some(mid_start) = t.find("app.use(") {
                let after = &t[mid_start + 8..];
                let name  = after.split(['(', ')', ',']).next().unwrap_or("").trim().to_string();
                if !name.is_empty() {
                    results.push(MiddlewareInfo {
                        name,
                        file: file.to_string(),
                        framework: "express/hono".to_string(),
                    });
                }
            }
        }
    }
}

fn detect_python(content: &str, file: &str, results: &mut Vec<MiddlewareInfo>) {
    for line in content.lines() {
        let t = line.trim();
        // FastAPI: app.add_middleware(CORSMiddleware, ...)
        if t.starts_with("app.add_middleware(") {
            let after = t.trim_start_matches("app.add_middleware(");
            let name  = after.split(',').next().unwrap_or("").trim().to_string();
            if !name.is_empty() {
                results.push(MiddlewareInfo {
                    name,
                    file: file.to_string(),
                    framework: "fastapi/flask".to_string(),
                });
            }
        }
    }
}

fn detect_go(content: &str, file: &str, results: &mut Vec<MiddlewareInfo>) {
    for line in content.lines() {
        let t = line.trim();
        // Gin: r.Use(gin.Logger()) r.Use(cors.Default())
        if t.starts_with("r.Use(") || t.contains(".Use(") {
            if let Some(use_start) = t.find(".Use(") {
                let after = &t[use_start + 5..];
                let name  = after.split(['(', ')']).next().unwrap_or("").trim().to_string();
                if !name.is_empty() {
                    results.push(MiddlewareInfo {
                        name,
                        file: file.to_string(),
                        framework: "gin/fiber/echo".to_string(),
                    });
                }
            }
        }
    }
}

fn extract_layer_name(line: &str) -> Option<String> {
    let start = line.find(".layer(")?;
    let after = &line[start + 7..];
    let name  = after.split(['(', ':',  ' ']).next()?.trim().to_string();
    if name.is_empty() { None } else { Some(name) }
}

fn extract_wrap_name(line: &str) -> Option<String> {
    let start = line.find(".wrap(")?;
    let after = &line[start + 6..];
    let name  = after.split(['(', ':',  ' ']).next()?.trim().to_string();
    if name.is_empty() { None } else { Some(name) }
}
