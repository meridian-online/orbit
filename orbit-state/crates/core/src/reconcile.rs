//! `--reconcile` mode for `orbit canonicalise`: a permissive YAML pass that
//! brings legacy field shapes into the canonical schema.
//!
//! Per ac-01 the permissive read lives in THIS module only. Every canonical
//! schema struct in [`crate::schema`] keeps `deny_unknown_fields`; routine
//! paths (`orbit verify`, `orbit canonicalise` without `--reconcile`, every
//! other verb) parse strictly. The mode is reachable only when the CLI
//! invokes `reconcile_all` after the user passes `--reconcile`.
//!
//! Pipeline per file:
//!   1. Read raw bytes; parse as `serde_yaml::Value` (no schema validation).
//!   2. Walk the top-level mapping. For each key not in the entity's
//!      canonical [`crate::schema`] `FIELDS`, look up `FIELD_RULES` for a
//!      registry rule. `Map` renames the key; `Drop` removes it; the
//!      default is `Quarantine` — the entry is moved into a sibling
//!      `<name>.legacy.yaml` sidecar so a human can re-anchor it later.
//!   3. Recurse into list-of-struct inner shapes — `Spec.acceptance_criteria[]`
//!      against `AcceptanceCriterion::FIELDS`, `Card.scenarios[]` against
//!      `Scenario::FIELDS`, `Card.relations[]` against `Relation::FIELDS`.
//!      Inner-field dispositions record structural paths like
//!      `acceptance_criteria[2].ac_type`.
//!   4. Re-parse the cleaned `Value` against the typed schema and reserialise
//!      via the canonical writer. If parsing now fails, the file is reported
//!      as `parse_failed` (a registry gap we couldn't paper over with
//!      quarantine alone — e.g. a missing required field).
//!   5. Sidecar write goes FIRST (atomic), then the canonical rewrite —
//!      this ordering means a crash between writes leaves the quarantined
//!      content recoverable rather than silently destroyed.
//!
//! Idempotency:
//!   - Clean tree → no-op (exit 0, empty disposition list).
//!   - Re-runs that encounter an existing sidecar parse it, merge new
//!     entries (by parsed-`Value` equality, not byte equality), and rewrite
//!     only if the set changed.
//!
//! This module is intentionally invoked only from CLI's `run_canonicalise`
//! when `--reconcile` is passed; the canonical helpers in [`crate::canonicalise`]
//! never see it.

use crate::atomic::write_atomic;
use crate::canonical::{parse_yaml, serialise_yaml};
use crate::layout::OrbitLayout;
use crate::schema::{AcceptanceCriterion, Card, Choice, Memory, Relation, Scenario, Spec};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::path::{Path, PathBuf};

// ============================================================================
// Public types
// ============================================================================

/// Entity type a registry rule applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntityType {
    Card,
    Spec,
    Choice,
    Memory,
}

impl EntityType {
    pub fn as_str(&self) -> &'static str {
        match self {
            EntityType::Card => "card",
            EntityType::Spec => "spec",
            EntityType::Choice => "choice",
            EntityType::Memory => "memory",
        }
    }
}

/// Disposition applied to a legacy field. The default for an unknown field
/// without a registry rule is [`Disposition::Quarantine`] — the substrate
/// never silently destroys content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// Rename the field to the named canonical field; the value passes
    /// through unchanged.
    Map(&'static str),
    /// Remove the field entirely. Use only when the value carries no
    /// semantic content (e.g. `version: '0.1'` on a Spec authored before
    /// the schema-version file owned that concept).
    Drop,
    /// Move the field into a sibling `<name>.legacy.yaml`. The default
    /// when no rule matches.
    Quarantine,
}

impl Disposition {
    pub fn action_str(&self) -> &'static str {
        match self {
            Disposition::Map(_) => "map",
            Disposition::Drop => "drop",
            Disposition::Quarantine => "quarantine",
        }
    }
}

