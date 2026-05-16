//! ac-05 parity harness — CLI surface.
//!
//! Strategy: the CLI's `--json` stdout MUST equal the canonical envelope
//! produced by [`orbit_state_core::envelope_ok`] over the same response.
//! The MCP test (`crates/mcp/tests/parity.rs`) checks the same expected
//! envelope from its surface — when both pass, the two surfaces agree.
//!
//! Cross-binary comparison is unnecessary: both surfaces match the same
//! reference, so by transitivity they match each other. This sidesteps the
//! `CARGO_BIN_EXE_*` cross-crate visibility problem.

use std::path::Path;
use std::process::{Command, Stdio};

mod common;

#[test]
fn spec_list_cli_json_matches_canonical_envelope() {
    let dir = tempfile::tempdir().unwrap();
    common::populate_two_specs(dir.path());

    let cli_bin = env!("CARGO_BIN_EXE_orbit");
    let output = Command::new(cli_bin)
        .args(["--root", dir.path().to_str().unwrap(), "--json", "spec", "list"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("run orbit cli");

    assert!(
        output.status.success(),
        "CLI exited non-zero: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    let actual = stdout.trim_end_matches('\n');

    let expected = common::expected_envelope_for_two_specs();
    assert_eq!(
        actual, expected,
        "CLI envelope diverged from canonical envelope"
    );
}

#[test]
fn spec_list_cli_default_output_is_human_readable() {
    let dir = tempfile::tempdir().unwrap();
    common::populate_two_specs(dir.path());

    let cli_bin = env!("CARGO_BIN_EXE_orbit");
    let output = Command::new(cli_bin)
        .args(["--root", dir.path().to_str().unwrap(), "spec", "list"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("run orbit cli");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    // Two specs, tab-separated, sorted by id.
    assert!(stdout.contains("0001\topen\tfirst spec"), "got: {stdout}");
    assert!(stdout.contains("0002\tclosed\tsecond spec"), "got: {stdout}");
    let pos1 = stdout.find("0001").unwrap();
    let pos2 = stdout.find("0002").unwrap();
    assert!(pos1 < pos2, "specs not sorted by id: {stdout}");
}

#[test]
fn spec_list_cli_empty_dir_emits_ok_envelope() {
    let dir = tempfile::tempdir().unwrap();
    // Don't populate — directory has no .orbit/ at all. spec_list returns Ok([]).

    let cli_bin = env!("CARGO_BIN_EXE_orbit");
    let output = Command::new(cli_bin)
        .args(["--root", dir.path().to_str().unwrap(), "--json", "spec", "list"])
        .stdin(Stdio::null())
        .output()
        .expect("run orbit cli");

    assert!(output.status.success(), "CLI exited non-zero on empty dir");
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    let actual = stdout.trim_end_matches('\n');
    assert_eq!(actual, common::expected_envelope_for_empty());
}

#[test]
fn spec_list_cli_invalid_status_emits_err_envelope() {
    let dir = tempfile::tempdir().unwrap();
    common::populate_two_specs(dir.path());

    let cli_bin = env!("CARGO_BIN_EXE_orbit");
    let output = Command::new(cli_bin)
        .args([
            "--root", dir.path().to_str().unwrap(),
            "--json", "spec", "list", "--status", "nope",
        ])
        .stdin(Stdio::null())
        .output()
        .expect("run orbit cli");

    assert!(!output.status.success(), "CLI must exit non-zero on err");
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    let actual = stdout.trim_end_matches('\n');
    assert_eq!(actual, common::expected_envelope_for_invalid_status());
}

#[test]
fn spec_show_cli_json_matches_canonical_envelope() {
    let dir = tempfile::tempdir().unwrap();
    common::populate_two_specs(dir.path());

    let cli_bin = env!("CARGO_BIN_EXE_orbit");
    let output = Command::new(cli_bin)
        .args(["--root", dir.path().to_str().unwrap(), "--json", "spec", "show", "0001"])
        .stdin(Stdio::null())
        .output()
        .expect("run cli");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf-8");
    let actual = stdout.trim_end_matches('\n');
    assert_eq!(actual, common::expected_envelope_for_spec_show_0001());
}

#[test]
fn spec_show_cli_missing_id_emits_not_found() {
    let dir = tempfile::tempdir().unwrap();
    common::populate_two_specs(dir.path());

    let cli_bin = env!("CARGO_BIN_EXE_orbit");
    let output = Command::new(cli_bin)
        .args(["--root", dir.path().to_str().unwrap(), "--json", "spec", "show", "0099"])
        .stdin(Stdio::null())
        .output()
        .expect("run cli");

    assert!(!output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf-8");
    let actual = stdout.trim_end_matches('\n');
    assert_eq!(
        actual,
        common::expected_envelope_for_spec_show_missing(dir.path())
    );
}

// ---------------------------------------------------------------------------
// State-mutation parity (ac-05 core gate) — spec.note
// ---------------------------------------------------------------------------

#[test]
fn spec_note_cli_writes_byte_identical_jsonl_and_envelope() {
    let dir = tempfile::tempdir().unwrap();
    common::populate_two_specs(dir.path());

    let note = common::fixture_note();
    let cli_bin = env!("CARGO_BIN_EXE_orbit");
    let output = Command::new(cli_bin)
        .args([
            "--root", dir.path().to_str().unwrap(),
            "--json",
            "spec", "note",
            &note.spec_id,
            &note.body,
            "--label", &note.labels[0],
            "--timestamp", &note.timestamp,
        ])
        .stdin(Stdio::null())
        .output()
        .expect("run cli");
    assert!(
        output.status.success(),
        "spec.note failed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Envelope parity: CLI stdout matches the canonical envelope.
    let stdout = String::from_utf8(output.stdout).unwrap();
    let envelope = stdout.trim_end_matches('\n');
    assert_eq!(envelope, common::expected_envelope_for_fixture_note());

    // State parity: the JSONL stream on disk matches what the canonical
    // serialiser would produce. This is the "byte-identical state" half of
    // ac-05's parity contract — both surfaces, given the same input, produce
    // the same on-disk bytes.
    let stream_path = dir.path().join(".orbit/specs/0001/notes.jsonl");
    let actual = std::fs::read_to_string(&stream_path).unwrap();
    assert_eq!(actual, common::expected_notes_jsonl_for_fixture_note());
}

#[test]
fn spec_note_cli_appends_in_order_for_two_calls() {
    let dir = tempfile::tempdir().unwrap();
    common::populate_two_specs(dir.path());

    let cli_bin = env!("CARGO_BIN_EXE_orbit");
    for (i, body) in ["first", "second"].iter().enumerate() {
        let ts = format!("2026-05-07T12:00:0{i}Z");
        let status = Command::new(cli_bin)
            .args([
                "--root", dir.path().to_str().unwrap(),
                "spec", "note", "0001", body, "--timestamp", &ts,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("run cli");
        assert!(status.success());
    }

    let stream = std::fs::read_to_string(dir.path().join(".orbit/specs/0001/notes.jsonl")).unwrap();
    let lines: Vec<_> = stream.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains(r#""body":"first""#));
    assert!(lines[1].contains(r#""body":"second""#));
}

// ---------------------------------------------------------------------------
// End-to-end lifecycle — create → note → update → close
// ---------------------------------------------------------------------------

#[test]
fn cli_full_spec_lifecycle() {
    let dir = tempfile::tempdir().unwrap();

    // Pre-stage a card so spec.close has something to update.
    let cards_dir = dir.path().join(".orbit/cards");
    std::fs::create_dir_all(&cards_dir).unwrap();
    std::fs::write(
        cards_dir.join("0020-orbit-state.yaml"),
        "feature: orbit-state\ngoal: substrate\nmaturity: planned\n",
    )
    .unwrap();

    let cli_bin = env!("CARGO_BIN_EXE_orbit");
    let root = dir.path().to_str().unwrap();

    // 1. Create
    let out = Command::new(cli_bin)
        .args([
            "--root", root, "spec", "create", "0001", "the goal",
            "--card", "0020-orbit-state",
        ])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success(), "create failed: {}", String::from_utf8_lossy(&out.stderr));

    // 2. Note
    let out = Command::new(cli_bin)
        .args([
            "--root", root, "spec", "note", "0001", "kicked off",
            "--timestamp", "2026-05-07T12:00:00Z",
        ])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success(), "note failed: {}", String::from_utf8_lossy(&out.stderr));

    // 3. Update goal
    let out = Command::new(cli_bin)
        .args([
            "--root", root, "spec", "update", "0001",
            "--goal", "the revised goal",
        ])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success(), "update failed: {}", String::from_utf8_lossy(&out.stderr));

    // 4. Close — triggers transactional card update
    let out = Command::new(cli_bin)
        .args(["--root", root, "spec", "close", "0001"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success(), "close failed: {}", String::from_utf8_lossy(&out.stderr));

    // 5. Verify final state
    //    spec is closed with revised goal
    let spec_text = std::fs::read_to_string(dir.path().join(".orbit/specs/0001/spec.yaml")).unwrap();
    assert!(spec_text.contains("status: closed"), "spec not closed: {spec_text}");
    assert!(spec_text.contains("the revised goal"), "goal not updated: {spec_text}");

    //    note stream has one entry
    let notes = std::fs::read_to_string(dir.path().join(".orbit/specs/0001/notes.jsonl")).unwrap();
    assert_eq!(notes.lines().count(), 1);
    assert!(notes.contains(r#""body":"kicked off""#));

    //    linked card's specs array now contains the spec ref
    let card_text = std::fs::read_to_string(cards_dir.join("0020-orbit-state.yaml")).unwrap();
    assert!(
        card_text.contains(".orbit/specs/0001/spec.yaml"),
        "card not updated: {card_text}"
    );
}

// ---------------------------------------------------------------------------
// AC-check flag — `spec update --ac-check / --ac-uncheck` round-trip
// ---------------------------------------------------------------------------

#[test]
fn cli_spec_update_ac_check_flips_named_ac() {
    let dir = tempfile::tempdir().unwrap();
    let spec_dir = dir.path().join(".orbit/specs/test");
    std::fs::create_dir_all(&spec_dir).unwrap();
    std::fs::write(
        spec_dir.join("spec.yaml"),
        "id: test\n\
         goal: smoke\n\
         cards: []\n\
         status: open\n\
         labels: []\n\
         acceptance_criteria:\n\
         - id: ac-01\n  description: First\n  gate: true\n  checked: false\n\
         - id: ac-02\n  description: Second\n  gate: false\n  checked: false\n",
    )
    .unwrap();

    let cli = env!("CARGO_BIN_EXE_orbit");
    let root = dir.path().to_str().unwrap();

    // Check ac-01.
    let out = Command::new(cli)
        .args(["--root", root, "spec", "update", "test", "--ac-check", "ac-01"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "ac-check failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let yaml = std::fs::read_to_string(spec_dir.join("spec.yaml")).unwrap();
    assert!(yaml.contains("- id: ac-01\n  description: First\n  gate: true\n  checked: true\n"));
    assert!(yaml.contains("- id: ac-02\n  description: Second\n  gate: false\n  checked: false\n"));

    // Re-checking emits a conflict envelope.
    let out = Command::new(cli)
        .args(["--root", root, "--json", "spec", "update", "test", "--ac-check", "ac-01"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains(r#""category":"conflict""#), "got: {stdout}");
    assert!(stdout.contains("ac-01 is already checked"), "got: {stdout}");

    // Uncheck flips it back.
    let out = Command::new(cli)
        .args(["--root", root, "spec", "update", "test", "--ac-uncheck", "ac-01"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success());

    let yaml = std::fs::read_to_string(spec_dir.join("spec.yaml")).unwrap();
    assert!(yaml.contains("- id: ac-01\n  description: First\n  gate: true\n  checked: false\n"));
}

#[test]
fn cli_spec_update_ac_check_missing_ac_emits_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let spec_dir = dir.path().join(".orbit/specs/test");
    std::fs::create_dir_all(&spec_dir).unwrap();
    std::fs::write(
        spec_dir.join("spec.yaml"),
        "id: test\n\
         goal: smoke\n\
         cards: []\n\
         status: open\n\
         labels: []\n\
         acceptance_criteria:\n\
         - id: ac-01\n  description: First\n  gate: false\n  checked: false\n",
    )
    .unwrap();

    let cli = env!("CARGO_BIN_EXE_orbit");
    let root = dir.path().to_str().unwrap();

    let out = Command::new(cli)
        .args(["--root", root, "--json", "spec", "update", "test", "--ac-check", "ac-99"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains(r#""category":"not-found""#), "got: {stdout}");
}

#[test]
fn cli_spec_update_both_ac_flags_is_malformed() {
    let dir = tempfile::tempdir().unwrap();
    let spec_dir = dir.path().join(".orbit/specs/test");
    std::fs::create_dir_all(&spec_dir).unwrap();
    std::fs::write(
        spec_dir.join("spec.yaml"),
        "id: test\n\
         goal: smoke\n\
         cards: []\n\
         status: open\n\
         labels: []\n\
         acceptance_criteria:\n\
         - id: ac-01\n  description: First\n  gate: false\n  checked: false\n",
    )
    .unwrap();

    let cli = env!("CARGO_BIN_EXE_orbit");
    let root = dir.path().to_str().unwrap();

    let out = Command::new(cli)
        .args([
            "--root", root, "--json",
            "spec", "update", "test",
            "--ac-check", "ac-01",
            "--ac-uncheck", "ac-01",
        ])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains(r#""category":"malformed""#), "got: {stdout}");
    assert!(stdout.contains("mutually exclusive"), "got: {stdout}");
}

#[test]
fn card_tree_cli_json_matches_canonical_envelope() {
    let dir = tempfile::tempdir().unwrap();
    common::populate_two_related_cards(dir.path());

    let cli_bin = env!("CARGO_BIN_EXE_orbit");
    let output = Command::new(cli_bin)
        .args([
            "--root", dir.path().to_str().unwrap(),
            "--json", "card", "tree", "0001-alpha", "--depth", "1",
        ])
        .stdin(Stdio::null())
        .output()
        .expect("run orbit cli");

    assert!(
        output.status.success(),
        "CLI exited non-zero: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    let actual = stdout.trim_end_matches('\n');
    let expected = common::expected_envelope_for_card_tree_alpha_depth1();
    assert_eq!(actual, expected, "CLI envelope diverged from canonical");
}

#[test]
fn card_specs_cli_unknown_id_emits_canonical_err_envelope() {
    let dir = tempfile::tempdir().unwrap();
    common::populate_two_related_cards(dir.path());
    let cards_dir = dir.path().join(".orbit/cards");

    let cli_bin = env!("CARGO_BIN_EXE_orbit");
    let output = Command::new(cli_bin)
        .args([
            "--root", dir.path().to_str().unwrap(),
            "--json", "card", "specs", "9999",
        ])
        .stdin(Stdio::null())
        .output()
        .expect("run orbit cli");

    assert!(!output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    let actual = stdout.trim_end_matches('\n');
    let expected = common::expected_envelope_for_card_specs_unknown(&cards_dir);
    assert_eq!(actual, expected);
}

#[test]
fn graph_cli_unknown_card_emits_canonical_err_envelope() {
    let dir = tempfile::tempdir().unwrap();
    common::populate_two_related_cards(dir.path());
    let cards_dir = dir.path().join(".orbit/cards");

    let cli_bin = env!("CARGO_BIN_EXE_orbit");
    let output = Command::new(cli_bin)
        .args([
            "--root", dir.path().to_str().unwrap(),
            "--json", "graph", "--card", "9999",
        ])
        .stdin(Stdio::null())
        .output()
        .expect("run orbit cli");

    assert!(!output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    let actual = stdout.trim_end_matches('\n');
    let expected = common::expected_envelope_for_graph_unknown(&cards_dir);
    assert_eq!(actual, expected);
}

#[test]
fn card_tree_cli_unknown_id_emits_canonical_err_envelope() {
    let dir = tempfile::tempdir().unwrap();
    common::populate_two_related_cards(dir.path());
    let cards_dir = dir.path().join(".orbit/cards");

    let cli_bin = env!("CARGO_BIN_EXE_orbit");
    let output = Command::new(cli_bin)
        .args([
            "--root", dir.path().to_str().unwrap(),
            "--json", "card", "tree", "9999",
        ])
        .stdin(Stdio::null())
        .output()
        .expect("run orbit cli");

    assert!(!output.status.success(), "CLI should exit non-zero on unknown id");
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    let actual = stdout.trim_end_matches('\n');
    let expected = common::expected_envelope_for_card_tree_unknown(&cards_dir);
    assert_eq!(actual, expected, "error envelope diverged from canonical");
}

#[test]
fn audit_drift_cli_json_matches_canonical_envelope() {
    let dir = tempfile::tempdir().unwrap();
    common::populate_card_with_drift(dir.path());

    let cli_bin = env!("CARGO_BIN_EXE_orbit");
    let output = Command::new(cli_bin)
        .args([
            "--root", dir.path().to_str().unwrap(),
            "--json", "audit", "drift",
        ])
        .stdin(Stdio::null())
        .output()
        .expect("run orbit cli");

    assert!(
        output.status.success(),
        "CLI exited non-zero: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    let actual = stdout.trim_end_matches('\n');
    let expected = common::expected_envelope_for_audit_drift_one_unknown();
    assert_eq!(actual, expected, "CLI envelope diverged from canonical");
}

#[test]
fn graph_cli_mermaid_json_matches_canonical_envelope() {
    let dir = tempfile::tempdir().unwrap();
    common::populate_two_related_cards(dir.path());

    let cli_bin = env!("CARGO_BIN_EXE_orbit");
    let output = Command::new(cli_bin)
        .args([
            "--root", dir.path().to_str().unwrap(),
            "--json", "graph",
        ])
        .stdin(Stdio::null())
        .output()
        .expect("run orbit cli");

    assert!(
        output.status.success(),
        "CLI exited non-zero: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    let actual = stdout.trim_end_matches('\n');
    let expected = common::expected_envelope_for_graph_mermaid_two_related_cards();
    assert_eq!(actual, expected, "CLI envelope diverged from canonical");
}

#[test]
fn overview_cli_json_matches_canonical_envelope() {
    let dir = tempfile::tempdir().unwrap();
    common::populate_two_related_cards(dir.path());

    let cli_bin = env!("CARGO_BIN_EXE_orbit");
    let output = Command::new(cli_bin)
        .args([
            "--root", dir.path().to_str().unwrap(),
            "--json", "overview",
        ])
        .stdin(Stdio::null())
        .output()
        .expect("run orbit cli");

    assert!(
        output.status.success(),
        "CLI exited non-zero: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    let actual = stdout.trim_end_matches('\n');
    let expected = common::expected_envelope_for_overview_two_related_cards();
    assert_eq!(actual, expected, "CLI envelope diverged from canonical");
}

#[test]
fn card_specs_cli_json_matches_canonical_envelope() {
    let dir = tempfile::tempdir().unwrap();
    common::populate_card_with_linked_spec(dir.path());

    let cli_bin = env!("CARGO_BIN_EXE_orbit");
    let output = Command::new(cli_bin)
        .args([
            "--root", dir.path().to_str().unwrap(),
            "--json", "card", "specs", "0001-alpha",
        ])
        .stdin(Stdio::null())
        .output()
        .expect("run orbit cli");

    assert!(
        output.status.success(),
        "CLI exited non-zero: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    let actual = stdout.trim_end_matches('\n');
    let expected = common::expected_envelope_for_card_specs_alpha();
    assert_eq!(actual, expected, "CLI envelope diverged from canonical");
}

// ---------------------------------------------------------------------------
// spec.close AC pre-flight (spec 2026-05-13-spec-close-ac-preflight, ac-05)
// ---------------------------------------------------------------------------

#[test]
fn spec_close_cli_unchecked_acs_emits_conflict_envelope() {
    // ac-05 / ac-02: CLI `spec close` against a spec with one unchecked
    // non-time-gated AC emits the canonical conflict envelope; no
    // state mutation occurs.
    let dir = tempfile::tempdir().unwrap();
    common::populate_spec_close_preflight_fixture(dir.path());

    let cli_bin = env!("CARGO_BIN_EXE_orbit");
    let output = Command::new(cli_bin)
        .args(["--root", dir.path().to_str().unwrap(), "--json", "spec", "close", "0001"])
        .stdin(Stdio::null())
        .output()
        .expect("run cli");

    assert!(!output.status.success(), "expected non-zero exit on conflict");
    let stdout = String::from_utf8(output.stdout).expect("utf-8");
    let actual = stdout.trim_end_matches('\n');
    assert_eq!(actual, common::expected_envelope_for_spec_close_unchecked_blocking());

    // State parity: spec is still open, card is unmutated.
    let spec_text = std::fs::read_to_string(dir.path().join(".orbit/specs/0001/spec.yaml")).unwrap();
    assert!(spec_text.contains("status: open"), "spec mutated: {spec_text}");
    let card_text = std::fs::read_to_string(dir.path().join(".orbit/cards/0020-orbit-state.yaml")).unwrap();
    assert!(!card_text.contains("specs:"), "card specs array touched: {card_text}");
}

#[test]
fn spec_close_cli_force_proceeds_with_envelope() {
    // ac-05 / ac-03: CLI `spec close --force` bypasses the unchecked-AC
    // guard and emits the canonical ok envelope with `forced_unchecked`
    // and `deferrable_open` populated.
    let dir = tempfile::tempdir().unwrap();
    common::populate_spec_close_preflight_fixture(dir.path());

    let cli_bin = env!("CARGO_BIN_EXE_orbit");
    let output = Command::new(cli_bin)
        .args(["--root", dir.path().to_str().unwrap(), "--json", "spec", "close", "0001", "--force"])
        .stdin(Stdio::null())
        .output()
        .expect("run cli");

    assert!(output.status.success(), "force should succeed: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf-8");
    let actual = stdout.trim_end_matches('\n');
    assert_eq!(actual, common::expected_envelope_for_spec_close_force());

    // State parity: spec is closed on disk, card's specs array gained the ref.
    let spec_text = std::fs::read_to_string(dir.path().join(".orbit/specs/0001/spec.yaml")).unwrap();
    assert!(spec_text.contains("status: closed"), "spec not closed: {spec_text}");
    let card_text = std::fs::read_to_string(dir.path().join(".orbit/cards/0020-orbit-state.yaml")).unwrap();
    assert!(
        card_text.contains(".orbit/specs/0001/spec.yaml"),
        "card not updated: {card_text}"
    );
}

#[test]
fn spec_close_cli_deferrable_only_proceeds_without_force() {
    // spec 2026-05-16-ac-taxonomy ac-02 (generalising ac-05 / ac-04 of
    // the precursor): CLI `spec close` against a spec whose sole unchecked
    // AC is deferrable-kind (Observation) succeeds without `--force`;
    // envelope carries `deferrable_open` and empty `forced_unchecked`.
    let dir = tempfile::tempdir().unwrap();
    common::populate_spec_close_only_deferrable_fixture(dir.path());

    let cli_bin = env!("CARGO_BIN_EXE_orbit");
    let output = Command::new(cli_bin)
        .args(["--root", dir.path().to_str().unwrap(), "--json", "spec", "close", "0001"])
        .stdin(Stdio::null())
        .output()
        .expect("run cli");

    assert!(output.status.success(), "close should succeed: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf-8");
    let actual = stdout.trim_end_matches('\n');
    assert_eq!(actual, common::expected_envelope_for_spec_close_only_deferrable());

    // State parity: spec is closed.
    let spec_text = std::fs::read_to_string(dir.path().join(".orbit/specs/0001/spec.yaml")).unwrap();
    assert!(spec_text.contains("status: closed"), "spec not closed: {spec_text}");
}

// ---------------------------------------------------------------------------
// Spec 2026-05-15-agent-learning-loop parity tests
// ---------------------------------------------------------------------------

#[test]
fn session_start_cli_envelope_matches_canonical() {
    let dir = tempfile::tempdir().unwrap();
    let cli_bin = env!("CARGO_BIN_EXE_orbit");
    let output = Command::new(cli_bin)
        .args([
            "--root",
            dir.path().to_str().unwrap(),
            "--json",
            "session",
            "start",
            "--id",
            common::PARITY_SESSION_ID,
        ])
        .stdin(Stdio::null())
        .output()
        .expect("run cli");
    assert!(output.status.success(), "stderr={}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf-8");
    let actual = stdout.trim_end_matches('\n');
    assert_eq!(actual, common::expected_envelope_for_session_start(dir.path()));

    let on_disk = std::fs::read_to_string(dir.path().join(".orbit/.session-id")).unwrap();
    assert_eq!(on_disk.trim(), common::PARITY_SESSION_ID);
}

#[test]
fn skill_record_invocation_cli_envelope_matches_canonical() {
    let dir = tempfile::tempdir().unwrap();
    let cli_bin = env!("CARGO_BIN_EXE_orbit");
    let output = Command::new(cli_bin)
        .args([
            "--root",
            dir.path().to_str().unwrap(),
            "--json",
            "skill",
            "record-invocation",
            "card",
            "--outcome",
            "worked",
            "--session-id",
            common::PARITY_SESSION_ID,
            "--timestamp",
            common::PARITY_TIMESTAMP,
        ])
        .env_remove("ORBIT_SESSION_ID")
        .stdin(Stdio::null())
        .output()
        .expect("run cli");
    assert!(output.status.success(), "stderr={}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf-8");
    let actual = stdout.trim_end_matches('\n');
    assert_eq!(actual, common::expected_envelope_for_skill_record_invocation());

    // State parity: one JSONL row on disk.
    let path = dir.path().join(".orbit/skills/card.invocations.jsonl");
    let body = std::fs::read_to_string(&path).unwrap();
    assert_eq!(body.lines().count(), 1);
}

#[test]
fn skill_recurrence_cli_envelope_empty_matches_canonical() {
    let dir = tempfile::tempdir().unwrap();
    let cli_bin = env!("CARGO_BIN_EXE_orbit");
    let output = Command::new(cli_bin)
        .args([
            "--root",
            dir.path().to_str().unwrap(),
            "--json",
            "skill",
            "recurrence",
            "design",
        ])
        .stdin(Stdio::null())
        .output()
        .expect("run cli");
    assert!(output.status.success(), "stderr={}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf-8");
    let actual = stdout.trim_end_matches('\n');
    assert_eq!(actual, common::expected_envelope_for_skill_recurrence_empty());
}

#[test]
fn session_distill_cli_envelope_matches_canonical() {
    use orbit_state_core::schema::Session;
    let dir = tempfile::tempdir().unwrap();
    let cli_bin = env!("CARGO_BIN_EXE_orbit");

    // Write the distillate via --from to avoid stdin plumbing.
    let from = dir.path().join("distillate.txt");
    std::fs::write(&from, "parity-distillate").unwrap();

    let output = Command::new(cli_bin)
        .args([
            "--root",
            dir.path().to_str().unwrap(),
            "--json",
            "session",
            "distill",
            "--session-id",
            common::PARITY_SESSION_ID,
            "--from",
            from.to_str().unwrap(),
        ])
        .env_remove("ORBIT_SESSION_ID")
        .stdin(Stdio::null())
        .output()
        .expect("run cli");
    assert!(output.status.success(), "stderr={}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf-8");
    let actual = stdout.trim_end_matches('\n');

    // Read substrate-stamped timestamps from disk.
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
    assert_eq!(actual, expected);
}

// Helper visible to ensure the test binary depends on the CLI binary.
#[allow(dead_code)]
fn _binary_dep_anchor(_p: &Path) {}
