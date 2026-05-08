# Code Brain

A codebase intelligence engine that gives AI agents a structured knowledge graph instead of a bag of files.

Code Brain scans your project, extracts a dependency and symbol graph via tree-sitter AST parsing, detects HTTP routes and database schemas, and exposes everything through an MCP server. Claude Code, Cursor, Windsurf, Zed, and any other MCP-aware agent can query the graph to orient themselves before reading or modifying code — without consuming large amounts of context window on raw file contents.

## What It Does

Most AI agents working on a codebase start blind. They read files one at a time, rebuild mental models from scratch each session, and often miss the connections between components. The first 10–20% of every session is wasted on orientation. Code Brain eliminates that.

When an agent calls `codebrain_god_nodes`, it gets back the ten most-connected abstractions in the codebase — the things everything else depends on. Before touching a shared utility, `codebrain_blast_radius` tells the agent exactly which files, routes, and models would be affected by a change. When an agent needs context on a specific feature area, `codebrain_context` runs a BFS over the knowledge graph from the most relevant nodes and returns a token-budgeted summary rather than dumping entire files into context.

The graph contains: every function, struct, class, and method extracted by tree-sitter; every import/call/contains/implements edge between them; every detected HTTP route with its method, path, framework, and source file; every ORM model with its fields and table name; and a flat import dependency graph used for blast radius analysis. All of this is built locally — no API calls, no network access, no keys required.

The difference between raw file dumps and a knowledge graph is the difference between giving someone a pile of paper and giving them an org chart. The graph tells you what matters, what connects to what, and where to look first.

## Tools

### `codebrain_scan`

Force a full re-scan of the project and rebuild the knowledge graph. The first call to any other tool triggers a scan automatically; call this explicitly after significant code changes.

| Argument | Type | Required | Description |
|----------|------|----------|-------------|
| (none) | | | |

**Example:**
```json
{"name": "codebrain_scan", "arguments": {}}
```

**Example output:**
```
Scan complete.
Project: my-api
Files scanned: 142
Routes detected: 31
Schemas detected: 8
Graph nodes: 1847
Graph edges: 3291
```

---

### `codebrain_context`

BFS query: returns a token-budgeted context block anchored to codebase nodes that match the query. Best for understanding a feature area, module, or component neighbourhood.

| Argument | Type | Required | Description |
|----------|------|----------|-------------|
| `query` | string | yes | Natural language query, e.g. `"authentication flow"` |
| `max_tokens` | integer | no | Token budget (default: 2000) |

**Example:**
```json
{"name": "codebrain_context", "arguments": {"query": "payment processing", "max_tokens": 1500}}
```

**Example output:**
```
=== Graph context for "payment processing" (12 nodes, BFS) ===
  [fn] process_payment [src/billing/payment.rs:45] → charge_card, log_transaction, notify_user
  [fn] charge_card [src/billing/stripe.rs:12] → create_intent, handle_error
  [struct] PaymentIntent [src/billing/types.rs:8]
  [route] POST /api/payments [src/billing/handlers.rs]
  ...
```

---

### `codebrain_context_dfs`

DFS query: traverses call chains depth-first from matching nodes. Better than BFS for tracing how control flows through the codebase — following a request from handler to database, for example.

| Argument | Type | Required | Description |
|----------|------|----------|-------------|
| `query` | string | yes | Natural language query, e.g. `"request handler pipeline"` |
| `max_tokens` | integer | no | Token budget (default: 2000) |

**Example:**
```json
{"name": "codebrain_context_dfs", "arguments": {"query": "user authentication"}}
```

**Example output:**
```
=== Graph context for "user authentication" (8 nodes, DFS) ===
  [fn] authenticate [src/auth/middleware.rs:23] → verify_token, load_user
  [fn] verify_token [src/auth/jwt.rs:11] → decode_jwt, check_expiry
  [fn] decode_jwt [src/auth/jwt.rs:34]
  ...
```

---

### `codebrain_blast_radius`

Returns all files and components that would be affected if the specified file changes. Runs a reverse BFS over the import dependency graph to find everything that directly or transitively imports the target file, then overlays which HTTP routes and models are served by those files.

| Argument | Type | Required | Description |
|----------|------|----------|-------------|
| `path` | string | yes | Relative file path, e.g. `"src/auth.rs"` |
| `depth` | integer | no | Traversal depth (default: 3) |

**Example:**
```json
{"name": "codebrain_blast_radius", "arguments": {"path": "src/db/pool.rs", "depth": 4}}
```

