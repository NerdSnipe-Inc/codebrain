//! `codebrain-mcp` — MCP server for Code Brain codebase intelligence.
//!
//! Exposes 9 tools to any MCP-aware client (Claude Code, Cursor, Windsurf, etc.):
//!
//!   codebrain_scan            — force re-scan the project
//!   codebrain_context         — BFS context block for a query
//!   codebrain_context_dfs     — DFS context block for a query
//!   codebrain_blast_radius    — what breaks if this file changes
//!   codebrain_symbol_callers  — specific callers of a function or type
//!   codebrain_routes          — all detected HTTP routes
//!   codebrain_schemas         — all detected database schemas/models
//!   codebrain_god_nodes       — most-connected nodes (architectural hot spots)
//!   codebrain_communities     — Louvain community clusters + cross-community edges
//!
//! # Usage
//!
//! ```bash
//! codebrain-mcp --project /path/to/project
//! ```
//!
//! With no `--project` flag, defaults to the current working directory.
//!
//! # Claude Code integration (per-project .mcp.json)
//!
//! Create `.mcp.json` in the project root:
//! ```json
//! {
//!   "mcpServers": {
//!     "codebrain": {
//!       "type": "stdio",
//!       "command": "codebrain-mcp",
//!       "args": ["--project", "."]
//!     }
//!   }
//! }
//! ```
//!
//! # Global Claude Code config (~/.claude/settings.json)
//!
//! ```json
//! {
//!   "mcpServers": {
//!     "codebrain": {
//!       "type": "stdio",
//!       "command": "codebrain-mcp",
//!       "args": ["--project", "/absolute/path/to/project"]
//!     }
//!   }
//! }
//! ```
//!
//! # Logs
//!
//! All tracing output goes to **stderr** so it never corrupts the stdout
//! JSON-RPC stream. Set `RUST_LOG=debug` to see detailed request tracing.
//! Default log level is `warn` (only errors and warnings).

mod protocol;
mod server;
mod tools;

use std::path::PathBuf;

use codebrain::{CodeBrainConfig, CodeBrainHandle};

fn main() -> anyhow::Result<()> {
    // All logs to stderr — stdout is reserved for the JSON-RPC protocol.
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};
    tracing_subscriber::registry()
        .with(
            fmt::layer()
                .with_writer(std::io::stderr)
                .with_target(false)
                .with_ansi(false), // no ANSI in stderr when piped
        )
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();

    // `--help` / `-h`
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return Ok(());
    }

    // `--version` / `-V`
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("codebrain-mcp {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let project = parse_flag(&args, "--project")
        .or_else(|| parse_flag(&args, "-p"))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));

    if !project.exists() {
        anyhow::bail!(
            "project path does not exist: {}\n\
             Use --project /path/to/your/project",
            project.display()
        );
    }

    let project = project.canonicalize().unwrap_or(project);

    tracing::info!(
        project = %project.display(),
        version = env!("CARGO_PKG_VERSION"),
        "codebrain-mcp starting"
    );

    let config = CodeBrainConfig::new(&project);
    let handle = CodeBrainHandle::new(config);

    server::run_stdio(handle)
}

// ── CLI helpers ───────────────────────────────────────────────────────────────

fn parse_flag(args: &[String], flag: &str) -> Option<PathBuf> {
    let pos = args.iter().position(|a| a == flag)?;
    args.get(pos + 1).map(PathBuf::from)
}

fn print_help() {
    println!(
        "codebrain-mcp {version}

MCP server exposing Code Brain knowledge graph queries to any MCP-aware agent.

USAGE:
    codebrain-mcp [OPTIONS]

OPTIONS:
    -p, --project <PATH>   Path to the project root to analyse
                           (default: current working directory)
    -h, --help             Print this help message
    -V, --version          Print version

ENVIRONMENT:
    RUST_LOG               Log level for stderr output (default: warn)
                           Set to 'debug' to see all request tracing.

CLAUDE CODE INTEGRATION:
    Add to .mcp.json in your project root:

    {{
      \"mcpServers\": {{
        \"codebrain\": {{
          \"type\": \"stdio\",
          \"command\": \"codebrain-mcp\",
          \"args\": [\"--project\", \".\"]
        }}
      }}
    }}

TOOLS EXPOSED:
    codebrain_scan            Force re-scan the project
    codebrain_context         BFS context block for a natural-language query
    codebrain_context_dfs     DFS context block (traces call chains)
    codebrain_blast_radius    Files affected if a given file changes
    codebrain_symbol_callers  Callers of a specific function or type (symbol-level)
    codebrain_routes          All detected HTTP routes
    codebrain_schemas         All detected database schemas and models
    codebrain_god_nodes       Most-connected nodes (architectural hot spots)
    codebrain_communities     Louvain community clusters + cross-community edges

FIRST-RUN NOTE:
    The first tool call triggers a project scan. For large projects this may
    take a few seconds. Subsequent calls use the in-memory cache and the
    SHA-256 extraction cache at ~/.cache/codebrain/ast-cache/.",
        version = env!("CARGO_PKG_VERSION"),
    );
}
