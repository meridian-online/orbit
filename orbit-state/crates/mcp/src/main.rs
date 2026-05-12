//! orbit-mcp — Model Context Protocol server for orbit-state.
//!
//! Hand-rolled JSON-RPC 2.0 stdio loop. Real MCP-SDK integration is a
//! follow-up — for ac-05 we need the parity contract, not full wire
//! compliance. Methods supported:
//!
//! - `initialize`     → returns an empty capabilities object
//! - `tools/list`     → returns the verb surface
//! - `tools/call`     → translates `{name, arguments}` to a [`VerbRequest`],
//!                      calls [`orbit_state_core::execute`], and wraps the
//!                      [envelope][orbit_state_core::envelope_ok] in MCP's
//!                      `content[].text` shape
//!
//! Architectural contract with the CLI: both surfaces construct a
//! `VerbRequest`, dispatch through the same `execute`, and emit the same
//! envelope helpers. The envelope text inside `tools/call`'s
//! `result.content[0].text` is byte-identical to the CLI's `--json` stdout.
//! That's how the parity test (`tests/parity.rs` in this crate) verifies
//! ac-05.
//!
//! Wire transport: newline-delimited JSON. One request per line, one
//! response per line. Unparseable lines produce a JSON-RPC parse-error
//! response with `id: null`.

use orbit_state_core::layout::OrbitLayout;
use orbit_state_core::{envelope_err, envelope_ok, execute, VerbRequest};
use serde_json::{json, Value};
use std::io::{BufRead, Write};

fn main() -> anyhow::Result<()> {
    // ac-21 link preservation — same rationale as the CLI.
    orbit_state_core::link_sanity_check()?;

    let root = std::env::current_dir()?;
    let layout = OrbitLayout::at(&root);

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let response = handle_line(&layout, &line);
        // Skip notifications (no id). For requests we always emit a response.
        if let Some(resp) = response {
            writeln!(out, "{resp}")?;
            out.flush()?;
        }
    }
    Ok(())
}

/// Handle a single line of input. Returns `None` for notifications (which
/// JSON-RPC says have no response), `Some(value)` for requests.
fn handle_line(layout: &OrbitLayout, line: &str) -> Option<Value> {
    let req: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            return Some(json!({
                "jsonrpc": "2.0",
                "id": Value::Null,
                "error": { "code": -32700, "message": format!("parse error: {e}") }
            }));
        }
    };

    let id = req.get("id").cloned();
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(Value::Null);

    // Notifications (no id) get no response per JSON-RPC 2.0.
    let id = match id {
        Some(v) if !v.is_null() => v,
        _ => return None,
    };

    Some(dispatch(layout, &id, method, &params))
}

fn dispatch(layout: &OrbitLayout, id: &Value, method: &str, params: &Value) -> Value {
    match method {
        "initialize" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": {
                    "name": "orbit-mcp",
                    "version": env!("CARGO_PKG_VERSION"),
                }
            }
        }),
        "tools/list" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "tools": tool_descriptors() }
        }),
        "tools/call" => handle_tool_call(layout, id, params),
        other => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32601, "message": format!("method not found: {other}") }
        }),
    }
}

