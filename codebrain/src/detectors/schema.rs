use regex::Regex;
use std::sync::OnceLock;

use crate::collector::CollectedFile;
use crate::types::{Confidence, FieldFlag, Orm, SchemaField, SchemaModel};

// ── Static regexes ────────────────────────────────────────────────────────────

fn sea_orm_table() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"#\[sea_orm\s*\(\s*table_name\s*=\s*"([^"]+)""#).unwrap())
}

fn rust_struct() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"pub\s+struct\s+(\w+)"#).unwrap())
}

fn diesel_table() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"table!\s*\{\s*(\w+)\s*\((\w+)\)"#).unwrap())
}

fn diesel_column() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"(\w+)\s*->\s*(\w+)"#).unwrap())
}

fn prisma_model() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"model\s+(\w+)\s*\{"#).unwrap())
}

fn prisma_field() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"^\s+(\w+)\s+(\w+[\?]?)"#).unwrap())
}

fn drizzle_table() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"export\s+const\s+(\w+)\s*=\s*(?:pg|mysql|sqlite)Table\s*\(\s*['"]([^'"]+)['"]"#)
            .unwrap()
    })
}

fn sqlalchemy_class() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"class\s+(\w+)\s*\([^)]*(?:Base|Model|db\.Model)[^)]*\)"#).unwrap()
    })
}

fn sqlalchemy_tablename() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"__tablename__\s*=\s*['"]([^'"]+)['"]"#).unwrap())
}

fn gorm_struct() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"type\s+(\w+)\s+struct\s*\{"#).unwrap())
}

// ── ORM detection ─────────────────────────────────────────────────────────────

pub fn detect_orms(files: &[CollectedFile]) -> Vec<Orm> {
    let mut found = std::collections::HashSet::new();

    for file in files {
        let c   = &file.content;
        let ext = &file.extension;

        match ext.as_str() {
            "rs" => {
                if c.contains("sea_orm") || c.contains("SeaOrm") || c.contains("sea-orm") {
                    found.insert(Orm::SeaOrm);
                }
                if c.contains("diesel") {
                    found.insert(Orm::Diesel);
                }
                if c.contains("sqlx") {
                    found.insert(Orm::Sqlx);
                }
            }
            "ts" | "tsx" => {
                if c.contains("@prisma/client") || c.contains("prisma") {
                    found.insert(Orm::Prisma);
                }
                if c.contains("drizzle-orm") || c.contains("drizzle") {
                    found.insert(Orm::Drizzle);
                }
                if c.contains("typeorm") || c.contains("TypeORM") {
                    found.insert(Orm::TypeOrm);
                }
            }
            "py" => {
                if c.contains("sqlalchemy") || c.contains("SQLAlchemy") {
                    found.insert(Orm::SqlAlchemy);
                }
            }
            "go" => {
                if c.contains("gorm") || c.contains("GORM") {
                    found.insert(Orm::Gorm);
                }
            }
            _ => {}
        }
    }

    if found.is_empty() {
        found.insert(Orm::Unknown);
    }
    let mut v: Vec<Orm> = found.into_iter().collect();
    v.sort_by_key(|o| format!("{:?}", o));
    v
}

// ── Schema detection ──────────────────────────────────────────────────────────

pub fn detect_all(files: &[CollectedFile], orms: &[Orm]) -> Vec<SchemaModel> {
    let mut models = Vec::new();

    for file in files {
        let content  = &file.content;
        let ext      = &file.extension;
        let rel_path = &file.relative;

        if orms.contains(&Orm::SeaOrm) && ext == "rs" {
            detect_sea_orm(content, rel_path, &mut models);
        }
        if orms.contains(&Orm::Diesel) && ext == "rs" {
            detect_diesel(content, rel_path, &mut models);
        }
        if orms.contains(&Orm::Prisma) && (rel_path.ends_with(".prisma") || ext == "ts") {
            detect_prisma(content, rel_path, &mut models);
        }
        if orms.contains(&Orm::Drizzle) && matches!(ext.as_str(), "ts" | "tsx") {
            detect_drizzle(content, rel_path, &mut models);
        }
        if orms.contains(&Orm::SqlAlchemy) && ext == "py" {
            detect_sqlalchemy(content, rel_path, &mut models);
        }
        if orms.contains(&Orm::Gorm) && ext == "go" {
            detect_gorm(content, rel_path, &mut models);
        }
    }

    models
}

fn detect_sea_orm(content: &str, _file: &str, models: &mut Vec<SchemaModel>) {
    // Find table_name annotations paired with the next struct
    let mut table_name: Option<String> = None;

    for line in content.lines() {
        if let Some(cap) = sea_orm_table().captures(line) {
            table_name = Some(cap[1].to_string());
        } else if let Some(cap) = rust_struct().captures(line) {
            let name = cap[1].to_string();
            // Simple field extraction: look for `pub field: Type` in the struct body
            // (multi-line, so we do a rough pass after the struct header)
            models.push(SchemaModel {
                name,
                table_name: table_name.take(),
                fields: Vec::new(), // field extraction skipped for simplicity
                relations: Vec::new(),
                orm: Orm::SeaOrm,
                confidence: Confidence::Regex,
            });
        }
    }
}