/// Default registry. Keyed by `(EntityType, structural_path)`; the
/// `Disposition` is the looked-up value, not a key dimension.
///
/// Structural-path syntax:
///   - top-level field on the entity: `"<name>"`
///   - inner field inside a list-of-struct: `"<list>[].<name>"`
///     (matches every list element since inner shape is uniform).
///
/// Seeded entries cover the legacy fields encountered in the substrate
/// today plus archive evidence (`.orbit/archive/`). Adding a new rule
/// requires the legacy field to be encountered on at least one entity
/// type — speculative entries are not seeded.
pub const FIELD_RULES: &[(EntityType, &str, Disposition)] = &[
    // Pre-orbit-state Spec metadata: version + date_opened were authored
    // before the schema-version file owned that concept. No semantic
    // content to preserve.
    (EntityType::Spec, "version", Disposition::Drop),
    (EntityType::Spec, "date_opened", Disposition::Drop),
    // bd-era inner AC field: `ac_type` was a free-text classifier ("code",
    // "gate", "test", ...). The canonical schema replaces it with the
    // `gate` boolean; the rest of the taxonomy carries no enforced
    // semantics. Drop the inner field — gate-ness is captured separately.
    (
        EntityType::Spec,
        "acceptance_criteria[].ac_type",
        Disposition::Drop,
    ),
    // bd-era top-level prose fields on Spec — predecessor_evidence,
    // constraints, exit_conditions — carry semantic content and have no
    // canonical equivalent yet, so they default to quarantine (no rule
    // here). The sidecar preserves the prose for a human to re-anchor.
];

/// One disposition applied (or to-be-applied in dry-run) during a reconcile
/// pass. Surfaces in the run summary and the JSON envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispositionRecord {
    /// File path relative to the parent of `.orbit/` (e.g.
    /// `.orbit/specs/<id>/spec.yaml`).
    pub path: String,
    /// Entity kind: `"card"` / `"spec"` / `"choice"` / `"memory"`.
    pub kind: String,
    /// Structural field path (e.g. `"version"` or
    /// `"acceptance_criteria[2].ac_type"`).
    pub field: String,
    /// Action: `"map"` / `"drop"` / `"quarantine"`.
    pub action: String,
}

/// Aggregate result of `reconcile_all`. Mirrors
/// [`crate::canonicalise::CanonicaliseReport`]'s shape plus a
/// `dispositions` list.
#[derive(Debug, Default)]
pub struct ReconcileReport {
    pub rewrote: usize,
    pub unchanged: usize,
    pub parse_failed: Vec<(PathBuf, String)>,
    pub dispositions: Vec<DispositionRecord>,
}

impl ReconcileReport {
    pub fn has_failures(&self) -> bool {
        !self.parse_failed.is_empty()
    }
    pub fn has_dispositions(&self) -> bool {
        !self.dispositions.is_empty()
    }
}

// ============================================================================
// Public entry
// ============================================================================

/// Walk every canonical entity type and apply reconcile dispositions.
///
/// In `dry_run` mode no file is written; the report still records every
/// disposition that *would* have been applied so callers can preview.
pub fn reconcile_all(layout: &OrbitLayout, dry_run: bool) -> ReconcileReport {
    let mut report = ReconcileReport::default();
    let orbit_parent = layout.root.parent().unwrap_or(&layout.root).to_path_buf();

    for path in list_or_empty(layout.list_card_files()) {
        reconcile_one::<Card>(&path, EntityType::Card, &orbit_parent, dry_run, &mut report);
    }
    for path in list_or_empty(layout.list_spec_files()) {
        reconcile_one::<Spec>(&path, EntityType::Spec, &orbit_parent, dry_run, &mut report);
    }
    for path in list_or_empty(layout.list_choice_files()) {
        reconcile_one::<Choice>(&path, EntityType::Choice, &orbit_parent, dry_run, &mut report);
    }
    for path in list_or_empty(layout.list_memory_files()) {
        reconcile_one::<Memory>(&path, EntityType::Memory, &orbit_parent, dry_run, &mut report);
    }

    report
}