/// MCP tool descriptors. Mirrors the [`VerbRequest`] surface — adding a verb
/// means adding a descriptor here.
fn tool_descriptors() -> Vec<Value> {
    vec![
        json!({
            "name": "spec.list",
            "description": "List specs in the .orbit/ folder, sorted by id.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "status": {
                        "type": "string",
                        "enum": ["open", "closed"],
                        "description": "Filter by status."
                    }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "spec.show",
            "description": "Read a single spec by id and return its full contents.",
            "inputSchema": {
                "type": "object",
                "required": ["id"],
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Spec id (slug-shaped; no path separators)."
                    }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "spec.note",
            "description": "Append a timestamped note to a spec's notes JSONL stream.",
            "inputSchema": {
                "type": "object",
                "required": ["id", "body"],
                "properties": {
                    "id":   { "type": "string", "description": "Spec id." },
                    "body": { "type": "string", "description": "Note body (free text)." },
                    "labels": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional free-text labels."
                    },
                    "timestamp": {
                        "type": "string",
                        "description": "Override substrate timestamp (RFC 3339). Primarily for migration tools."
                    }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "spec.create",
            "description": "Create a new spec at .orbit/specs/<id>.yaml.",
            "inputSchema": {
                "type": "object",
                "required": ["id", "goal"],
                "properties": {
                    "id":   { "type": "string" },
                    "goal": { "type": "string" },
                    "cards": { "type": "array", "items": { "type": "string" } },
                    "labels": { "type": "array", "items": { "type": "string" } },
                    "acceptance_criteria": { "type": "array" }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "spec.update",
            "description": "Update fields on an existing spec. Status changes go via spec.close.",
            "inputSchema": {
                "type": "object",
                "required": ["id"],
                "properties": {
                    "id":   { "type": "string" },
                    "goal": { "type": "string" },
                    "cards": { "type": "array", "items": { "type": "string" } },
                    "labels": { "type": "array", "items": { "type": "string" } },
                    "acceptance_criteria": { "type": "array" }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "spec.close",
            "description": "Close a spec; transactionally appends to linked cards' specs arrays.",
            "inputSchema": {
                "type": "object",
                "required": ["id"],
                "properties": {
                    "id": { "type": "string" }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "task.open",
            "description": "Open a new task under a spec.",
            "inputSchema": {
                "type": "object",
                "required": ["spec_id", "body"],
                "properties": {
                    "spec_id": { "type": "string" },
                    "body":    { "type": "string" },
                    "labels":  { "type": "array", "items": { "type": "string" } },
                    "task_id": { "type": "string" },
                    "timestamp": { "type": "string" }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "task.list",
            "description": "List tasks (current state per task_id).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "spec_id": { "type": "string" },
                    "state": { "type": "string", "enum": ["open", "claim", "update", "done"] }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "task.show",
            "description": "Show one task with its full event history.",
            "inputSchema": {
                "type": "object",
                "required": ["spec_id", "task_id"],
                "properties": {
                    "spec_id": { "type": "string" },
                    "task_id": { "type": "string" }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "task.ready",
            "description": "List claimable (open, no claim) tasks.",
            "inputSchema": {
                "type": "object",
                "properties": { "spec_id": { "type": "string" } },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "task.claim",
            "description": "Claim an open task.",
            "inputSchema": {
                "type": "object",
                "required": ["spec_id", "task_id"],
                "properties": {
                    "spec_id": { "type": "string" },
                    "task_id": { "type": "string" },
                    "body": { "type": "string" },
                    "labels": { "type": "array", "items": { "type": "string" } },
                    "timestamp": { "type": "string" }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "task.update",
            "description": "Append an update note to a task.",
            "inputSchema": {
                "type": "object",
                "required": ["spec_id", "task_id", "body"],
                "properties": {
                    "spec_id": { "type": "string" },
                    "task_id": { "type": "string" },
                    "body": { "type": "string" },
                    "labels": { "type": "array", "items": { "type": "string" } },
                    "timestamp": { "type": "string" }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "task.done",
            "description": "Mark a task done.",
            "inputSchema": {
                "type": "object",
                "required": ["spec_id", "task_id"],
                "properties": {
                    "spec_id": { "type": "string" },
                    "task_id": { "type": "string" },
                    "body": { "type": "string" },
                    "labels": { "type": "array", "items": { "type": "string" } },
                    "timestamp": { "type": "string" }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "memory.remember",
            "description": "Upsert a memory entry. Persists across sessions/machines via git.",
            "inputSchema": {
                "type": "object",
                "required": ["key", "body"],
                "properties": {
                    "key":    { "type": "string" },
                    "body":   { "type": "string" },
                    "labels": { "type": "array", "items": { "type": "string" } },
                    "timestamp": { "type": "string" }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "memory.list",
            "description": "List all memories.",
            "inputSchema": { "type": "object", "additionalProperties": false }
        }),
        json!({
            "name": "memory.search",
            "description": "Substring (case-insensitive) search over body + labels.",
            "inputSchema": {
                "type": "object",
                "required": ["query"],
                "properties": { "query": { "type": "string" } },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "card.show",
            "description": "Show a card by slug.",
            "inputSchema": {
                "type": "object",
                "required": ["slug"],
                "properties": { "slug": { "type": "string" } },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "card.list",
            "description": "List cards. Optional filter by maturity.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "maturity": { "type": "string", "enum": ["planned", "emerging", "established"] }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "card.search",
            "description": "Substring (case-insensitive) search over slug + feature + goal.",
            "inputSchema": {
                "type": "object",
                "required": ["query"],
                "properties": { "query": { "type": "string" } },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "card.tree",
            "description": "Render the local subgraph from a card (outgoing + incoming `relations:` edges). Default depth is 2; cycle-safe.",
            "inputSchema": {
                "type": "object",
                "required": ["slug"],
                "properties": {
                    "slug": { "type": "string" },
                    "depth": { "type": "integer", "minimum": 0 }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "card.specs",
            "description": "List specs advancing a card, with bidirectional link health. Surfaces drift where card.specs[] and spec.cards[] disagree.",
            "inputSchema": {
                "type": "object",
                "required": ["slug"],
                "properties": { "slug": { "type": "string" } },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "choice.show",
            "description": "Show a choice by id.",
            "inputSchema": {
                "type": "object",
                "required": ["id"],
                "properties": { "id": { "type": "string" } },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "choice.list",
            "description": "List choices. Optional filter by status.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "status": {
                        "type": "string",
                        "enum": ["proposed", "accepted", "rejected", "deprecated", "superseded"]
                    }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "choice.search",
            "description": "Substring (case-insensitive) search over title + body.",
            "inputSchema": {
                "type": "object",
                "required": ["query"],
                "properties": { "query": { "type": "string" } },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "session.prime",
            "description": "Agent session priming context — bounded output (open specs + up to K memories).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "memory_cap": {
                        "type": "integer",
                        "minimum": 0,
                        "description": "Override the default K=10 memory cap."
                    }
                },
                "additionalProperties": false
            }
        }),
    ]
}

fn handle_tool_call(layout: &OrbitLayout, id: &Value, params: &Value) -> Value {
    let name = match params.get("name").and_then(Value::as_str) {
        Some(n) => n,
        None => {
            return json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32602, "message": "tools/call: missing 'name'" }
            });
        }
    };
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| Value::Object(Default::default()));

    // Reconstruct VerbRequest from MCP's {name, arguments} shape. The verbs
    // module's tagged-enum representation is `{"verb": <name>, "args": ...}`
    // — translate by reshaping into that.
    let request_value = json!({ "verb": name, "args": arguments });
    let request: VerbRequest = match serde_json::from_value(request_value) {
        Ok(r) => r,
        Err(e) => {
            // Invalid args surface as a tool-level error envelope inside a
            // successful JSON-RPC response — that's the MCP convention for
            // tool failures (clients should look at `isError`, not the
            // JSON-RPC error channel).
            let err = orbit_state_core::Error::malformed(
                name,
                format!("invalid arguments: {e}"),
            );
            return tool_error_response(id, &err);
        }
    };

    match execute(layout, &request) {
        Ok(response) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "content": [
                    { "type": "text", "text": envelope_ok(&response).to_string() }
                ]
            }
        }),
        Err(err) => tool_error_response(id, &err),
    }
}

fn tool_error_response(id: &Value, err: &orbit_state_core::Error) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "content": [
                { "type": "text", "text": envelope_err(err).to_string() }
            ],
            "isError": true,
        }
    })
}
