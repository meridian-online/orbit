//! ac-05 parity harness — MCP surface.
//!
//! Spawns the `orbit-mcp` binary, sends a JSON-RPC `tools/call` request for
//! `spec.list`, and asserts the inner envelope text inside
//! `result.content[0].text` equals the canonical envelope reference.
//!
//! See `crates/cli/tests/parity.rs` for the matching surface — when both
//! tests pass, both surfaces produce byte-identical envelopes for the same
//! input state, which is the parity contract from ac-05.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

mod common;

#[test]
fn spec_list_mcp_envelope_matches_canonical_envelope() {
    let dir = tempfile::tempdir().unwrap();
    common::populate_two_specs(dir.path());

    let inner = run_mcp_tools_call(
        dir.path(),
        json!({ "name": "spec.list", "arguments": {} }),
    );
    let envelope = inner_envelope_text(&inner);

    let expected = common::expected_envelope_for_two_specs();
    assert_eq!(envelope, expected, "MCP envelope diverged from canonical");
}

#[test]
fn spec_list_mcp_invalid_status_returns_error_envelope_with_is_error() {
    let dir = tempfile::tempdir().unwrap();
    common::populate_two_specs(dir.path());

    let inner = run_mcp_tools_call(
        dir.path(),
        json!({ "name": "spec.list", "arguments": { "status": "nope" } }),
    );
    let result = inner.get("result").expect("has result");
    assert_eq!(
        result.get("isError").and_then(Value::as_bool),
        Some(true),
        "expected isError=true: {result}"
    );
    let envelope = inner_envelope_text(&inner);
    assert_eq!(envelope, common::expected_envelope_for_invalid_status());
}

#[test]
fn tools_list_advertises_spec_verbs() {
    let dir = tempfile::tempdir().unwrap();
    let mcp_bin = env!("CARGO_BIN_EXE_orbit-mcp");

    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/list",
        "params": {}
    });
    let response = exchange_one(mcp_bin, dir.path(), &request);
    let tools = response
        .pointer("/result/tools")
        .and_then(Value::as_array)
        .expect("tools array present");
    let names: Vec<_> = tools
        .iter()
        .filter_map(|t| t.get("name").and_then(Value::as_str))
        .collect();
    assert!(names.contains(&"spec.list"), "spec.list missing: {names:?}");
    assert!(names.contains(&"spec.show"), "spec.show missing: {names:?}");
}

#[test]
fn spec_show_mcp_envelope_matches_canonical_envelope() {
    let dir = tempfile::tempdir().unwrap();
    common::populate_two_specs(dir.path());

    let inner = run_mcp_tools_call(
        dir.path(),
        json!({ "name": "spec.show", "arguments": { "id": "0001" } }),
    );
    let envelope = inner_envelope_text(&inner);
    assert_eq!(envelope, common::expected_envelope_for_spec_show_0001());
}

// ---------------------------------------------------------------------------
// State-mutation parity (ac-05 core gate) — spec.note
// ---------------------------------------------------------------------------

#[test]
fn spec_note_mcp_writes_byte_identical_jsonl_and_envelope() {
    let dir = tempfile::tempdir().unwrap();
    common::populate_two_specs(dir.path());

    let note = common::fixture_note();
    let inner = run_mcp_tools_call(
        dir.path(),
        json!({
            "name": "spec.note",
            "arguments": {
                "id": note.spec_id,
                "body": note.body,
                "labels": note.labels,
                "timestamp": note.timestamp,
            }
        }),
    );

    // Envelope parity: MCP content[].text matches the canonical envelope.
    let envelope = inner_envelope_text(&inner);
    assert_eq!(envelope, common::expected_envelope_for_fixture_note());

    // State parity: same on-disk bytes as the CLI surface produces, by
    // transitivity (both surfaces compared to the same library reference).
    let stream_path = dir.path().join(".orbit/specs/0001/notes.jsonl");
    let actual = std::fs::read_to_string(&stream_path).unwrap();
    assert_eq!(actual, common::expected_notes_jsonl_for_fixture_note());
}

#[test]
fn spec_show_mcp_missing_id_returns_error_envelope() {
    let dir = tempfile::tempdir().unwrap();
    common::populate_two_specs(dir.path());

    let inner = run_mcp_tools_call(
        dir.path(),
        json!({ "name": "spec.show", "arguments": { "id": "0099" } }),
    );
    let result = inner.get("result").expect("has result");
    assert_eq!(result.get("isError").and_then(Value::as_bool), Some(true));
    let envelope = inner_envelope_text(&inner);
    assert_eq!(
        envelope,
        common::expected_envelope_for_spec_show_missing(dir.path())
    );
}

