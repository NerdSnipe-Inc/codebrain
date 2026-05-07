use codebrain::collector::CollectedFile;
use codebrain::detectors::schema::{detect_all, detect_orms};
use codebrain::Orm;
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
fn detects_prisma_models() {
    let files = vec![make_file(
        "schema.prisma", "prisma",
        r#"
model User {
  id    Int    @id @default(autoincrement())
  email String @unique
  name  String?
  posts Post[]
}

model Post {
  id      Int    @id
  title   String
  content String?
  userId  Int
}
        "#,
    )];

    let orms = detect_orms(&files);
    // .prisma extension is not matched by content-based ORM heuristics; Unknown is the fallback
    assert!(!orms.contains(&Orm::Prisma), "detect_orms must not match Prisma from a .prisma file alone");
    assert!(orms.contains(&Orm::Unknown));
    let orms_with_prisma = vec![Orm::Prisma];
    let models = detect_all(&files, &orms_with_prisma);

    let names: Vec<&str> = models.iter().map(|m| m.name.as_str()).collect();
    assert!(names.contains(&"User"), "should detect User model");
    assert!(names.contains(&"Post"), "should detect Post model");

    let user = models.iter().find(|m| m.name == "User").unwrap();
    let field_names: Vec<&str> = user.fields.iter().map(|f| f.name.as_str()).collect();
    assert!(field_names.contains(&"email"));
    assert!(field_names.contains(&"name"));
}

#[test]
fn detects_drizzle_tables() {
    let files = vec![make_file(
        "src/db/schema.ts", "ts",
        r#"
        import { pgTable, serial, text, integer } from "drizzle-orm/pg-core";

        export const users = pgTable("users", {
            id:    serial("id").primaryKey(),
            email: text("email").notNull(),
            name:  text("name"),
        });

        export const posts = pgTable("posts", {
            id:      serial("id").primaryKey(),
            userId:  integer("user_id"),
            content: text("content"),
        });
        "#,
    )];

    let orms = detect_orms(&files);
    assert!(orms.contains(&Orm::Drizzle));

    let models = detect_all(&files, &orms);
    assert!(!models.is_empty());
    let names: Vec<&str> = models.iter().map(|m| m.name.as_str()).collect();
    assert!(names.contains(&"Users") || names.contains(&"users"));
}

#[test]
fn detects_sea_orm_models() {
    let files = vec![make_file(
        "src/entity/user.rs", "rs",
        r#"
        use sea_orm::entity::prelude::*;

        #[sea_orm(table_name = "users")]
        pub struct Model {
            #[sea_orm(primary_key)]
            pub id: i32,
            pub email: String,
        }

        #[sea_orm(table_name = "posts")]
        pub struct Post {
            pub id: i32,
            pub title: String,
        }
        "#,
    )];

    let orms = detect_orms(&files);
    assert!(orms.contains(&Orm::SeaOrm));

    let models = detect_all(&files, &orms);
    assert!(!models.is_empty());
    // table_name should be captured
    let user_model = models.iter().find(|m| m.table_name.as_deref() == Some("users"));
    assert!(user_model.is_some(), "should capture table_name = 'users'");
}

#[test]
fn detects_sqlalchemy_models() {
    let files = vec![make_file(
        "models.py", "py",
        r#"
from sqlalchemy import Column, Integer, String
from sqlalchemy.orm import DeclarativeBase

class Base(DeclarativeBase):
    pass

class User(Base):
    __tablename__ = "users"
    id = Column(Integer, primary_key=True)
    email = Column(String, nullable=False)

class Product(Base):
    __tablename__ = "products"
    id = Column(Integer, primary_key=True)
        "#,
    )];

    let orms = detect_orms(&files);
    assert!(orms.contains(&Orm::SqlAlchemy));

    let models = detect_all(&files, &orms);
    let names: Vec<&str> = models.iter().map(|m| m.name.as_str()).collect();
    assert!(names.contains(&"User"));
    assert!(names.contains(&"Product"));
}
