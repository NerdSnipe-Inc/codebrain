//! MCP JSON-RPC 2.0 wire types.
//!
//! The Model Context Protocol uses a subset of JSON-RPC 2.0 over stdio
//! (newline-delimited) or HTTP+SSE. This module covers the types needed for
//! the stdio transport:
//!
//!   Client → Server: `initialize`, `initialized` (notification), `ping`,
//!                    `tools/list`, `tools/call`
//!   Server → Client: corresponding responses
//!
//! Notifications have no `id` field and require no response.

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── Inbound ───────────────────────────────────────────────────────────────────

/// JSON-RPC 2.0 request or notification from the client.
///
/// `id` is absent on notifications; present (null, integer, or string) on
/// requests that require a response.
#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    #[allow(dead_code)] // read by serde for protocol validation
    pub jsonrpc: String,
    /// Missing for notifications. Null, integer, or string for requests.
    #[serde(default)]
    pub id:      Option<Value>,
    pub method:  String,
    #[serde(default)]
    pub params:  Option<Value>,
}

impl JsonRpcRequest {
    /// True for notifications — no response should be sent.
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

// ── Outbound ──────────────────────────────────────────────────────────────────

/// JSON-RPC 2.0 response.
#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    pub id:      Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result:  Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error:   Option<JsonRpcError>,
}

impl JsonRpcResponse {
    pub fn ok(id: Value, result: impl Serialize) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result:  Some(serde_json::to_value(result).unwrap_or(Value::Null)),
            error:   None,
        }
    }

    pub fn err(id: Value, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result:  None,
            error:   Some(JsonRpcError { code, message: message.into() }),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code:    i64,
    pub message: String,
}

// ── Tool result ───────────────────────────────────────────────────────────────

/// The MCP tool call result: a content array and an error flag.
///
/// Note: tool errors are encoded here as `isError: true` with the error text
/// in the content array — *not* as a JSON-RPC error. JSON-RPC errors are
/// reserved for protocol-level failures (bad method, bad params, etc.).
#[derive(Debug, Serialize)]
pub struct ToolResult {
    pub content:  Vec<ContentItem>,
    #[serde(rename = "isError")]
    pub is_error: bool,
}

impl ToolResult {
    pub fn text(s: impl Into<String>) -> Self {
        Self {
            content:  vec![ContentItem::text(s)],
            is_error: false,
        }
    }

    pub fn error(s: impl Into<String>) -> Self {
        Self {
            content:  vec![ContentItem::text(s)],
            is_error: true,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ContentItem {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub text: String,
}

impl ContentItem {
    pub fn text(s: impl Into<String>) -> Self {
        Self { kind: "text", text: s.into() }
    }
}