fn list_or_empty(result: std::io::Result<Vec<PathBuf>>) -> Vec<PathBuf> {
    result.unwrap_or_default()
}

// ============================================================================
// Per-file pipeline
// ============================================================================

fn reconcile_one<T>(
    path: &Path,
    kind: EntityType,
    orbit_parent: &Path,
    dry_run: bool,
    report: &mut ReconcileReport,
) where
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

    // Permissive parse — no schema validation.
    let mut value: serde_yaml::Value = match serde_yaml::from_str(&original) {
        Ok(v) => v,
        Err(e) => {
            report
                .parse_failed
                .push((path.to_path_buf(), format!("yaml parse failed: {e}")));
            return;
        }
    };

    let display_path = display_path_of(path, orbit_parent);

    // Collect dispositions for this file. We need the full set before any
    // write so we can build the sidecar in one pass.
    let mut file_dispositions: Vec<(DispositionRecord, serde_yaml::Value)> = Vec::new();

    // Walk the top-level mapping plus inner lists-of-struct, mutating
    // `value` in place: Map renames keys, Drop removes them, Quarantine
    // removes them AND records the original value for the sidecar.
    if let Err(e) = walk_and_classify(&mut value, kind, &display_path, &mut file_dispositions) {
        report.parse_failed.push((path.to_path_buf(), e));
        return;
    }

    // Re-parse the cleaned Value through the typed schema and reserialise
    // via the canonical writer. If parsing still fails, the file needs a
    // richer registry rule than v1's rename/drop/quarantine can deliver.
    let reserialised = match value_to_canonical::<T>(&value) {
        Ok(s) => s,
        Err(e) => {
            report.parse_failed.push((path.to_path_buf(), e));
            return;
        }
    };

    // Sidecar payload: existing entries (if any) merged with new ones.
    let sidecar_path = sidecar_path(path);
    let (new_sidecar_yaml, sidecar_changed) = match merge_sidecar(&sidecar_path, &file_dispositions)
    {
        Ok(pair) => pair,
        Err(e) => {
            report.parse_failed.push((path.to_path_buf(), e));
            return;
        }
    };

    let canonical_changed = reserialised != original;

    if !canonical_changed && !sidecar_changed {
        report.unchanged += 1;
        // Even on a no-op file the dispositions vec for this run was empty
        // (because nothing new surfaced). Fall through.
        push_dispositions(&mut report.dispositions, &file_dispositions);
        return;
    }

    if !dry_run {
        // Sidecar-first: a crash between writes leaves quarantined content
        // on disk and the canonical file untouched (recoverable). The
        // opposite ordering would silently destroy content.
        if sidecar_changed {
            if let Err(e) = write_sidecar(&sidecar_path, &new_sidecar_yaml) {
                report
                    .parse_failed
                    .push((path.to_path_buf(), format!("sidecar write: {e}")));
                return;
            }
        }
        if canonical_changed {
            if let Err(e) = write_atomic(path, reserialised.as_bytes()) {
                report
                    .parse_failed
                    .push((path.to_path_buf(), format!("canonical write: {e}")));
                return;
            }
        }
    }

    report.rewrote += 1;
    push_dispositions(&mut report.dispositions, &file_dispositions);
}

fn push_dispositions(
    out: &mut Vec<DispositionRecord>,
    file_dispositions: &[(DispositionRecord, serde_yaml::Value)],
) {
    for (record, _) in file_dispositions {
        out.push(record.clone());
    }
}

// ============================================================================
// Classify
// ============================================================================