// ---------------------------------------------------------------------------
// Test plumbing
// ---------------------------------------------------------------------------

/// Send a single `tools/call` to the MCP server and return the parsed JSON-RPC
/// response.
fn run_mcp_tools_call(root: &std::path::Path, params: Value) -> Value {
    let mcp_bin = env!("CARGO_BIN_EXE_orbit-mcp");
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": params,
    });
    exchange_one(mcp_bin, root, &request)
}

/// Spawn the MCP, write one JSON-RPC line, read one JSON-RPC line back, exit.
fn exchange_one(bin: &str, root: &std::path::Path, request: &Value) -> Value {
    let mut child = Command::new(bin)
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn orbit-mcp");

    let stdin = child.stdin.as_mut().expect("stdin");
    writeln!(stdin, "{request}").expect("write request");
    stdin.flush().expect("flush");
    // Closing stdin signals EOF so the server's read loop terminates after
    // emitting the response — keeps the test deterministic.
    drop(child.stdin.take());

    let stdout = child.stdout.take().expect("stdout");
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).expect("read response");

    let _ = child.wait();

    serde_json::from_str(line.trim()).unwrap_or_else(|e| {
        panic!("MCP response is not valid JSON: {e}\nline: {line}");
    })
}

#[test]
fn card_tree_mcp_envelope_matches_canonical_envelope() {
    let dir = tempfile::tempdir().unwrap();
    common::populate_two_related_cards(dir.path());

    let inner = run_mcp_tools_call(
        dir.path(),
        json!({ "name": "card.tree", "arguments": { "slug": "0001-alpha", "depth": 1 } }),
    );
    let envelope = inner_envelope_text(&inner);

    let expected = common::expected_envelope_for_card_tree_alpha_depth1();
    assert_eq!(envelope, expected, "MCP envelope diverged from canonical");
}

#[test]
fn card_specs_mcp_unknown_id_returns_error_envelope_with_is_error() {
    let dir = tempfile::tempdir().unwrap();
    common::populate_two_related_cards(dir.path());
    let cards_dir = dir.path().join(".orbit/cards");

    let inner = run_mcp_tools_call(
        dir.path(),
        json!({ "name": "card.specs", "arguments": { "slug": "9999" } }),
    );
    let result = inner.get("result").expect("has result");
    assert_eq!(result.get("isError").and_then(Value::as_bool), Some(true));
    let envelope = inner_envelope_text(&inner);
    assert_eq!(envelope, common::expected_envelope_for_card_specs_unknown(&cards_dir));
}

#[test]
fn graph_mcp_unknown_card_returns_error_envelope_with_is_error() {
    let dir = tempfile::tempdir().unwrap();
    common::populate_two_related_cards(dir.path());
    let cards_dir = dir.path().join(".orbit/cards");

    let inner = run_mcp_tools_call(
        dir.path(),
        json!({ "name": "graph", "arguments": { "card": "9999" } }),
    );
    let result = inner.get("result").expect("has result");
    assert_eq!(result.get("isError").and_then(Value::as_bool), Some(true));
    let envelope = inner_envelope_text(&inner);
    assert_eq!(envelope, common::expected_envelope_for_graph_unknown(&cards_dir));
}

#[test]
fn card_tree_mcp_unknown_id_returns_error_envelope_with_is_error() {
    let dir = tempfile::tempdir().unwrap();
    common::populate_two_related_cards(dir.path());
    let cards_dir = dir.path().join(".orbit/cards");

    let inner = run_mcp_tools_call(
        dir.path(),
        json!({ "name": "card.tree", "arguments": { "slug": "9999" } }),
    );
    let result = inner.get("result").expect("has result");
    assert_eq!(
        result.get("isError").and_then(Value::as_bool),
        Some(true),
        "expected isError=true: {result}"
    );
    let envelope = inner_envelope_text(&inner);
    assert_eq!(envelope, common::expected_envelope_for_card_tree_unknown(&cards_dir));
}

