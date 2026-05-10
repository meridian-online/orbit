//! Substrate-wide canonicalise pass: parse every canonical YAML, reserialise
//! through the canonical writer, and rewrite any file whose bytes drift from
//! that output.
//!
//! Companion to [`verify`](crate::verify): verify reports drift, canonicalise
//! repairs it. Both modules share the same scope (specs, cards, choices,
//! memories) and the same exclusions (task `.tasks.jsonl` streams are
//! append-only and not round-trippable as YAML; state.db and schema-version
//! are substrate-managed and almost always already canonical).
//!
//! Use cases:
//! - Hand-edited cards/choices that pick up whitespace or field-order drift
//!   relative to the canonical writer's output.
//! - Migrated entities authored before the v0.1 schema froze (the original
//!   one-shot ac-23 use case for the `orbit-canonicalise` standalone binary).
//!
//! `dry_run` mode parses and reserialises without writing — useful for
//! previewing what would change.
//!
//! Files that fail to parse are reported but not rewritten; canonicalise
//! cannot repair structural problems (unknown fields, malformed YAML), only
//! formatting drift on otherwise-valid content.
//!
//! Both the `orbit canonicalise` subcommand of the main CLI and the
//! standalone `orbit-canonicalise` binary call into this module — they
//! differ only in argument parsing and output rendering.

use crate::canonical::{parse_yaml, serialise_yaml};
use crate::layout::OrbitLayout;
use crate::schema::{Card, Choice, Memory, Spec};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Aggregate result of `canonicalise_all`. Counts files by outcome and
/// preserves per-file detail for parse failures so the caller can render
/// actionable diagnostics.
#[derive(Debug, Default)]
pub struct CanonicaliseReport {
    /// Files whose bytes drifted from the canonical writer's output and were
    /// rewritten in place (or would have been, in `dry_run` mode).
    pub rewrote: usize,
    /// Files whose bytes already matched the canonical writer's output.
    pub unchanged: usize,
    /// Files that could not be parsed against their canonical schema. Each
    /// entry pairs the path with the underlying error message.
    pub parse_failed: Vec<(PathBuf, String)>,
}

impl CanonicaliseReport {
    pub fn has_failures(&self) -> bool {
        !self.parse_failed.is_empty()
    }
}

/// Walk the substrate and canonicalise every spec, card, choice, and memory.
/// Tasks (`.tasks.jsonl` streams) are excluded — append-only events are not
/// round-trippable as YAML.
///
/// In `dry_run` mode the walk is read-only; counters reflect what *would*
/// change if a write pass ran. Outside dry-run, drifted files are rewritten
/// atomically by the canonical layer.
pub fn canonicalise_all(layout: &OrbitLayout, dry_run: bool) -> CanonicaliseReport {
    let mut report = CanonicaliseReport::default();

    for path in list_or_empty(layout.list_spec_files()) {
        canonicalise_file::<Spec>(&path, dry_run, &mut report);
    }
    for path in list_or_empty(layout.list_card_files()) {
        canonicalise_card_file(&path, dry_run, &mut report);
    }
    for path in list_or_empty(layout.list_choice_files()) {
        canonicalise_file::<Choice>(&path, dry_run, &mut report);
    }
    for path in list_or_empty(layout.list_memory_files()) {
        canonicalise_file::<Memory>(&path, dry_run, &mut report);
    }

    report
}

fn list_or_empty(result: std::io::Result<Vec<PathBuf>>) -> Vec<PathBuf> {
    result.unwrap_or_default()
}

/// Card-specific canonicalise: parses the file, populates `id` from the
/// filename when missing, validates `id` matches the filename when present,
/// and reserialises through the canonical writer. Per choice 0022, the
/// canonical writer emits `id` as the first field; this function is the
/// migration path that fills it for cards authored before the field existed.
fn canonicalise_card_file(path: &Path, dry_run: bool, report: &mut CanonicaliseReport) {
    let original = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            report
                .parse_failed
                .push((path.to_path_buf(), format!("read: {e}")));
            return;
        }
    };
    let mut parsed: Card = match parse_yaml(&original) {
        Ok(v) => v,
        Err(e) => {
            report.parse_failed.push((path.to_path_buf(), e.to_string()));
            return;
        }
    };
    let expected_id = match path.file_stem().and_then(|s| s.to_str()) {
        Some(s) => s.to_string(),
        None => {
            report
                .parse_failed
                .push((path.to_path_buf(), "filename has no stem".into()));
            return;
        }
    };
    match parsed.id.as_deref() {
        Some(id) if id != expected_id => {
            report.parse_failed.push((
                path.to_path_buf(),
                format!("id mismatch: yaml says `{id}`, filename says `{expected_id}`"),
            ));
            return;
        }
        Some(_) => {}
        None => parsed.id = Some(expected_id),
    }
    let reserialised = match serialise_yaml(&parsed) {
        Ok(s) => s,
        Err(e) => {
            report.parse_failed.push((path.to_path_buf(), e.to_string()));
            return;
        }
    };
    if reserialised == original {
        report.unchanged += 1;
        return;
    }
    if !dry_run {
        if let Err(e) = std::fs::write(path, &reserialised) {
            report
                .parse_failed
                .push((path.to_path_buf(), format!("write: {e}")));
            return;
        }
    }
    report.rewrote += 1;
}