fn walk_and_classify(
    value: &mut serde_yaml::Value,
    kind: EntityType,
    display_path: &str,
    out: &mut Vec<(DispositionRecord, serde_yaml::Value)>,
) -> std::result::Result<(), String> {
    let top_fields: &[&str] = match kind {
        EntityType::Card => Card::FIELDS,
        EntityType::Spec => Spec::FIELDS,
        EntityType::Choice => Choice::FIELDS,
        EntityType::Memory => Memory::FIELDS,
    };

    let mapping = match value.as_mapping_mut() {
        Some(m) => m,
        None => return Err("root is not a mapping".into()),
    };

    // Iterate over a snapshot of keys so we can mutate the mapping
    // (rename / remove) without invalidating an active iterator.
    let keys: Vec<String> = mapping
        .iter()
        .filter_map(|(k, _)| k.as_str().map(String::from))
        .collect();

    for key in keys {
        if top_fields.contains(&key.as_str()) {
            // Known canonical field. Recurse into list-of-struct inner
            // shapes; leave scalar / other fields alone.
            recurse_inner(mapping, kind, &key, display_path, out)?;
            continue;
        }

        // Unknown top-level field — look up registry.
        let disposition = lookup_disposition(kind, &key);
        let (record, payload) = apply_top_level(mapping, &key, disposition, display_path, kind);
        if let Some(p) = payload {
            out.push((record, p));
        } else {
            out.push((record, serde_yaml::Value::Null));
        }
    }

    Ok(())
}

/// Recurse into a known top-level field if it carries inner-shape drift
/// (lists of structs whose inner fields can themselves be legacy).
fn recurse_inner(
    mapping: &mut serde_yaml::Mapping,
    kind: EntityType,
    field_name: &str,
    display_path: &str,
    out: &mut Vec<(DispositionRecord, serde_yaml::Value)>,
) -> std::result::Result<(), String> {
    let (inner_fields, inner_path_prefix): (&[&str], &str) = match (kind, field_name) {
        (EntityType::Spec, "acceptance_criteria") => {
            (AcceptanceCriterion::FIELDS, "acceptance_criteria")
        }
        (EntityType::Card, "scenarios") => (Scenario::FIELDS, "scenarios"),
        (EntityType::Card, "relations") => (Relation::FIELDS, "relations"),
        _ => return Ok(()),
    };

    let list_value = match mapping.get_mut(serde_yaml::Value::String(field_name.into())) {
        Some(v) => v,
        None => return Ok(()),
    };
    let seq = match list_value.as_sequence_mut() {
        Some(s) => s,
        None => return Ok(()),
    };

    for (idx, item) in seq.iter_mut().enumerate() {
        let inner_map = match item.as_mapping_mut() {
            Some(m) => m,
            None => continue,
        };

        let inner_keys: Vec<String> = inner_map
            .iter()
            .filter_map(|(k, _)| k.as_str().map(String::from))
            .collect();

        for inner_key in inner_keys {
            if inner_fields.contains(&inner_key.as_str()) {
                continue;
            }

            let registry_path = format!("{}[].{}", inner_path_prefix, inner_key);
            let disposition = lookup_disposition(kind, &registry_path);

            let display_field = format!("{}[{}].{}", inner_path_prefix, idx, inner_key);
            let (record, payload) = apply_inner(
                inner_map,
                &inner_key,
                disposition,
                &display_field,
                display_path,
                kind,
            );
            if let Some(p) = payload {
                out.push((record, p));
            } else {
                out.push((record, serde_yaml::Value::Null));
            }
        }
    }
    Ok(())
}

fn apply_top_level(
    mapping: &mut serde_yaml::Mapping,
    key: &str,
    disposition: Disposition,
    display_path: &str,
    kind: EntityType,
) -> (DispositionRecord, Option<serde_yaml::Value>) {
    let key_value = serde_yaml::Value::String(key.into());
    let record = DispositionRecord {
        path: display_path.to_string(),
        kind: kind.as_str().to_string(),
        field: key.to_string(),
        action: disposition.action_str().to_string(),
    };
    match disposition {
        Disposition::Map(new_name) => {
            if let Some(v) = mapping.remove(&key_value) {
                mapping.insert(serde_yaml::Value::String(new_name.into()), v);
            }
            (record, None)
        }
        Disposition::Drop => {
            mapping.remove(&key_value);
            (record, None)
        }
        Disposition::Quarantine => {
            let payload = mapping.remove(&key_value);
            (record, payload)
        }
    }
}