fn detect_diesel(content: &str, _file: &str, models: &mut Vec<SchemaModel>) {
    for cap in diesel_table().captures_iter(content) {
        let table_name = cap[1].to_string();
        let pk         = cap[2].to_string();

        // Find block after this match and extract columns
        let fields = extract_diesel_fields(content, &pk);

        models.push(SchemaModel {
            name: to_pascal_case(&table_name),
            table_name: Some(table_name),
            fields,
            relations: Vec::new(),
            orm: Orm::Diesel,
            confidence: Confidence::Regex,
        });
    }
}

fn extract_diesel_fields(content: &str, pk: &str) -> Vec<SchemaField> {
    let mut fields = Vec::new();
    let mut in_block = false;
    let mut depth = 0i32;

    for line in content.lines() {
        if line.contains("table!") { in_block = true; }
        if !in_block { continue; }

        depth += line.chars().filter(|&c| c == '{').count() as i32;
        depth -= line.chars().filter(|&c| c == '}').count() as i32;

        if in_block && depth > 0 {
            if let Some(cap) = diesel_column().captures(line) {
                let name  = cap[1].to_string();
                let ftype = cap[2].to_string();
                if name == "id" || name == pk { continue; } // skip repeat pk
                let flags = if name == pk { vec![FieldFlag::Pk] } else { Vec::new() };
                fields.push(SchemaField { name, field_type: ftype, flags });
            }
        }
        if depth <= 0 && in_block { break; }
    }
    fields
}

fn detect_prisma(content: &str, _file: &str, models: &mut Vec<SchemaModel>) {
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        if let Some(cap) = prisma_model().captures(lines[i]) {
            let name = cap[1].to_string();
            let mut fields = Vec::new();
            i += 1;

            while i < lines.len() && !lines[i].trim_start().starts_with('}') {
                if let Some(fcap) = prisma_field().captures(lines[i]) {
                    let fname = fcap[1].to_string();
                    let ftype = fcap[2].to_string();
                    if fname == "@@" { i += 1; continue; }

                    let mut flags = Vec::new();
                    if lines[i].contains("@id")     { flags.push(FieldFlag::Pk); }
                    if lines[i].contains("@unique")  { flags.push(FieldFlag::Unique); }
                    if lines[i].contains("@default") { flags.push(FieldFlag::Default); }
                    if ftype.ends_with('?')          { flags.push(FieldFlag::Nullable); }

                    fields.push(SchemaField {
                        name: fname,
                        field_type: ftype.trim_end_matches('?').to_string(),
                        flags,
                    });
                }
                i += 1;
            }

            models.push(SchemaModel {
                name,
                table_name: None,
                fields,
                relations: Vec::new(),
                orm: Orm::Prisma,
                confidence: Confidence::Regex,
            });
        }
        i += 1;
    }
}

fn detect_drizzle(content: &str, _file: &str, models: &mut Vec<SchemaModel>) {
    for cap in drizzle_table().captures_iter(content) {
        let const_name = cap[1].to_string();
        let table_name = cap[2].to_string();
        models.push(SchemaModel {
            name: to_pascal_case(&const_name),
            table_name: Some(table_name),
            fields: Vec::new(),
            relations: Vec::new(),
            orm: Orm::Drizzle,
            confidence: Confidence::Regex,
        });
    }
}

fn detect_sqlalchemy(content: &str, _file: &str, models: &mut Vec<SchemaModel>) {
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        if let Some(cap) = sqlalchemy_class().captures(lines[i]) {
            let name = cap[1].to_string();
            let mut table_name: Option<String> = None;

            // Look ahead for __tablename__
            for j in (i + 1)..lines.len().min(i + 10) {
                if let Some(tc) = sqlalchemy_tablename().captures(lines[j]) {
                    table_name = Some(tc[1].to_string());
                    break;
                }
                if lines[j].trim().is_empty() { break; }
            }

            models.push(SchemaModel {
                name,
                table_name,
                fields: Vec::new(),
                relations: Vec::new(),
                orm: Orm::SqlAlchemy,
                confidence: Confidence::Regex,
            });
        }
        i += 1;
    }
}

fn detect_gorm(content: &str, _file: &str, models: &mut Vec<SchemaModel>) {
    for cap in gorm_struct().captures_iter(content) {
        let name = cap[1].to_string();
        // Skip non-model structs (heuristic: only capture if content has gorm tags)
        if content.contains("gorm:") {
            models.push(SchemaModel {
                name,
                table_name: None,
                fields: Vec::new(),
                relations: Vec::new(),
                orm: Orm::Gorm,
                confidence: Confidence::Regex,
            });
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().to_string() + chars.as_str(),
            }
        })
        .collect()
}
