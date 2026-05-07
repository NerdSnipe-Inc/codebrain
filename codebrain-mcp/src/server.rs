//! MCP stdio transport loop.
//!
//! Reads newline-delimited JSON-RPC 2.0 from stdin, dispatches to handlers,
//! writes responses to stdout. All tracing goes to stderr so it never
//! corrupts the stdout protocol stream.
//!
//! # Request flow
//!
//! ```text
//! Client           Server (this file)           codebrain
//! ──────           ──────────────────           ─────────
//! initialize  →    handle_initialize()     →    (no-op)
//! initialized →    (notification, ignored)
//! tools/list  →    tool definitions JSON   →    (no-op)
//! tools/call  →    tools::call()           →    CodeBrainHandle methods
//! ```
//!
//! # Error handling
//!
//! - Parse error:    JSON-RPC error code -32700
//! - Unknown method: JSON-RPC error code -32601
//! - Bad params:     JSON-RPC error code -32602
//! - Tool errors:    `isError: true` in the tool result content (not JSON-RPC error)

use std::io::{BufRead, Write};

use serde_json::{json, Value};

use codebrain::CodeBrainHandle;

use crate::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::tools;

const PROTOCOL_VERSION: &str = "2024-11-05";

/// Run the MCP stdio server until stdin closes (EOF).
///
/// Reads one JSON line at a time, dispatches, writes response, flushes.
/// Blocking — intended to run on the main thread with no async runtime.
pub fn run_stdio(handle: CodeBrainHandle) -> anyhow::Result<()> {
    let stdin  = std::io::stdin();
    let stdout = std::io::stdout();

    // BufWriter batches the write + newline into one syscall per response.
    let mut out = std::io::BufWriter::new(stdout.lock());

    tracing::info!("codebrain-mcp ready — listening on stdin");

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l)  => l,
            Err(e) => {
                tracing::warn!(err = %e, "stdin read error");
                break;
            }
        };

        let trimmed = line.trim();
        if trimmed.is_empty() { continue; }

        let req: JsonRpcRequest = match serde_json::from_str(trimmed) {
            Ok(r)  => r,
            Err(e) => {
                tracing::warn!(err = %e, "JSON parse error");
                let resp = JsonRpcResponse::err(Value::Null, -32700, format!("Parse error: {e}"));
                writeln!(out, "{}", serde_json::to_string(&resp)?)?;
                out.flush()?;
                continue;
            }
        };

        tracing::debug!(method = %req.method, id = ?req.id, "request");

        // Notifications have no id — process silently, no response.
        if req.is_notification() {
            tracing::debug!(method = %req.method, "notification (no response)");
            continue;
        }

        let id   = req.id.clone().unwrap_or(Value::Null);
        let resp = dispatch(&req, id, &handle);

        writeln!(out, "{}", serde_json::to_string(&resp)?)?;
        out.flush()?;
    }

    tracing::info!("stdin closed — exiting");
    Ok(())
}

// ── Dispatcher ────────────────────────────────────────────────────────────────

fn dispatch(req: &JsonRpcRequest, id: Value, handle: &CodeBrainHandle) -> JsonRpcResponse {
    match req.method.as_str() {
        "initialize"  => handle_initialize(id),
        "ping"        => JsonRpcResponse::ok(id, json!({})),
        "tools/list"  => handle_tools_list(id),
        "tools/call"  => handle_tools_call(id, req.params.as_ref(), handle),
        _ => {
            tracing::debug!(method = %req.method, "unknown method");
            JsonRpcResponse::err(id, -32601, format!("Method not found: {}", req.method))
        }
    }
}

fn handle_initialize(id: Value) -> JsonRpcResponse {
    JsonRpcResponse::ok(id, json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": {
            "tools": {}
        },
        "serverInfo": {
            "name":    env!("CARGO_PKG_NAME"),
            "version": env!("CARGO_PKG_VERSION"),
        }
    }))
}

fn handle_tools_list(id: Value) -> JsonRpcResponse {
    JsonRpcResponse::ok(id, json!({ "tools": tools::definitions() }))
}

fn handle_tools_call(
    id:     Value,
    params: Option<&Value>,
    handle: &CodeBrainHandle,
) -> JsonRpcResponse {
    let params = match params {
        Some(p) => p,
        None    => return JsonRpcResponse::err(id, -32602, "tools/call: missing params"),
    };

    let name = match params.get("name").and_then(Value::as_str) {
        Some(n) => n.to_string(),
        None    => return JsonRpcResponse::err(id, -32602, "tools/call: missing 'name' field"),
    };

    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    tracing::debug!(tool = %name, "tool call");

    let result = tools::call(&name, &args, handle);
    JsonRpcResponse::ok(id, result)
}
