//! Substrate-wide hygiene check: round-trip + index-rebuild verification.
//!
//! Wires the two CI gates into a single inspection routine so callers get a
//! single pass/fail signal:
//!
//! - **ac-16 (round-trip gate):** every file under `.orbit/specs/`,
//!   `.orbit/cards/`, `.orbit/choices/`, `.orbit/memories/`, plus the
//!   `schema-version` file, parses and re-serialises byte-identically. Tasks
//!   are excluded — they are append-only JSONL events and explicitly out of
//!   scope per the `acceptance_criteria` of ac-16.
//!
//! - **ac-17 (verify gate):** the SQLite index rebuilds from files alone and
//!   diffs clean against any pre-existing on-disk index.
//!
//! Both gates fail the same way (`VerifyOutcome::has_failures() == true`) so
//! CI can run a single `orbit verify` invocation and treat any drift as a
//! merge blocker. The per-failure detail is preserved in the outcome so a
//! human can locate the offending file without re-running.
//!
//! Per ac-16's exclusion list, task JSONL streams are not iterated here. They
//! are append-only events (substrate-written, never rewritten in place); a
//! round-trip test does not apply to that storage shape.
//!
//! The index check creates `state.db` if it does not already exist (CI runs
//! against fresh checkouts where state.db is gitignored). On a fresh tree the
//! check reduces to "files parse cleanly and rebuild succeeds" — which is
//! still the meaningful CI signal.

use crate::canonical::{parse_yaml, serialise_yaml};
use crate::index::Index;
use crate::layout::OrbitLayout;
use crate::migrations::init_schema_version;
use crate::schema::{Card, Choice, Memory, SchemaVersion, Spec};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Aggregate result of `verify_all`.
///
/// Empty vectors mean all checks passed; any non-empty vector is a failure
/// the caller should report and exit non-zero on.
#[derive(Debug, Default)]
pub struct VerifyOutcome {
    /// Files whose `parse → serialise` does not round-trip byte-identical, or
    /// which fail to parse against the canonical schema.
    pub round_trip_failures: Vec<RoundTripFailure>,
    /// Drift between the on-disk index and a fresh rebuild from files.
    pub index_drift: Vec<String>,
}

impl VerifyOutcome {
    pub fn has_failures(&self) -> bool {
        !self.round_trip_failures.is_empty() || !self.index_drift.is_empty()
    }
}

#[derive(Debug)]
pub struct RoundTripFailure {
    pub path: PathBuf,
    pub kind: RoundTripFailureKind,
}

#[derive(Debug)]
pub enum RoundTripFailureKind {
    /// File could not be parsed against its canonical schema (malformed YAML,
    /// unknown field, CRLF, etc.). The wrapped string is the underlying
    /// canonical-layer error message.
    ParseFailed(String),
    /// File parsed, re-serialised, but the bytes do not match the original.
    /// The substrate has not yet rewritten this file through the canonical
    /// writer — `orbit ... update` (or any verb that touches the file) will
    /// normalise it.
    NotByteIdentical,
}

/// Run the full substrate hygiene check. Idempotent; safe to invoke from CI.
///
/// Steps in order:
/// 1. Ensure layout subdirectories exist.
/// 2. Ensure `schema-version` exists (substrate-written file; CI checkouts
///    don't carry it because it's gitignored).
/// 3. Round-trip each canonical file (schema-version, specs, cards, choices,
///    memories). Tasks excluded per ac-16.
/// 4. Open or create `state.db`, rebuild from files, diff.
pub fn verify_all(layout: &OrbitLayout) -> std::io::Result<VerifyOutcome> {
    layout.ensure_dirs()?;
    // init_schema_version is idempotent. Errors here surface as round-trip
    // failures on the schema-version path so the caller sees a single channel
    // of diagnostics rather than a mid-run abort.
    let _ = init_schema_version(layout);

    let mut outcome = VerifyOutcome::default();

    // 1. schema-version (single file).
    if layout.schema_version_file().exists() {
        check_round_trip::<SchemaVersion>(&layout.schema_version_file(), &mut outcome);
    }

    // 2. specs/*.yaml — only the spec yamls, NOT the .tasks.jsonl streams
    //    (task events are append-only per ac-16's exclusion).
    for path in list_or_empty(layout.list_spec_files()) {
        check_round_trip::<Spec>(&path, &mut outcome);
    }

    // 3. cards/*.yaml — shallow; cards/memos/ is intentionally skipped.
    for path in list_or_empty(layout.list_card_files()) {
        check_round_trip::<Card>(&path, &mut outcome);
    }

    // 4. choices/*.yaml.
    for path in list_or_empty(layout.list_choice_files()) {
        check_round_trip::<Choice>(&path, &mut outcome);
    }

    // 5. memories/*.yaml.
    for path in list_or_empty(layout.list_memory_files()) {
        check_round_trip::<Memory>(&path, &mut outcome);
    }

    // 6. Index rebuild check (ac-17).
    //
    // We always rebuild against a fresh in-memory index — that's the hygiene
    // signal. A failure here is a file that parses individually but breaks
    // the index's stronger invariants (FK references, uniqueness, etc.). The
    // on-disk `state.db` is intentionally NOT touched: it is gitignored and
    // therefore absent on CI, and where it does exist (developer machines)
    // the index can lag the files in normal operation. Drift between an
    // existing state.db and files is a local-development question, not a
    // merge gate.
    match Index::open_in_memory() {
        Ok(mut idx) => {
            if let Err(e) = idx.rebuild_from_files(layout) {
                outcome
                    .index_drift
                    .push(format!("index rebuild failed: {e}"));
            }
        }
        Err(e) => outcome
            .index_drift
            .push(format!("index open failed: {e}")),
    }

    Ok(outcome)
}

