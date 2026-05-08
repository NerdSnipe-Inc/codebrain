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