fn canonicalise_file<T>(path: &Path, dry_run: bool, report: &mut CanonicaliseReport)
where
    T: DeserializeOwned + Serialize,
{
    let original = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            report
                .parse_failed
                .push((path.to_path_buf(), format!("read: {e}")));
            return;
        }
    };
    let parsed: T = match parse_yaml(&original) {
        Ok(v) => v,
        Err(e) => {
            report.parse_failed.push((path.to_path_buf(), e.to_string()));
            return;
        }
    };
    let reserialised = match serialise_yaml(&parsed) {
        Ok(s) => s,
        Err(e) => {
            report.parse_failed.push((path.to_path_buf(), e.to_string()));
            return;
        }
    };
    if reserialised == original {
        report.unchanged += 1;
        return;
    }
    if !dry_run {
        if let Err(e) = std::fs::write(path, &reserialised) {
            report
                .parse_failed
                .push((path.to_path_buf(), format!("write: {e}")));
            return;
        }
    }
    report.rewrote += 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::serialise_yaml;
    use crate::schema::{Choice, ChoiceStatus};
    use tempfile::tempdir;

    fn fresh_layout() -> (tempfile::TempDir, OrbitLayout) {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        (dir, layout)
    }

    #[test]
    fn canonicalise_clean_substrate_reports_unchanged() {
        let (_dir, layout) = fresh_layout();
        let report = canonicalise_all(&layout, false);
        assert_eq!(report.rewrote, 0);
        assert_eq!(report.unchanged, 0);
        assert!(report.parse_failed.is_empty());
    }

    #[test]
    fn canonicalise_rewrites_drifted_choice() {
        let (_dir, layout) = fresh_layout();
        let choice = Choice {
            id: "0001".into(),
            title: "Test".into(),
            status: ChoiceStatus::Proposed,
            date_created: "2026-05-08".into(),
            date_modified: Some("2026-05-08".into()),
            body: "# Test\n\nbody\n".into(),
            references: vec![],
        };
        let canonical = serialise_yaml(&choice).unwrap();
        // Write a drifted version (extra trailing whitespace on a non-empty line).
        let drifted = canonical.replace("status: proposed", "status:   proposed");
        let path = layout.choices_dir().join("0001-test.yaml");
        std::fs::write(&path, &drifted).unwrap();

        let report = canonicalise_all(&layout, false);
        assert_eq!(report.rewrote, 1);
        assert!(report.parse_failed.is_empty());

        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(after, canonical, "file should be rewritten to canonical form");
    }

    #[test]
    fn canonicalise_populates_card_id_from_filename() {
        // A card written with no `id:` field — the canonicalise pass must
        // populate it from the filename slug per choice 0022.
        let (_dir, layout) = fresh_layout();
        let yaml = "feature: F\ngoal: G\nmaturity: planned\n";
        let path = layout.cards_dir().join("0099-some-slug.yaml");
        std::fs::write(&path, yaml).unwrap();

        let report = canonicalise_all(&layout, false);
        assert_eq!(report.rewrote, 1);
        assert!(report.parse_failed.is_empty());

        let after = std::fs::read_to_string(&path).unwrap();
        assert!(
            after.starts_with("id: 0099-some-slug\n"),
            "id should be the first field after canonicalise; got:\n{after}"
        );
    }

    #[test]
    fn canonicalise_rejects_card_id_filename_mismatch() {
        // A card whose `id:` disagrees with its filename is a parse error,
        // not silently rewritten.
        let (_dir, layout) = fresh_layout();
        let yaml = "id: 0099-wrong-slug\nfeature: F\ngoal: G\nmaturity: planned\n";
        let path = layout.cards_dir().join("0099-actual-slug.yaml");
        std::fs::write(&path, yaml).unwrap();

        let report = canonicalise_all(&layout, false);
        assert_eq!(report.rewrote, 0);
        assert_eq!(report.unchanged, 0);
        assert_eq!(report.parse_failed.len(), 1);
        assert!(
            report.parse_failed[0].1.contains("id mismatch"),
            "expected id-mismatch error, got: {}",
            report.parse_failed[0].1
        );
    }

    #[test]
    fn canonicalise_dry_run_does_not_write() {
        let (_dir, layout) = fresh_layout();
        let choice = Choice {
            id: "0001".into(),
            title: "Test".into(),
            status: ChoiceStatus::Proposed,
            date_created: "2026-05-08".into(),
            date_modified: Some("2026-05-08".into()),
            body: "# Test\n\nbody\n".into(),
            references: vec![],
        };
        let canonical = serialise_yaml(&choice).unwrap();
        let drifted = canonical.replace("status: proposed", "status:   proposed");
        let path = layout.choices_dir().join("0001-test.yaml");
        std::fs::write(&path, &drifted).unwrap();

        let report = canonicalise_all(&layout, true);
        assert_eq!(report.rewrote, 1);

        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(after, drifted, "dry_run must not write");
    }
}