fn apply_inner(
    inner_map: &mut serde_yaml::Mapping,
    inner_key: &str,
    disposition: Disposition,
    display_field: &str,
    display_path: &str,
    kind: EntityType,
) -> (DispositionRecord, Option<serde_yaml::Value>) {
    let key_value = serde_yaml::Value::String(inner_key.into());
    let record = DispositionRecord {
        path: display_path.to_string(),
        kind: kind.as_str().to_string(),
        field: display_field.to_string(),
        action: disposition.action_str().to_string(),
    };
    match disposition {
        Disposition::Map(new_name) => {
            if let Some(v) = inner_map.remove(&key_value) {
                inner_map.insert(serde_yaml::Value::String(new_name.into()), v);
            }
            (record, None)
        }
        Disposition::Drop => {
            inner_map.remove(&key_value);
            (record, None)
        }
        Disposition::Quarantine => {
            let payload = inner_map.remove(&key_value);
            (record, payload)
        }
    }
}

fn lookup_disposition(kind: EntityType, structural_path: &str) -> Disposition {
    for (rule_kind, rule_path, disposition) in FIELD_RULES {
        if *rule_kind == kind && *rule_path == structural_path {
            return *disposition;
        }
    }
    Disposition::Quarantine
}

// ============================================================================
// Canonical re-emit
// ============================================================================

fn value_to_canonical<T>(value: &serde_yaml::Value) -> std::result::Result<String, String>
where
    T: DeserializeOwned + Serialize,
{
    // Round-trip through Value -> string -> typed -> string so the canonical
    // writer's deterministic field order applies.
    let intermediate = serde_yaml::to_string(value)
        .map_err(|e| format!("intermediate serialise failed: {e}"))?;
    let parsed: T =
        parse_yaml::<T>(&intermediate).map_err(|e| format!("post-clean parse failed: {e}"))?;
    serialise_yaml(&parsed).map_err(|e| format!("canonical serialise failed: {e}"))
}

// ============================================================================
// Sidecar I/O
// ============================================================================

fn sidecar_path(canonical: &Path) -> PathBuf {
    let parent = canonical.parent().expect("canonical path has parent");
    let stem = canonical
        .file_stem()
        .and_then(|s| s.to_str())
        .expect("canonical path has stem");
    parent.join(format!("{stem}.legacy.yaml"))
}

/// Compute the merged sidecar payload by combining existing entries (if any)
/// with the new dispositions of action `quarantine`. Existing-entry equality
/// is checked at the parsed-`Value` level so trivial yaml whitespace
/// differences do not trigger a rewrite (per ac-06).
fn merge_sidecar(
    sidecar: &Path,
    file_dispositions: &[(DispositionRecord, serde_yaml::Value)],
) -> std::result::Result<(String, bool), String> {
    // Existing sidecar entries.
    let existing_entries: Vec<SidecarEntry> = if sidecar.exists() {
        let text = std::fs::read_to_string(sidecar)
            .map_err(|e| format!("sidecar read: {e}"))?;
        if text.trim().is_empty() {
            Vec::new()
        } else {
            serde_yaml::from_str(&text).map_err(|e| format!("sidecar parse: {e}"))?
        }
    } else {
        Vec::new()
    };

    // New entries from this run (quarantine only — map/drop don't populate
    // the sidecar).
    let mut entries = existing_entries.clone();
    let mut changed_logical = false;
    for (record, payload) in file_dispositions {
        if record.action != "quarantine" {
            continue;
        }
        let new_entry = SidecarEntry {
            path: record.field.clone(),
            value: payload.clone(),
        };
        if entries.iter().any(|e| e == &new_entry) {
            continue;
        }
        entries.push(new_entry);
        changed_logical = true;
    }

    // No-quarantine and no-existing → no sidecar to write.
    if entries.is_empty() {
        return Ok((String::new(), false));
    }

    // Determine if the rewritten content would differ from disk. The byte
    // comparison is only used to decide whether to write; logical equality
    // (Value-level) drives the changed-set comparison above.
    let new_text = serde_yaml::to_string(&entries)
        .map_err(|e| format!("sidecar serialise: {e}"))?;
    let new_text = if new_text.ends_with('\n') {
        new_text
    } else {
        format!("{new_text}\n")
    };
    let on_disk = if sidecar.exists() {
        std::fs::read_to_string(sidecar).unwrap_or_default()
    } else {
        String::new()
    };
    let bytes_differ = on_disk != new_text;
    Ok((new_text, changed_logical || (bytes_differ && !on_disk.is_empty() && entries != existing_entries)))
}

