use codebrain::detectors::routes::{detect_all, detect_frameworks};
use codebrain::collector::CollectedFile;
use codebrain::Framework;
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
fn detects_axum_routes() {
    let files = vec![make_file(
        "src/router.rs", "rs",
        r#"
        use axum::Router;
        fn build() -> Router {
            Router::new()
                .route("/api/users", get(list_users))
                .route("/api/users/:id", post(create_user))
                .route("/health", get(health))
        }
        "#,
    )];

    let frameworks = detect_frameworks(&files);
    assert!(frameworks.contains(&Framework::Axum), "should detect Axum");

    let routes = detect_all(&files, &frameworks);
    assert!(!routes.is_empty(), "should detect routes");

    let paths: Vec<&str> = routes.iter().map(|r| r.path.as_str()).collect();
    assert!(paths.contains(&"/api/users"), "should detect /api/users");
    assert!(paths.contains(&"/health"), "should detect /health");
}

#[test]
fn detects_actix_attribute_routes() {
    let files = vec![make_file(
        "src/handlers.rs", "rs",
        r#"
        use actix_web::{get, post, HttpResponse};

        #[get("/api/products")]
        async fn list_products() -> HttpResponse { todo!() }

        #[post("/api/products")]
        async fn create_product() -> HttpResponse { todo!() }
        "#,
    )];

    let frameworks = detect_frameworks(&files);
    assert!(frameworks.contains(&Framework::Actix));

    let routes = detect_all(&files, &frameworks);
    let methods: Vec<&str> = routes.iter().map(|r| r.method.as_str()).collect();
    assert!(methods.contains(&"GET"));
    assert!(methods.contains(&"POST"));
}

#[test]
fn detects_express_routes() {
    let files = vec![make_file(
        "src/routes.ts", "ts",
        r#"
        import express from "express";
        const router = express.Router();

        router.get("/api/items", listItems);
        router.post("/api/items", createItem);
        router.delete("/api/items/:id", deleteItem);
        "#,
    )];

    let frameworks = detect_frameworks(&files);
    assert!(frameworks.contains(&Framework::Express));

    let routes = detect_all(&files, &frameworks);
    assert_eq!(routes.len(), 3);
    let paths: Vec<&str> = routes.iter().map(|r| r.path.as_str()).collect();
    assert!(paths.contains(&"/api/items"));
}

#[test]
fn detects_fastapi_routes() {
    let files = vec![make_file(
        "main.py", "py",
        r#"
        from fastapi import FastAPI, APIRouter

        app = FastAPI()
        router = APIRouter()

        @app.get("/")
        def root(): pass

        @router.post("/api/orders")
        def create_order(): pass
        "#,
    )];

    let frameworks = detect_frameworks(&files);
    assert!(frameworks.contains(&Framework::FastApi));

    let routes = detect_all(&files, &frameworks);
    assert!(!routes.is_empty());
    let paths: Vec<&str> = routes.iter().map(|r| r.path.as_str()).collect();
    assert!(paths.contains(&"/"));
    assert!(paths.contains(&"/api/orders"));
}

#[test]
fn detects_gin_routes() {
    let files = vec![make_file(
        "main.go", "go",
        r#"
        import "github.com/gin-gonic/gin"

        func main() {
            r := gin.Default()
            r.GET("/ping", pingHandler)
            r.POST("/api/users", createUser)
        }
        "#,
    )];

    let frameworks = detect_frameworks(&files);
    assert!(frameworks.contains(&Framework::Gin));

    let routes = detect_all(&files, &frameworks);
    assert!(!routes.is_empty());
}

#[test]
fn no_routes_for_empty_file() {
    let files = vec![make_file("src/lib.rs", "rs", "// empty")];
    let frameworks = detect_frameworks(&files);
    let routes = detect_all(&files, &frameworks);
    assert!(routes.is_empty());
}