**Example output:**
```
Blast radius for 'src/db/pool.rs' (depth 4):

Affected files (14):
  src/db/pool.rs
  src/user/repository.rs
  src/billing/repository.rs
  src/auth/session.rs
  ...

Affected routes (6):
  GET /api/users (src/user/handlers.rs)
  POST /api/users (src/user/handlers.rs)
  GET /api/payments (src/billing/handlers.rs)
  ...
```

---

### `codebrain_routes`

Returns all detected HTTP routes with method, path, handler file, and framework. Optionally filter by HTTP method or path prefix.

| Argument | Type | Required | Description |
|----------|------|----------|-------------|
| `method` | string | no | Filter by HTTP method: `GET`, `POST`, `PUT`, `DELETE`, `PATCH` |
| `path_prefix` | string | no | Filter to routes starting with this prefix, e.g. `"/api/v2"` |

**Example:**
```json
{"name": "codebrain_routes", "arguments": {"method": "POST"}}
```

**Example output:**
```
8 route(s) detected:

  POST   /api/users                               [Axum]  src/handlers/user.rs
  POST   /api/payments                            [Axum]  src/handlers/billing.rs
  POST   /api/auth/login                          [Axum]  src/handlers/auth.rs
  ...
```

---

### `codebrain_schemas`

Returns all detected database schemas and ORM models with their fields and relations. Optionally filter to a specific model by name.

| Argument | Type | Required | Description |
|----------|------|----------|-------------|
| `model_name` | string | no | Filter to a specific model (case-insensitive) |

**Example:**
```json
{"name": "codebrain_schemas", "arguments": {}}
```

**Example output:**
```
5 schema(s) detected:

  User [SeaOrm]  table: users
    fields: id, email, name, created_at, role
  Post [SeaOrm]  table: posts
    fields: id, user_id, title, content, published_at
  ...
```

---

### `codebrain_god_nodes`

Returns the top-N most-connected nodes in the knowledge graph — the load-bearing abstractions that most of the codebase depends on. Call this at the start of a session to understand the architecture before making changes.

| Argument | Type | Required | Description |
|----------|------|----------|-------------|
| `n` | integer | no | Number of nodes to return (default: 10, max: 50) |

**Example:**
```json
{"name": "codebrain_god_nodes", "arguments": {"n": 5}}
```

**Example output:**
```
Top 5 most-connected nodes (architectural hot spots):

  [struct] DatabasePool  (34 edges)
    src/db/pool.rs:8
  [fn] authenticate  (28 edges)
    src/auth/middleware.rs:23
  [struct] AppState  (22 edges)
    src/state.rs:12
  [fn] handle_error  (19 edges)
    src/error.rs:45
  [struct] UserRepository  (16 edges)
    src/user/repository.rs:5

Total graph: 1847 nodes, 3291 edges
```

---

## Community Detection

Code Brain runs the **Louvain modularity algorithm** on the knowledge graph after every scan to automatically partition the codebase into communities — groups of nodes that are densely connected internally and loosely connected to the rest.

### What it does

The algorithm optimises modularity Q over the graph in two phases:

**Phase 1 — Local moves.** For each node, compute the modularity gain ΔQ of moving it into each of its neighbours' communities. Move to the community with the highest positive ΔQ. Repeat until no move improves Q.

**Phase 2 — Aggregation.** Collapse each community into a single super-node. Sum edge weights between super-nodes. Run Phase 1 on the resulting meta-graph. Map labels back to original nodes.

Directed edges are treated as undirected (degree = in + out) so the algorithm works equally well on call graphs, import graphs, and mixed graphs.

### What you get

**Community labels in context output.** Every node returned by `codebrain_context`, `codebrain_context_dfs`, and `codebrain_god_nodes` carries a community id. The formatter labels each community by the most common top-level source directory among its members.

**Community summary.** When `codebrain_god_nodes` runs, it includes a community breakdown:

```
Top 5 most-connected nodes (architectural hot spots):

  [struct] DatabasePool  (34 edges)  [community: db (47 nodes)]
    src/db/pool.rs:8
  [fn] authenticate  (28 edges)  [community: auth (23 nodes)]
    src/auth/middleware.rs:23
  ...

Key communities:
  db (47 nodes) — src/db, src/repositories
  auth (23 nodes) — src/auth, src/middleware
  api (31 nodes) — src/handlers, src/routes
  billing (18 nodes) — src/billing, src/payments
```