fn write_sidecar(path: &Path, content: &str) -> std::result::Result<(), String> {
    write_atomic(path, content.as_bytes()).map_err(|e| format!("write_atomic: {e}"))
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct SidecarEntry {
    path: String,
    value: serde_yaml::Value,
}

// ============================================================================
// Path display
// ============================================================================

fn display_path_of(path: &Path, orbit_parent: &Path) -> String {
    if let Ok(rel) = path.strip_prefix(orbit_parent) {
        return rel.to_string_lossy().into_owned();
    }
    path.to_string_lossy().into_owned()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn fresh_layout() -> (tempfile::TempDir, OrbitLayout) {
        let dir = tempdir().unwrap();
        let layout = OrbitLayout::at(dir.path());
        layout.ensure_dirs().unwrap();
        (dir, layout)
    }

    fn write_spec(layout: &OrbitLayout, id: &str, body: &str) -> PathBuf {
        layout.ensure_spec_dir(id).unwrap();
        let path = layout.spec_file(id);
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn clean_substrate_is_no_op() {
        let (_dir, layout) = fresh_layout();
        let report = reconcile_all(&layout, false);
        assert_eq!(report.rewrote, 0);
        assert_eq!(report.unchanged, 0);
        assert!(report.dispositions.is_empty());
        assert!(report.parse_failed.is_empty());
    }

    #[test]
    fn spec_version_dropped_per_registry() {
        // Spec.version is registry-mapped to Drop.
        let (_dir, layout) = fresh_layout();
        let yaml = "id: '0001'\nversion: '0.1'\ngoal: g\nstatus: open\nacceptance_criteria: []\n";
        let path = write_spec(&layout, "0001", yaml);

        let report = reconcile_all(&layout, false);
        assert_eq!(report.rewrote, 1, "expected rewrite, got report: {report:?}");
        assert_eq!(report.dispositions.len(), 1);
        assert_eq!(report.dispositions[0].action, "drop");
        assert_eq!(report.dispositions[0].field, "version");
        assert_eq!(report.dispositions[0].kind, "spec");

        let after = std::fs::read_to_string(&path).unwrap();
        assert!(!after.contains("version"), "version field not dropped: {after}");
        // No sidecar should exist (drop, not quarantine).
        assert!(!sidecar_path(&path).exists(), "drop must not create sidecar");
    }

    #[test]
    fn unknown_top_level_field_quarantined_by_default() {
        let (_dir, layout) = fresh_layout();
        let yaml = "id: '0001'\ngoal: g\nstatus: open\nacceptance_criteria: []\npredecessor_evidence: \"some prose\"\n";
        let path = write_spec(&layout, "0001", yaml);

        let report = reconcile_all(&layout, false);
        assert_eq!(report.rewrote, 1);
        assert_eq!(report.dispositions.len(), 1);
        let d = &report.dispositions[0];
        assert_eq!(d.action, "quarantine");
        assert_eq!(d.field, "predecessor_evidence");
        assert_eq!(d.kind, "spec");

        // Canonical file no longer has the field.
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(!after.contains("predecessor_evidence"));

        // Sidecar exists and records the path + value.
        let side = sidecar_path(&path);
        assert!(side.exists(), "sidecar should exist for quarantined content");
        let side_text = std::fs::read_to_string(&side).unwrap();
        let entries: Vec<SidecarEntry> = serde_yaml::from_str(&side_text).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "predecessor_evidence");
        assert_eq!(
            entries[0].value,
            serde_yaml::Value::String("some prose".into())
        );
    }

    #[test]
    fn inner_field_canonical_ac_type_fires_no_reconcile_disposition() {
        // spec 2026-05-16-ac-taxonomy ac-01: `ac_type` is now a canonical
        // inner field on AcceptanceCriterion. A spec carrying a canonical
        // ac_type value does NOT trigger any reconcile disposition —
        // walk_and_classify recognises `ac_type` as a canonical inner
        // field and recurses past it. (Any canonical-writer normalisation
        // that may still rewrite the file is a separate concern from the
        // reconcile registry; the registry's Drop entry at
        // reconcile.rs:121-125 is dormant for canonical values and will
        // be REPLACED with a Transform rule in ac-06.)
        let (_dir, layout) = fresh_layout();
        let yaml = "id: '0001'\ngoal: g\nstatus: open\nacceptance_criteria:\n- id: ac-01\n  description: do thing\n  gate: true\n  checked: false\n  ac_type: observation\n";
        let path = write_spec(&layout, "0001", yaml);

        let report = reconcile_all(&layout, false);
        assert_eq!(
            report.dispositions.len(),
            0,
            "canonical ac_type must not fire any reconcile disposition; got: {:?}",
            report.dispositions
        );

        let after = std::fs::read_to_string(&path).unwrap();
        assert!(
            after.contains("ac_type: observation"),
            "non-default canonical ac_type must survive a reconcile pass:\n{after}"
        );
    }

    #[test]
    fn inner_unknown_field_quarantined() {
        // An inner AC field with no registry rule → default quarantine.
        let (_dir, layout) = fresh_layout();
        let yaml = "id: '0001'\ngoal: g\nstatus: open\nacceptance_criteria:\n- id: ac-01\n  description: d\n  gate: false\n  checked: false\n  predecessor_evidence: prose\n";
        let path = write_spec(&layout, "0001", yaml);

        let report = reconcile_all(&layout, false);
        assert_eq!(report.rewrote, 1);
        assert_eq!(report.dispositions.len(), 1);
        let d = &report.dispositions[0];
        assert_eq!(d.action, "quarantine");
        assert_eq!(d.field, "acceptance_criteria[0].predecessor_evidence");

        let side = sidecar_path(&path);
        assert!(side.exists());
        let entries: Vec<SidecarEntry> =
            serde_yaml::from_str(&std::fs::read_to_string(&side).unwrap()).unwrap();
        assert_eq!(entries[0].path, "acceptance_criteria[0].predecessor_evidence");
    }

    #[test]
    fn dry_run_does_not_write() {
        let (_dir, layout) = fresh_layout();
        let yaml = "id: '0001'\nversion: '0.1'\ngoal: g\nstatus: open\nacceptance_criteria: []\npredecessor_evidence: prose\n";
        let path = write_spec(&layout, "0001", yaml);

        let report = reconcile_all(&layout, true);
        assert_eq!(report.rewrote, 1, "dry-run still reports would-rewrite");
        assert_eq!(report.dispositions.len(), 2);

        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(after, yaml, "dry-run must not rewrite the canonical file");
        assert!(
            !sidecar_path(&path).exists(),
            "dry-run must not write the sidecar"
        );
    }

    #[test]
    fn re_run_after_reconcile_is_no_op() {
        // ac-06: idempotency. After a clean reconcile, the second run
        // discovers no new unknown fields and rewrites nothing.
        let (_dir, layout) = fresh_layout();
        let yaml = "id: '0001'\nversion: '0.1'\ngoal: g\nstatus: open\nacceptance_criteria: []\npredecessor_evidence: prose\n";
        write_spec(&layout, "0001", yaml);

        let first = reconcile_all(&layout, false);
        assert_eq!(first.rewrote, 1);

        let second = reconcile_all(&layout, false);
        assert_eq!(second.rewrote, 0, "second pass must be no-op");
        assert_eq!(second.unchanged, 1);
        assert!(
            second.dispositions.is_empty(),
            "no new dispositions on second pass"
        );
    }

    #[test]
    fn re_run_merges_new_quarantine_entries() {
        // ac-06: a re-run that surfaces a NEW unknown field merges into
        // the existing sidecar without disturbing earlier entries.
        let (_dir, layout) = fresh_layout();
        let yaml_a = "id: '0001'\ngoal: g\nstatus: open\nacceptance_criteria: []\nfield_a: A\n";
        let path = write_spec(&layout, "0001", yaml_a);
        let first = reconcile_all(&layout, false);
        assert_eq!(first.rewrote, 1);
        assert_eq!(first.dispositions.len(), 1);

        // Author adds another unknown field by hand (simulating drift).
        let yaml_b = "id: '0001'\ngoal: g\nstatus: open\nacceptance_criteria: []\nfield_b: B\n";
        std::fs::write(&path, yaml_b).unwrap();

        let second = reconcile_all(&layout, false);
        assert_eq!(second.rewrote, 1);
        assert_eq!(second.dispositions.len(), 1);
        assert_eq!(second.dispositions[0].field, "field_b");

        let side = sidecar_path(&path);
        let entries: Vec<SidecarEntry> =
            serde_yaml::from_str(&std::fs::read_to_string(&side).unwrap()).unwrap();
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"field_a"), "field_a preserved across re-run");
        assert!(paths.contains(&"field_b"), "field_b added on re-run");
    }

    #[test]
    fn parse_failure_recorded_not_rewritten() {
        let (_dir, layout) = fresh_layout();
        write_spec(&layout, "0001", ": not yaml :: at all");

        let report = reconcile_all(&layout, false);
        assert_eq!(report.parse_failed.len(), 1);
        assert_eq!(report.rewrote, 0);
    }

    #[test]
    fn card_scenarios_inner_field_walked() {
        let (_dir, layout) = fresh_layout();
        let yaml = "feature: f\ngoal: g\nmaturity: planned\nscenarios:\n- name: n\n  given: g\n  when: w\n  then: t\n  gate: false\n  legacy_marker: x\n";
        let path = layout.cards_dir().join("0099-x.yaml");
        std::fs::write(&path, yaml).unwrap();

        let report = reconcile_all(&layout, false);
        assert_eq!(report.rewrote, 1);
        assert_eq!(report.dispositions.len(), 1);
        let d = &report.dispositions[0];
        assert_eq!(d.action, "quarantine");
        assert_eq!(d.field, "scenarios[0].legacy_marker");
        assert_eq!(d.kind, "card");
    }

    #[test]
    fn lookup_disposition_returns_drop_for_spec_version() {
        assert_eq!(
            lookup_disposition(EntityType::Spec, "version"),
            Disposition::Drop
        );
    }

    #[test]
    fn lookup_disposition_returns_drop_for_inner_ac_type() {
        assert_eq!(
            lookup_disposition(EntityType::Spec, "acceptance_criteria[].ac_type"),
            Disposition::Drop
        );
    }

    #[test]
    fn lookup_disposition_defaults_to_quarantine() {
        assert_eq!(
            lookup_disposition(EntityType::Spec, "predecessor_evidence"),
            Disposition::Quarantine
        );
        assert_eq!(
            lookup_disposition(EntityType::Card, "no_such_field"),
            Disposition::Quarantine
        );
    }
}