#[test]
fn audit_drift_mcp_envelope_matches_canonical_envelope() {
    let dir = tempfile::tempdir().unwrap();
    common::populate_card_with_drift(dir.path());

    let inner = run_mcp_tools_call(
        dir.path(),
        json!({ "name": "audit.drift", "arguments": {} }),
    );
    let envelope = inner_envelope_text(&inner);

    let expected = common::expected_envelope_for_audit_drift_one_unknown();
    assert_eq!(envelope, expected, "MCP envelope diverged from canonical");
}

#[test]
fn graph_mcp_mermaid_envelope_matches_canonical_envelope() {
    let dir = tempfile::tempdir().unwrap();
    common::populate_two_related_cards(dir.path());

    let inner = run_mcp_tools_call(
        dir.path(),
        json!({ "name": "graph", "arguments": {} }),
    );
    let envelope = inner_envelope_text(&inner);

    let expected = common::expected_envelope_for_graph_mermaid_two_related_cards();
    assert_eq!(envelope, expected, "MCP envelope diverged from canonical");
}

#[test]
fn overview_mcp_envelope_matches_canonical_envelope() {
    let dir = tempfile::tempdir().unwrap();
    common::populate_two_related_cards(dir.path());

    let inner = run_mcp_tools_call(
        dir.path(),
        json!({ "name": "overview", "arguments": {} }),
    );
    let envelope = inner_envelope_text(&inner);

    let expected = common::expected_envelope_for_overview_two_related_cards();
    assert_eq!(envelope, expected, "MCP envelope diverged from canonical");
}

#[test]
fn card_specs_mcp_envelope_matches_canonical_envelope() {
    let dir = tempfile::tempdir().unwrap();
    common::populate_card_with_linked_spec(dir.path());

    let inner = run_mcp_tools_call(
        dir.path(),
        json!({ "name": "card.specs", "arguments": { "slug": "0001-alpha" } }),
    );
    let envelope = inner_envelope_text(&inner);

    let expected = common::expected_envelope_for_card_specs_alpha();
    assert_eq!(envelope, expected, "MCP envelope diverged from canonical");
}

// ---------------------------------------------------------------------------
// spec.close AC pre-flight (spec 2026-05-13-spec-close-ac-preflight, ac-05)
// ---------------------------------------------------------------------------

#[test]
fn spec_close_mcp_unchecked_acs_emits_conflict_envelope() {
    // ac-05 / ac-02: MCP `spec.close` against a spec with one unchecked
    // non-time-gated AC emits the canonical conflict envelope.
    let dir = tempfile::tempdir().unwrap();
    common::populate_spec_close_preflight_fixture(dir.path());

    let inner = run_mcp_tools_call(
        dir.path(),
        json!({ "name": "spec.close", "arguments": { "id": "0001" } }),
    );
    let result = inner.get("result").expect("has result");
    assert_eq!(result.get("isError").and_then(Value::as_bool), Some(true));

    let envelope = inner_envelope_text(&inner);
    assert_eq!(envelope, common::expected_envelope_for_spec_close_unchecked_blocking());

    // State parity: spec is still open, card is unmutated.
    let spec_text = std::fs::read_to_string(dir.path().join(".orbit/specs/0001/spec.yaml")).unwrap();
    assert!(spec_text.contains("status: open"), "spec mutated: {spec_text}");
    let card_text = std::fs::read_to_string(dir.path().join(".orbit/cards/0020-orbit-state.yaml")).unwrap();
    assert!(!card_text.contains("specs:"), "card specs array touched: {card_text}");
}

#[test]
fn spec_close_mcp_force_proceeds_with_envelope() {
    // ac-05 / ac-03: MCP `spec.close { force: true }` bypasses the
    // unchecked-AC guard and emits the canonical ok envelope with
    // `forced_unchecked` and `time_gated_open` populated.
    let dir = tempfile::tempdir().unwrap();
    common::populate_spec_close_preflight_fixture(dir.path());

    let inner = run_mcp_tools_call(
        dir.path(),
        json!({ "name": "spec.close", "arguments": { "id": "0001", "force": true } }),
    );
    let envelope = inner_envelope_text(&inner);
    assert_eq!(envelope, common::expected_envelope_for_spec_close_force());

    // State parity.
    let spec_text = std::fs::read_to_string(dir.path().join(".orbit/specs/0001/spec.yaml")).unwrap();
    assert!(spec_text.contains("status: closed"), "spec not closed: {spec_text}");
    let card_text = std::fs::read_to_string(dir.path().join(".orbit/cards/0020-orbit-state.yaml")).unwrap();
    assert!(
        card_text.contains(".orbit/specs/0001/spec.yaml"),
        "card not updated: {card_text}"
    );
}