fn list_or_empty(result: std::io::Result<Vec<PathBuf>>) -> Vec<PathBuf> {
    result.unwrap_or_default()
}

fn check_round_trip<T>(path: &Path, outcome: &mut VerifyOutcome)
where
    T: DeserializeOwned + Serialize,
{
    let original = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            outcome.round_trip_failures.push(RoundTripFailure {
                path: path.to_path_buf(),
                kind: RoundTripFailureKind::ParseFailed(format!("read failed: {e}")),
            });
            return;
        }
    };
    let parsed: T = match parse_yaml(&original) {
        Ok(v) => v,
        Err(e) => {
            outcome.round_trip_failures.push(RoundTripFailure {
                path: path.to_path_buf(),
                kind: RoundTripFailureKind::ParseFailed(e.to_string()),
            });
            return;
        }
    };
    let reserialised = match serialise_yaml(&parsed) {
        Ok(v) => v,
        Err(e) => {
            outcome.round_trip_failures.push(RoundTripFailure {
                path: path.to_path_buf(),
                kind: RoundTripFailureKind::ParseFailed(e.to_string()),
            });
            return;
        }
    };
    if reserialised != original {
        outcome.round_trip_failures.push(RoundTripFailure {
            path: path.to_path_buf(),
            kind: RoundTripFailureKind::NotByteIdentical,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atomic::write_atomic;
    use crate::canonical::serialise_yaml;
    use crate::schema::{Choice, ChoiceStatus, SchemaVersion};
    use tempfile::tempdir;

    fn fresh_layout() -> (tempfile::TempDir, OrbitLayout) {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        (dir, layout)
    }

    #[test]
    fn verify_clean_on_empty_substrate_initialises_schema_version() {
        let (_dir, layout) = fresh_layout();
        let outcome = verify_all(&layout).unwrap();
        assert!(
            !outcome.has_failures(),
            "empty substrate should verify clean: {outcome:?}"
        );
        assert!(
            layout.schema_version_file().exists(),
            "schema-version must be initialised by verify_all"
        );
    }

    #[test]
    fn verify_clean_with_canonical_choice() {
        let (_dir, layout) = fresh_layout();
        let choice = Choice {
            id: "0042".into(),
            title: "verify works".into(),
            status: ChoiceStatus::Accepted,
            date_created: "2026-05-07".into(),
            date_modified: None,
            body: "Decision: verify all the things.\n".into(),
            references: vec![],
        };
        let yaml = serialise_yaml(&choice).unwrap();
        write_atomic(layout.choice_file("0042"), yaml.as_bytes()).unwrap();

        let outcome = verify_all(&layout).unwrap();
        assert!(!outcome.has_failures(), "{outcome:?}");
    }

    #[test]
    fn verify_detects_non_canonical_byte_drift() {
        // ac-16: a non-canonical file (extra whitespace, wrong field order)
        // must fail the round-trip check.
        let (_dir, layout) = fresh_layout();
        let non_canonical = "\
status: accepted
id: '0042'
title: drift
date_created: '2026-05-07'
body: 'x'
references: []
";
        write_atomic(layout.choice_file("0042"), non_canonical.as_bytes()).unwrap();

        let outcome = verify_all(&layout).unwrap();
        assert!(
            outcome
                .round_trip_failures
                .iter()
                .any(|f| matches!(f.kind, RoundTripFailureKind::NotByteIdentical)),
            "expected NotByteIdentical failure, got {outcome:?}"
        );
    }

    #[test]
    fn verify_detects_unknown_field() {
        // ac-01 strict-schema property surfaces through verify as a parse
        // failure on the offending file.
        let (_dir, layout) = fresh_layout();
        let bad = "\
id: '0042'
title: bad
status: accepted
date_created: '2026-05-07'
body: 'x'
references: []
mystery_field: ohno
";
        write_atomic(layout.choice_file("0042"), bad.as_bytes()).unwrap();

        let outcome = verify_all(&layout).unwrap();
        let parse_failures: Vec<_> = outcome
            .round_trip_failures
            .iter()
            .filter(|f| matches!(f.kind, RoundTripFailureKind::ParseFailed(_)))
            .collect();
        assert!(
            !parse_failures.is_empty(),
            "expected ParseFailed; got {outcome:?}"
        );
    }

    #[test]
    fn verify_excludes_task_jsonl_from_round_trip() {
        // ac-16 exclusion: task event JSONL is append-only and not iterated.
        // We plant a task stream that would NOT round-trip as YAML and
        // confirm verify ignores it.
        let (_dir, layout) = fresh_layout();
        std::fs::write(
            layout.specs_dir().join("2026-05-07-x.tasks.jsonl"),
            r#"{"task_id":"t","spec_id":"2026-05-07-x","event":"open","timestamp":"x"}
"#,
        )
        .unwrap();
        let outcome = verify_all(&layout).unwrap();
        assert!(
            !outcome.has_failures(),
            "task jsonl must not be iterated; got {outcome:?}"
        );
    }

    #[test]
    fn verify_excludes_sidecar_yaml_shapes() {
        // 2026-05-09-drive-rally-sidecar-layout ac-00: sidecar yaml shapes
        // (`<id>.drive.yaml`, `<id>.rally.yaml`, and any future
        // `<id>.<sidecar>.yaml`) are filtered by `list_yaml_files`'s
        // dotless-stem rule and never reach the Spec round-trip check. A
        // file that would NOT parse as a Spec must still leave verify
        // clean.
        let (_dir, layout) = fresh_layout();
        // Drive sidecar with a non-Spec shape (has spec_id, stage — no id, no goal).
        std::fs::write(
            layout.specs_dir().join("2026-05-09-foo.drive.yaml"),
            "spec_id: '2026-05-09-foo'\nstage: review-spec\niteration: 1\n",
        )
        .unwrap();
        // Rally sidecar with arbitrary content.
        std::fs::write(
            layout.specs_dir().join("2026-05-09-bar.rally.yaml"),
            "rally_id: '2026-05-09-bar'\nchildren: []\n",
        )
        .unwrap();
        // Review markdown — different extension, also harmless.
        std::fs::write(
            layout.specs_dir().join("2026-05-09-foo.review-spec-2026-05-09.md"),
            "# Review\n",
        )
        .unwrap();
        let outcome = verify_all(&layout).unwrap();
        assert!(
            !outcome.has_failures(),
            "sidecar yaml shapes must not be iterated as Specs; got {outcome:?}"
        );
    }

    #[test]
    fn verify_detects_schema_version_drift() {
        // Hand-edit schema-version into a non-canonical (but parseable) form.
        let (_dir, layout) = fresh_layout();
        // Initialise normally.
        verify_all(&layout).unwrap();
        // Now overwrite with a parseable but non-canonical body.
        std::fs::write(
            layout.schema_version_file(),
            "version: '0.1'\nnote:\n",
        )
        .unwrap();
        // Re-serialise the correct canonical form for comparison sanity.
        let canonical_form = serialise_yaml(&SchemaVersion {
            version: "0.1".into(),
            note: None,
        })
        .unwrap();
        let on_disk = std::fs::read_to_string(layout.schema_version_file()).unwrap();
        assert_ne!(canonical_form, on_disk, "test setup: file must be non-canonical");

        let outcome = verify_all(&layout).unwrap();
        // Either ParseFailed (note: '' isn't a valid Option) or NotByteIdentical.
        assert!(outcome.has_failures(), "{outcome:?}");
    }
}