**Cross-community edges.** The graph tracks edges that cross community boundaries — call relationships between nodes in different clusters. These are architectural seams: places where tightly-knit modules are coupled to each other. They surface in context output as "cross-community connections" when present.

### Why it matters for agents

Without community structure, BFS and DFS traversals expand in all directions equally. With communities, queries can be scoped to relevant clusters, god node analysis tells you which communities are most central, and cross-community edges surface non-obvious architectural dependencies that are worth understanding before making changes.

The community assignment is stored in `NodeData.community` and persisted in the `GraphStore`, so re-runs don't re-cluster unless the graph changes.

---

## Agent Guidance (CLAUDE.md)

Registering Code Brain in `.mcp.json` makes the tools available, but agents default to familiar grep-based workflows unless they have explicit guidance on when and how to use the graph. Adding a short block to your project's `CLAUDE.md` closes that gap.

The guidance below is based on [real benchmark results](#real-world-benchmarks) showing a 23–40% reduction in tool calls on complex tasks when agents follow the orient-first workflow.

### Recommended CLAUDE.md block

Copy this into your project's `CLAUDE.md`:

```markdown
## Code Brain — codebase knowledge graph

Code Brain is available via MCP. Use it as your first-pass orientation tool on
any task that involves unfamiliar code, cross-file flows, or impact analysis.

### When to use Code Brain

- **Start of session** — call `codebrain_god_nodes` before reading any files.
  It returns the top 10 most-connected abstractions and tells you the architecture
  at a glance. One call, no token cost from reading files.
- **Cross-file or cross-package tasks** — use `codebrain_context` or
  `codebrain_context_dfs` to locate relevant files and their relationships in
  1–2 calls instead of iterating through grep results.
- **Before modifying shared utilities** — always call `codebrain_blast_radius`
  before changing a file that other code imports. It shows exactly which routes,
  models, and callers will be affected.
- **Cold start / unknown codebase** — use `codebrain_context` to answer "where
  does X happen?" without knowing filenames or package structure first.

### When to use Grep / Glob / Read directly

- You know the exact symbol name or filename you're looking for.
- The task is contained within a single, obviously-named module.
- You need a filesystem audit (dead code, duplicate files, naming issues).

### Optimal workflow

1. `codebrain_god_nodes` or `codebrain_context` — orient and identify candidate
   files (1–2 calls)
2. `Read` — verify the specific files Code Brain identified
3. `Grep` / `Glob` — catch edge cases or filesystem-level details
4. `codebrain_blast_radius` — before touching any shared or core file

### Keep the graph current

The graph is built once per session. Call `codebrain_scan` explicitly after
significant structural changes (renaming files, extracting modules, adding
packages) so the graph reflects the current state.
```

### Minimal version

If you only want the blast-radius reminder without the full workflow guidance:

```markdown
## Code Brain

`codebrain_god_nodes` — call at session start to orient.  
`codebrain_context` — use before grepping for any unfamiliar feature area.  
`codebrain_blast_radius` — required before modifying any shared/core file.  
`codebrain_scan` — call after structural refactors to refresh the graph.
```

### Why this matters

Without explicit guidance, agents treat Code Brain as an optional extra and default to grep because it always works. The benchmark showed Code Brain only outperforms raw exploration when agents use it *first* — not as a fallback after grep fails. CLAUDE.md shifts the default.

---

## Supported Languages & Frameworks

### Languages (AST extraction via tree-sitter)

| Language | Extensions | Extracted |
|----------|------------|-----------|
| Rust | `.rs` | functions, structs, impl blocks, modules, use declarations, call edges |
| TypeScript | `.ts`, `.tsx` | functions, classes, methods, arrow functions, import edges |
| JavaScript | `.js`, `.jsx` | functions, classes, methods, import edges |
| Python | `.py` | functions, classes, methods, import edges, inheritance |
| Go | `.go` | functions, methods, structs, interfaces, import edges |
| Swift | `.swift` | classes, structs, enums, protocols, extensions, functions, methods, conformance edges (regex-based) |

### Web Frameworks (route detection)

Axum, Actix-web, Express, Fastify, Hono, NestJS, Next.js, FastAPI, Flask, Django, Gin, Fiber, Echo

### ORMs / Schema systems (schema extraction)

SeaORM, Diesel, Prisma, Drizzle, TypeORM, SQLAlchemy, GORM

---

## Installation

### Option 1: Build from source

Requires Rust 1.75 or later.

```bash
git clone https://github.com/NerdSnipe-Inc/codebrain
cd code-brain
cargo build --release -p codebrain-mcp
# Binary is at ./target/release/codebrain-mcp
cp target/release/codebrain-mcp ~/.local/bin/
```

### Option 2: Cargo install

```bash
cargo install codebrain-mcp
```

This installs the binary to `~/.cargo/bin/codebrain-mcp`, which should already be on your PATH if you have Rust installed.

---

## Setup: Claude Code

Claude Code has first-class MCP support. Code Brain works best as a per-project server, but can also be configured globally.

### Per-project setup (recommended)

Create `.mcp.json` in your project root. Claude Code picks it up automatically when you open the project.

```json
{
  "mcpServers": {
    "codebrain": {
      "type": "stdio",
      "command": "codebrain-mcp",
      "args": ["--project", "."]
    }
  }
}
```

The `"."` tells Code Brain to analyse the directory Claude Code is running from — your project root. This is the right default for per-project configs.

### Global setup

To use Code Brain with any project without a per-project config file, add it to `~/.claude/settings.json`. Use an absolute path in the `--project` argument, or omit it and let Code Brain default to the current working directory (which Claude Code sets to the project root at startup).

```json
{
  "mcpServers": {
    "codebrain": {
      "type": "stdio",
      "command": "codebrain-mcp",
      "args": ["--project", "/absolute/path/to/your/project"]
    }
  }
}
```

If `codebrain-mcp` is not on your PATH in the shell environment Claude Code uses, specify the full path to the binary instead.

### Tips for Claude Code users

**Start sessions with `codebrain_god_nodes`.** Before reading any files, call this tool to see the top 10 most-connected abstractions. It tells you the architecture at a glance and identifies the files that matter most.

**Use `codebrain_blast_radius` before editing shared utilities.** If you're about to change a helper, type, or interface that other code imports, run blast radius first. It surfaces which routes will be affected and how far the change ripples.

**Use `codebrain_context_dfs` to trace request pipelines.** When you need to understand how a request flows from an HTTP handler through middleware, business logic, and database layer, DFS traversal follows the call chain more naturally than BFS.

**Re-scan after significant refactors.** The knowledge graph is built once per session (and cached). Call `codebrain_scan` explicitly after renaming files, extracting modules, or making other structural changes so the graph reflects the current state.

---

## Setup: Cursor

Add to `~/.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "codebrain": {
      "type": "stdio",
      "command": "codebrain-mcp",
      "args": ["--project", "/path/to/your/project"]
    }
  }
}
```

Cursor's MCP support is configured globally. Use the absolute path to your project, or start `codebrain-mcp` without `--project` to use the current working directory (Cursor sets this to the workspace root).

---

## Setup: Windsurf

Add to `~/.codeium/windsurf/mcp_config.json`:

```json
{
  "mcpServers": {
    "codebrain": {
      "command": "codebrain-mcp",
      "args": ["--project", "/path/to/your/project"]
    }
  }
}
```

---

## Setup: Zed

Add to your Zed `settings.json` under the `context_servers` key:

```json
{
  "context_servers": {
    "codebrain": {
      "command": {
        "path": "codebrain-mcp",
        "args": ["--project", "/path/to/your/project"]
      }
    }
  }
}
```

---

## Setup: Other MCP clients

Code Brain uses the stdio transport (newline-delimited JSON-RPC 2.0), which is the standard MCP transport. Any MCP-compatible client can connect by spawning `codebrain-mcp` as a subprocess and communicating over stdin/stdout. All tracing and log output goes to stderr, so it never interferes with the protocol stream.

---

## Usage

```bash
# Analyse the current directory
codebrain-mcp

# Analyse a specific project
codebrain-mcp --project /path/to/project

# Short form
codebrain-mcp -p /path/to/project

# Debug logging (to stderr)
RUST_LOG=debug codebrain-mcp --project .

# Version
codebrain-mcp --version
```

---

## Performance

Code Brain uses a two-layer cache to stay fast on repeated runs.

| Operation | Cold (first run) | Warm (cached) |
|-----------|-----------------|---------------|
| `codebrain_scan` (small project, ~50 files) | ~200ms | ~5ms |
| `codebrain_scan` (medium project, ~500 files) | ~2–4s | ~20ms |
| `codebrain_scan` (large project, ~2000 files) | ~10–20s | ~80ms |
| `codebrain_context` / `codebrain_context_dfs` | ~1ms after scan | ~1ms |
| `codebrain_blast_radius` | ~1ms after scan | ~1ms |
| `codebrain_god_nodes` | ~1ms after scan | ~1ms |

**In-memory cache:** The `CodeBrainHandle` caches the full scan result and knowledge graph in memory. Subsequent tool calls within the same session return immediately without re-scanning.

**SHA-256 AST extraction cache:** Individual file parse results are stored as `{sha256-hash}.json` at `~/.cache/codebrain/ast-cache/`. Files whose content has not changed since the last scan are loaded from this cache rather than re-parsed by tree-sitter. Only changed files incur parsing overhead on re-scans.

---

## Roadmap

Known improvements, roughly prioritised by impact:

- [x] **Symbol-level blast radius** (`codebrain_symbol_callers`) — the existing `codebrain_blast_radius` operates at file granularity. If you change one function in a large shared file, it flags every importer of that file — including callers of unrelated symbols. Symbol-level callers walks `Calls` edges in the knowledge graph to return the specific functions that call the target symbol, not just the files that import its module.

- [x] **Community detection MCP surface** (`codebrain_communities`) — Louvain community detection runs after every scan and writes community ids into every node, but agents couldn't query the results directly. Added `codebrain_communities` to expose the community breakdown and cross-community edges.

- [ ] **Symbol-level blast radius: transitive call chain** — the current implementation (`codebrain_symbol_callers`) walks direct and transitive `Calls` edges. A future improvement would also traverse `Implements` and `InheritsFrom` edges so that trait/interface implementations surface as dependents when the interface contract changes.

- [ ] **Betweenness centrality for `codebrain_god_nodes`** — degree centrality (current) ranks frequently-called utility functions and error handlers as "god nodes" because everything calls them, not because they're architecturally load-bearing. Betweenness centrality (which nodes sit on the most shortest paths between other nodes) would surface genuinely structural components.

- [ ] **Incremental graph updates** — the graph is rebuilt fully on each `codebrain_scan`. The SHA-256 AST cache avoids re-parsing unchanged files, but nodes and edges are still reassembled from scratch. A file-watcher with incremental node replacement would let the graph stay current without manual re-scan calls after every code change.

- [ ] **tree-sitter-swift** — Swift extraction currently uses regex rather than tree-sitter due to grammar compatibility constraints. Switching to a tree-sitter grammar would make Swift extraction as reliable and complete as Rust, TypeScript, and Go.

- [ ] **Semantic seed nodes** — BFS/DFS queries are seeded by keyword matching against node labels and file names. If the actual symbol uses different terminology than the query, seeds are missed. A lightweight local embedding index (no API, computed at scan time) would enable semantic similarity seeding that is robust to naming convention differences.

- [ ] **Edge confidence propagation** — `EdgeConfidence::Ambiguous` exists in the type system but is not widely used. Regex-inferred edges (Swift, heuristic import resolution) should be marked as `Inferred` rather than `Extracted` so agents know which parts of the graph to verify before acting on them.

- [ ] **Duplicate file detection** — the benchmark showed the graph does not flag byte-identical files at different paths. A scan-time content-hash comparison across collected files would surface this class of structural problem.

---

## Real-World Benchmarks

Code Brain was benchmarked head-to-head against raw filesystem exploration (Grep / Glob / Read) across seven tasks of increasing complexity on a real production Swift codebase — the macOS app, an 8-package project with 435 files, 2,891 graph nodes, and 1,673 edges.

**Method:** Two independent sub-agents ran each task simultaneously. One was restricted to Code Brain tools only; the other to Grep/Glob/Read/Bash only. Tool call counts, token usage, and answer completeness were compared on identical tasks.

> **Note:** These results were captured before the Louvain community detection implementation. The algorithm was a stub at test time — all nodes were assigned to community 0, and cross-community edge analysis was unavailable. The current version with full two-phase Louvain is expected to improve architectural orientation tasks further, particularly on cold-start sessions where community structure guides the initial BFS.

---

### Simple Lookups (3 tasks)

For directly named targets — "where is `AgenticRegistry` defined?", "find the OAuth token refresh" — raw Grep wins every time. Code Brain's schema-load overhead (~1 tool call) is not amortised across a single lookup when the answer is a filename away.

**Verdict: use Grep/Glob for simple named lookups.**

---

### Complex Semantic Tasks (4 tasks)

These are the tasks Code Brain is built for: no single searchable keyword, requiring cross-file reasoning, dependency traversal, or architectural understanding.

| Task | CB calls | Raw calls | CB tokens | Raw tokens | Result |
|------|----------|-----------|-----------|------------|--------|
| A — Trace full chat send → streamed UI response | 13 | 12 | 44,623 | 51,961 | Tie |
| B — All files needed to add a new AI provider | 22 | 39 | 73,932 | 73,860 | CB wins (−44% calls) |
| C — Dashboard persistence read/write flow | 21 | 17 | 61,194 | 49,008 | Raw wins |
| D — Blast radius of a struct format change | 20 | 31 | 59,160 | 68,924 | CB wins (−35% calls) |
| **Totals** | **76** | **99** | **238,909** | **243,753** | |

**Answer quality was identical across all 8 agents (4 tasks × 2 strategies). All rated 5/5 confidence. No critical file was missed by either strategy on the primary answer.**

---

### Key Findings

**23% fewer tool calls overall on complex tasks** (76 vs 99). The advantage scales with dependency complexity:

| Task type | Call reduction |
|-----------|---------------|
| All complex tasks | −23% |
| Dependency / impact analysis (Tasks B + D) | −40% |
| Self-contained well-named modules (Tasks A + C) | +17% (CB slower) |

**Token usage is essentially equivalent** (−2% total). Code Brain does not save tokens on complex tasks — it saves *round trips*. Fewer calls to reach the same quality answer means less agent back-and-forth, not smaller context.

**`codebrain_blast_radius` is the standout tool.** On Task D (blast radius of a struct change), Code Brain located all 6 affected files across 4 dependency layers in 2 `blast_radius` calls. Raw grep required 8+ grep iterations across different symbol permutations to reach equivalent confidence. This is the canonical use case for a code graph.

**Code Brain missed one filesystem anomaly.** The raw agent discovered a duplicate `DashboardLayout.swift` file existing byte-identically at two different paths. Code Brain indexed both but its BFS did not flag the duplication. For filesystem audits, raw tooling is superior.

---

### When to Use Each Strategy

**Use Code Brain when:**
- You're starting cold with no prior context on the codebase
- The task spans multiple packages or unknown file locations
- You need dependency / impact analysis ("what does changing X break?")
- You're tracing a cross-cutting flow (auth pipeline, request lifecycle, event system)

**Use Grep / Glob / Read when:**
- You know the symbol name or filename
- The task is contained within a single, well-named module
- You need an exhaustive filesystem audit (dead code, duplicate files, naming consistency)

**Optimal combined approach:**
```
1. codebrain_context or codebrain_god_nodes — orient and identify candidate files (1–2 calls)
2. Read — verify the files Code Brain identified
3. Grep/Glob — catch edge cases or filesystem-level details
4. codebrain_blast_radius — before modifying any shared or core file
```

---

## Architecture

Code Brain is split into two crates:

**`codebrain`** (library) — the core engine. Collects source files, runs framework/ORM detection with static regexes, builds a flat import dependency graph, runs tree-sitter AST extraction per file, assembles everything into a petgraph `DiGraph`, and provides BFS/DFS query methods. No LLM dependency, no network access, no API keys. Can be embedded in other tools as a library.

**`codebrain-mcp`** (binary) — the MCP server. Wraps the library behind a JSON-RPC 2.0 stdio transport. Reads from stdin, dispatches to the library, writes responses to stdout. Single-threaded, blocking — no async runtime. All tracing goes to stderr.

The graph topology: nodes are `NodeData` (id, label, type, file, line, community); edges are `GraphEdge` (from_id, to_id, kind, confidence). The `GraphStore` type is the serialisable snapshot stored between runs. `CodeGraph` is the in-memory petgraph wrapper rebuilt from `GraphStore` on startup. After each scan, `assign_communities` runs the Louvain algorithm on the graph and writes community ids into each node — no extra step required.

AST extraction uses tree-sitter 0.22 with grammars for Rust, TypeScript, JavaScript, Python, and Go. Swift uses regex extraction due to grammar compatibility constraints.

MCP protocol version: `2024-11-05`. Transport: stdio (newline-delimited JSON-RPC 2.0).

---

## Contributing

Bug reports and pull requests are welcome. Open an issue first for significant changes.

**Requirements:** Rust 1.75 or later.

```bash
# Run all tests
cargo test --workspace

# Run library tests only
cargo test -p codebrain

# Check formatting
cargo fmt --check

# Lint
cargo clippy --workspace
```

The test suite uses `tempfile` for isolated project roots and does not require network access or API keys.

---

## License

MIT — see [LICENSE](LICENSE).