#[test]
fn spec_close_mcp_time_gated_only_proceeds_without_force() {
    // ac-05 / ac-04: MCP `spec.close` against a spec whose sole unchecked
    // AC is time-gated succeeds without `force`; envelope carries
    // `time_gated_open` and empty `forced_unchecked`.
    let dir = tempfile::tempdir().unwrap();
    common::populate_spec_close_only_time_gated_fixture(dir.path());

    let inner = run_mcp_tools_call(
        dir.path(),
        json!({ "name": "spec.close", "arguments": { "id": "0001" } }),
    );
    let envelope = inner_envelope_text(&inner);
    assert_eq!(envelope, common::expected_envelope_for_spec_close_only_time_gated());

    let spec_text = std::fs::read_to_string(dir.path().join(".orbit/specs/0001/spec.yaml")).unwrap();
    assert!(spec_text.contains("status: closed"), "spec not closed: {spec_text}");
}

/// Extract `result.content[0].text` from a JSON-RPC response — that's where
/// the wire envelope lives in MCP's `tools/call` shape.
fn inner_envelope_text(response: &Value) -> String {
    response
        .pointer("/result/content/0/text")
        .and_then(Value::as_str)
        .map(String::from)
        .unwrap_or_else(|| panic!("missing /result/content/0/text in response: {response}"))
}

// ---------------------------------------------------------------------------
// Spec 2026-05-15-agent-learning-loop parity tests
// ---------------------------------------------------------------------------

#[test]
fn session_start_mcp_envelope_matches_canonical() {
    let dir = tempfile::tempdir().unwrap();
    let inner = run_mcp_tools_call(
        dir.path(),
        json!({ "name": "session.start", "arguments": { "id": common::PARITY_SESSION_ID } }),
    );
    let envelope = inner_envelope_text(&inner);
    assert_eq!(envelope, common::expected_envelope_for_session_start(dir.path()));

    let on_disk = std::fs::read_to_string(dir.path().join(".orbit/.session-id")).unwrap();
    assert_eq!(on_disk.trim(), common::PARITY_SESSION_ID);
}

#[test]
fn skill_record_invocation_mcp_envelope_matches_canonical() {
    let dir = tempfile::tempdir().unwrap();
    let inner = run_mcp_tools_call(
        dir.path(),
        json!({
            "name": "skill.record-invocation",
            "arguments": {
                "skill_id": "card",
                "outcome": "worked",
                "session_id": common::PARITY_SESSION_ID,
                "timestamp": common::PARITY_TIMESTAMP,
            }
        }),
    );
    let envelope = inner_envelope_text(&inner);
    assert_eq!(envelope, common::expected_envelope_for_skill_record_invocation());

    let path = dir.path().join(".orbit/skills/card.invocations.jsonl");
    let body = std::fs::read_to_string(&path).unwrap();
    assert_eq!(body.lines().count(), 1);
}

#[test]
fn skill_recurrence_mcp_envelope_empty_matches_canonical() {
    let dir = tempfile::tempdir().unwrap();
    let inner = run_mcp_tools_call(
        dir.path(),
        json!({ "name": "skill.recurrence", "arguments": { "skill_id": "design" } }),
    );
    let envelope = inner_envelope_text(&inner);
    assert_eq!(envelope, common::expected_envelope_for_skill_recurrence_empty());
}

#[test]
fn session_distill_mcp_envelope_matches_canonical() {
    use orbit_state_core::schema::Session;
    let dir = tempfile::tempdir().unwrap();
    let inner = run_mcp_tools_call(
        dir.path(),
        json!({
            "name": "session.distill",
            "arguments": {
                "session_id": common::PARITY_SESSION_ID,
                "distillate": "parity-distillate",
            }
        }),
    );
    let envelope = inner_envelope_text(&inner);

    let session_path = dir
        .path()
        .join(".orbit/sessions")
        .join(format!("{}.yaml", common::PARITY_SESSION_ID));
    let text = std::fs::read_to_string(&session_path).unwrap();
    let session: Session = serde_yaml::from_str(&text).unwrap();
    let expected = common::expected_envelope_for_session_distill(
        "parity-distillate",
        &session.started_at,
        session.ended_at.as_deref().unwrap_or(""),
    );
    assert_eq!(envelope, expected);
}
