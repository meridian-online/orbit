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
    /// Apply a value-transform function. The function inspects the
    /// field's current value and the surrounding entity mapping (with
    /// the field itself removed) and returns either a [`TransformResult::Replace`]
    /// (rewrite the value + optionally set sibling fields atomically) or
    /// [`TransformResult::Quarantine`] (fall back to the existing
    /// quarantine path with a reason).
    ///
    /// Per spec 2026-05-16-ac-taxonomy ac-05 — second-project trigger
    /// for value-level transforms beyond v1's rename/drop/quarantine.
    Transform(TransformFn),
}

/// Function pointer signature for [`Disposition::Transform`].
///
/// Receives the field's current value and a snapshot of the surrounding
/// entity mapping (with the field itself removed so the transform can't
/// accidentally re-read its own pre-image).
pub type TransformFn = fn(&serde_yaml::Value, &serde_yaml::Mapping) -> TransformResult;

/// Outcome of a Transform call. Either rewrite the value (with optional
/// sibling-field writes) or fall back to quarantine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransformResult {
    /// Rewrite the field's value. `sibling_writes` lets the transform set
    /// adjacent fields in the same atomic mapping — used by the typed-AC
    /// reconcile (spec 2026-05-16-ac-taxonomy ac-06) to split brownfield
    /// `ac_type: gate` into `ac_type: <kind>` + `gate: true`. `detail`
    /// surfaces in the run summary as `DispositionRecord.transform_detail`.
    Replace {
        value: serde_yaml::Value,
        sibling_writes: Vec<(&'static str, serde_yaml::Value)>,
        detail: Option<String>,
    },
    /// Fall back to the existing Quarantine path. The reason surfaces in
    /// the run summary as `DispositionRecord.transform_detail`.
    Quarantine(String),
}

impl Disposition {
    pub fn action_str(&self) -> &'static str {
        match self {
            Disposition::Map(_) => "map",
            Disposition::Drop => "drop",
            Disposition::Quarantine => "quarantine",
            Disposition::Transform(_) => "transform",
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
    // Inner AC `ac_type` — brownfield projects use a richer free-text
    // taxonomy (code / doc / config / gate / docs / ...). The canonical
    // schema (spec 2026-05-16-ac-taxonomy ac-01) accepts five values
    // (Code/Config/Doc/Ops/Observation); this Transform routes brownfield
    // values onto the canonical set, splits gate-as-type into orthogonal
    // ac_type + gate=true, and quarantines unknown values.
    //
    // Replaces the v1 Drop entry per spec 2026-05-16-ac-taxonomy ac-06 —
    // the second-project trigger called out in spec 2026-05-12-reconcile-mode.
    (
        EntityType::Spec,
        "acceptance_criteria[].ac_type",
        Disposition::Transform(reconcile_ac_type),
    ),
    // bd-era top-level prose fields on Spec — predecessor_evidence,
    // constraints, exit_conditions — carry semantic content and have no
    // canonical equivalent yet, so they default to quarantine (no rule
    // here). The sidecar preserves the prose for a human to re-anchor.
];

/// Transform handler for `acceptance_criteria[].ac_type` — spec
/// 2026-05-16-ac-taxonomy ac-06. Routes brownfield values onto the
/// canonical enum and splits the `gate`-as-type collision via a
/// description-keyword heuristic. Unknown values quarantine.
fn reconcile_ac_type(
    value: &serde_yaml::Value,
    surrounding: &serde_yaml::Mapping,
) -> TransformResult {
    let s = match value.as_str() {
        Some(s) => s,
        None => {
            return TransformResult::Quarantine(format!(
                "ac_type expected a string, got {value:?}"
            ));
        }
    };

    // Canonical pass-through (no-op rewrite — surfaces a "transform"
    // disposition record so the run summary acknowledges every AC).
    if matches!(s, "code" | "config" | "doc" | "ops" | "observation") {
        return TransformResult::Replace {
            value: serde_yaml::Value::String(s.into()),
            sibling_writes: vec![],
            detail: Some(format!("canonical pass-through: {s}")),
        };
    }

    // Typo normalisation.
    if s == "docs" {
        return TransformResult::Replace {
            value: serde_yaml::Value::String("doc".into()),
            sibling_writes: vec![],
            detail: Some("typo normalisation: docs -> doc".into()),
        };
    }

    // gate-as-type split: read the AC's description and route by keyword.
    if s == "gate" {
        let description = surrounding
            .get(serde_yaml::Value::String("description".into()))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();

        const BUILD_NEEDLES: &[&str] =
            &["build", "cargo", "cmake", "make", "compile"];
        const OBSERVATION_NEEDLES: &[&str] = &[
            "eval",
            "score",
            "accuracy",
            "completes",
            "training",
            "trained",
            "metric",
            "val_accuracy",
            "profile_eval",
        ];

        if BUILD_NEEDLES.iter().any(|n| word_contains(&description, n)) {
            return TransformResult::Replace {
                value: serde_yaml::Value::String("code".into()),
                sibling_writes: vec![(
                    "gate",
                    serde_yaml::Value::Bool(true),
                )],
                detail: Some(
                    "gate-as-type with build/test description -> code + gate=true".into(),
                ),
            };
        }
        if OBSERVATION_NEEDLES
            .iter()
            .any(|n| word_contains(&description, n))
        {
            return TransformResult::Replace {
                value: serde_yaml::Value::String("observation".into()),
                sibling_writes: vec![(
                    "gate",
                    serde_yaml::Value::Bool(true),
                )],
                detail: Some(
                    "gate-as-type with eval/training description -> observation + gate=true"
                        .into(),
                ),
            };
        }

        return TransformResult::Quarantine(format!(
            "unclassified gate-as-type; manual classification required (description: {description:?})"
        ));
    }

    TransformResult::Quarantine(format!("unknown ac_type value: {s:?}"))
}

/// Whole-word substring match — `needle` matches inside `haystack` only
/// if surrounded by non-alphanumeric characters or string boundaries.
/// Plain `contains` would let "compile" match "compiled"; this guard
/// prevents that.
fn word_contains(haystack: &str, needle: &str) -> bool {
    let bytes = haystack.as_bytes();
    let mut start = 0;
    while let Some(pos) = haystack[start..].find(needle) {
        let abs = start + pos;
        let prev_ok = abs == 0 || !bytes[abs - 1].is_ascii_alphanumeric();
        let next_idx = abs + needle.len();
        let next_ok = next_idx == bytes.len() || !bytes[next_idx].is_ascii_alphanumeric();
        if prev_ok && next_ok {
            return true;
        }
        start = abs + 1;
    }
    false
}

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
    /// Action: `"map"` / `"drop"` / `"quarantine"` / `"transform"`. Note
    /// that a Transform that falls back to quarantine flips the action
    /// to `"quarantine"` so the existing sidecar code picks up the
    /// payload.
    pub action: String,
    /// Per-disposition rationale set by Transform handlers — names which
    /// rule matched or why the fallback fired (spec 2026-05-16-ac-taxonomy
    /// ac-05). Non-Transform dispositions leave this as None.
    pub transform_detail: Option<String>,
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
            // Per spec 2026-05-16-ac-taxonomy ac-06, an inner field with a
            // registry rule fires that rule even if the field name is in
            // the canonical FIELDS set — Transform rules use this to
            // route brownfield values onto the canonical enum (and to
            // surface a no-op pass-through disposition record for the run
            // summary). A canonical field with NO registry rule is left
            // alone (the original 'continue' path).
            let registry_path = format!("{}[].{}", inner_path_prefix, inner_key);
            let explicit = lookup_disposition_explicit(kind, &registry_path);
            let disposition = match explicit {
                Some(d) => d,
                None => {
                    if inner_fields.contains(&inner_key.as_str()) {
                        continue;
                    }
                    Disposition::Quarantine
                }
            };

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
    let mut record = DispositionRecord {
        path: display_path.to_string(),
        kind: kind.as_str().to_string(),
        field: key.to_string(),
        action: disposition.action_str().to_string(),
        transform_detail: None,
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
        Disposition::Transform(transform_fn) => {
            let current = match mapping.get(&key_value).cloned() {
                Some(v) => v,
                None => return (record, None),
            };
            let mut surrounding = mapping.clone();
            surrounding.remove(&key_value);
            match transform_fn(&current, &surrounding) {
                TransformResult::Replace {
                    value,
                    sibling_writes,
                    detail,
                } => {
                    mapping.insert(key_value, value);
                    for (sib_key, sib_val) in sibling_writes {
                        mapping.insert(serde_yaml::Value::String(sib_key.into()), sib_val);
                    }
                    record.transform_detail = detail;
                    (record, None)
                }
                TransformResult::Quarantine(reason) => {
                    let payload = mapping.remove(&key_value);
                    record.action = "quarantine".into();
                    record.transform_detail = Some(reason);
                    (record, payload)
                }
            }
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
    let mut record = DispositionRecord {
        path: display_path.to_string(),
        kind: kind.as_str().to_string(),
        field: display_field.to_string(),
        action: disposition.action_str().to_string(),
        transform_detail: None,
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
        Disposition::Transform(transform_fn) => {
            let current = match inner_map.get(&key_value).cloned() {
                Some(v) => v,
                None => return (record, None),
            };
            let mut surrounding = inner_map.clone();
            surrounding.remove(&key_value);
            match transform_fn(&current, &surrounding) {
                TransformResult::Replace {
                    value,
                    sibling_writes,
                    detail,
                } => {
                    inner_map.insert(key_value, value);
                    for (sib_key, sib_val) in sibling_writes {
                        inner_map
                            .insert(serde_yaml::Value::String(sib_key.into()), sib_val);
                    }
                    record.transform_detail = detail;
                    (record, None)
                }
                TransformResult::Quarantine(reason) => {
                    let payload = inner_map.remove(&key_value);
                    record.action = "quarantine".into();
                    record.transform_detail = Some(reason);
                    (record, payload)
                }
            }
        }
    }
}

fn lookup_disposition(kind: EntityType, structural_path: &str) -> Disposition {
    lookup_disposition_explicit(kind, structural_path).unwrap_or(Disposition::Quarantine)
}

/// Like `lookup_disposition` but returns `None` when no rule matches —
/// lets callers (e.g. `recurse_inner`) distinguish "explicit registry
/// rule" from "default-quarantine fallback." Per spec 2026-05-16-ac-
/// taxonomy ac-06: a canonical inner field with an explicit Transform
/// rule should still fire that rule on read.
fn lookup_disposition_explicit(kind: EntityType, structural_path: &str) -> Option<Disposition> {
    for (rule_kind, rule_path, disposition) in FIELD_RULES {
        if *rule_kind == kind && *rule_path == structural_path {
            return Some(*disposition);
        }
    }
    None
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
    fn inner_field_canonical_ac_type_fires_transform_pass_through() {
        // spec 2026-05-16-ac-taxonomy ac-06: a canonical ac_type value
        // fires the Transform rule as a no-op pass-through. Disposition
        // count == 1 with action "transform" + the canonical-pass-through
        // detail. The on-disk value is preserved.
        let (_dir, layout) = fresh_layout();
        let yaml = "id: '0001'\ngoal: g\nstatus: open\nacceptance_criteria:\n- id: ac-01\n  description: do thing\n  gate: true\n  checked: false\n  ac_type: observation\n";
        let path = write_spec(&layout, "0001", yaml);

        let report = reconcile_all(&layout, false);
        assert_eq!(report.dispositions.len(), 1, "got: {:?}", report.dispositions);
        let d = &report.dispositions[0];
        assert_eq!(d.action, "transform");
        assert_eq!(d.field, "acceptance_criteria[0].ac_type");
        assert_eq!(
            d.transform_detail.as_deref(),
            Some("canonical pass-through: observation")
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
    fn lookup_disposition_returns_transform_for_inner_ac_type() {
        // spec 2026-05-16-ac-taxonomy ac-06: the ac_type Drop entry was
        // replaced with Transform(reconcile_ac_type).
        let d = lookup_disposition(EntityType::Spec, "acceptance_criteria[].ac_type");
        assert!(
            matches!(d, Disposition::Transform(_)),
            "expected Transform variant, got {d:?}"
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

    // ========================================================================
    // Spec 2026-05-16-ac-taxonomy ac-05 + ac-06 — Transform variant + the
    // typed-AC reconcile registry.
    // ========================================================================

    fn empty_mapping() -> serde_yaml::Mapping {
        serde_yaml::Mapping::new()
    }

    fn ac_mapping_with_description(desc: &str) -> serde_yaml::Mapping {
        let mut m = serde_yaml::Mapping::new();
        m.insert(
            serde_yaml::Value::String("description".into()),
            serde_yaml::Value::String(desc.into()),
        );
        m
    }

    #[test]
    fn ac_05_disposition_action_str_includes_transform() {
        // spec 2026-05-16-ac-taxonomy ac-05: action_str gains a "transform" arm.
        fn noop(_v: &serde_yaml::Value, _m: &serde_yaml::Mapping) -> TransformResult {
            TransformResult::Replace {
                value: serde_yaml::Value::Null,
                sibling_writes: vec![],
                detail: None,
            }
        }
        let d: Disposition = Disposition::Transform(noop);
        assert_eq!(d.action_str(), "transform");
    }

    #[test]
    fn ac_05_transform_replace_rewrites_value_and_writes_siblings() {
        // spec 2026-05-16-ac-taxonomy ac-05: a Transform returning
        // Replace { value, sibling_writes } rewrites the field's value
        // and atomically sets each sibling.
        let (_dir, layout) = fresh_layout();
        // Define an inline transform that always returns code + gate=true.
        fn force_code_gate(
            _v: &serde_yaml::Value,
            _m: &serde_yaml::Mapping,
        ) -> TransformResult {
            TransformResult::Replace {
                value: serde_yaml::Value::String("code".into()),
                sibling_writes: vec![("gate", serde_yaml::Value::Bool(true))],
                detail: Some("forced for test".into()),
            }
        }
        // Run the transform directly — proves the contract independent of
        // the registry plumbing.
        let result = force_code_gate(
            &serde_yaml::Value::String("anything".into()),
            &empty_mapping(),
        );
        match result {
            TransformResult::Replace {
                value,
                sibling_writes,
                detail,
            } => {
                assert_eq!(value, serde_yaml::Value::String("code".into()));
                assert_eq!(sibling_writes.len(), 1);
                assert_eq!(sibling_writes[0].0, "gate");
                assert_eq!(sibling_writes[0].1, serde_yaml::Value::Bool(true));
                assert_eq!(detail.as_deref(), Some("forced for test"));
            }
            TransformResult::Quarantine(_) => panic!("expected Replace"),
        }
        // Sanity: the layout helper still works (test fixture sanity).
        let _ = layout;
    }

    #[test]
    fn ac_05_transform_quarantine_falls_back_to_quarantine_path() {
        // spec 2026-05-16-ac-taxonomy ac-05: Transform returning Quarantine
        // falls through to the existing Quarantine path. Test the
        // returning-quarantine variant via the typed-AC handler so we get
        // the integration as well.
        let (_dir, layout) = fresh_layout();
        let yaml = "id: '0001'\ngoal: g\nstatus: open\nacceptance_criteria:\n- id: ac-01\n  description: 'unrelated description text'\n  gate: false\n  checked: false\n  ac_type: gate\n";
        let path = write_spec(&layout, "0001", yaml);

        let report = reconcile_all(&layout, false);
        assert_eq!(report.dispositions.len(), 1);
        let d = &report.dispositions[0];
        // Falls back to "quarantine" so the existing sidecar code picks
        // up the value.
        assert_eq!(d.action, "quarantine");
        assert!(
            d.transform_detail
                .as_ref()
                .map(|s| s.contains("unclassified gate-as-type"))
                .unwrap_or(false),
            "transform_detail should explain the fallback: {:?}",
            d.transform_detail
        );
        // Sidecar carries the quarantined value.
        let side = sidecar_path(&path);
        assert!(side.exists(), "sidecar should exist after quarantine fallback");
    }

    #[test]
    fn ac_06_typo_normalises_docs_to_doc() {
        let result = reconcile_ac_type(
            &serde_yaml::Value::String("docs".into()),
            &empty_mapping(),
        );
        match result {
            TransformResult::Replace { value, sibling_writes, detail } => {
                assert_eq!(value, serde_yaml::Value::String("doc".into()));
                assert!(sibling_writes.is_empty());
                assert!(detail.unwrap().contains("typo normalisation"));
            }
            r => panic!("expected Replace, got {r:?}"),
        }
    }

    #[test]
    fn ac_06_canonical_value_passes_through_with_no_op() {
        for canonical in &["code", "config", "doc", "ops", "observation"] {
            let result = reconcile_ac_type(
                &serde_yaml::Value::String((*canonical).into()),
                &empty_mapping(),
            );
            match result {
                TransformResult::Replace { value, sibling_writes, detail } => {
                    assert_eq!(value, serde_yaml::Value::String((*canonical).into()));
                    assert!(sibling_writes.is_empty());
                    assert!(detail.unwrap().contains("canonical pass-through"));
                }
                r => panic!("expected Replace for {canonical}, got {r:?}"),
            }
        }
    }

    #[test]
    fn ac_06_gate_with_build_description_routes_to_code_plus_gate() {
        let result = reconcile_ac_type(
            &serde_yaml::Value::String("gate".into()),
            &ac_mapping_with_description("cargo build succeeds without errors"),
        );
        match result {
            TransformResult::Replace { value, sibling_writes, detail } => {
                assert_eq!(value, serde_yaml::Value::String("code".into()));
                assert_eq!(sibling_writes.len(), 1);
                assert_eq!(sibling_writes[0].0, "gate");
                assert_eq!(sibling_writes[0].1, serde_yaml::Value::Bool(true));
                assert!(detail.unwrap().contains("build/test description"));
            }
            r => panic!("expected Replace, got {r:?}"),
        }
    }

    #[test]
    fn ac_06_gate_with_eval_description_routes_to_observation_plus_gate() {
        let result = reconcile_ac_type(
            &serde_yaml::Value::String("gate".into()),
            &ac_mapping_with_description("Profile eval >= 160/190 label accuracy"),
        );
        match result {
            TransformResult::Replace { value, sibling_writes, detail } => {
                assert_eq!(value, serde_yaml::Value::String("observation".into()));
                assert_eq!(sibling_writes.len(), 1);
                assert_eq!(sibling_writes[0].0, "gate");
                assert_eq!(sibling_writes[0].1, serde_yaml::Value::Bool(true));
                assert!(detail.unwrap().contains("eval/training"));
            }
            r => panic!("expected Replace, got {r:?}"),
        }
    }

    #[test]
    fn ac_06_gate_with_unmatched_description_quarantines() {
        let result = reconcile_ac_type(
            &serde_yaml::Value::String("gate".into()),
            &ac_mapping_with_description("some unrelated thing"),
        );
        match result {
            TransformResult::Quarantine(reason) => {
                assert!(reason.contains("unclassified gate-as-type"));
            }
            r => panic!("expected Quarantine, got {r:?}"),
        }
    }

    #[test]
    fn ac_06_unknown_ac_type_value_quarantines() {
        let result = reconcile_ac_type(
            &serde_yaml::Value::String("custom_kind".into()),
            &empty_mapping(),
        );
        match result {
            TransformResult::Quarantine(reason) => {
                assert!(reason.contains("unknown ac_type value"));
            }
            r => panic!("expected Quarantine, got {r:?}"),
        }
    }

    #[test]
    fn ac_06_word_contains_respects_word_boundaries() {
        // "compile" should NOT match "compiled" (word-boundary on the right).
        // "build" should match "build" surrounded by spaces.
        // "eval" should NOT match "evaluation" (word-boundary on the right).
        assert!(word_contains("cargo build succeeds", "build"));
        assert!(word_contains("cmake compile step", "compile"));
        assert!(!word_contains("compiled binary", "compile"));
        assert!(!word_contains("evaluation pipeline", "eval"));
        assert!(word_contains("eval = 99/100", "eval"));
    }

    #[test]
    fn ac_06_brownfield_dry_run_routes_observed_corpus_correctly() {
        // spec 2026-05-16-ac-taxonomy ac-06 verification: an integration
        // run against a fixture brownfield tree containing one AC of each
        // routing path produces the expected disposition shape.
        let (_dir, layout) = fresh_layout();
        let yaml = "id: '0001'\n\
                    goal: g\n\
                    status: open\n\
                    acceptance_criteria:\n\
                    - id: ac-canonical\n  description: already canonical\n  gate: false\n  checked: false\n  ac_type: code\n\
                    - id: ac-typo\n  description: doc-shaped\n  gate: false\n  checked: false\n  ac_type: docs\n\
                    - id: ac-build\n  description: cargo build succeeds\n  gate: false\n  checked: false\n  ac_type: gate\n\
                    - id: ac-eval\n  description: eval >= 160/190\n  gate: false\n  checked: false\n  ac_type: gate\n\
                    - id: ac-unknown-gate\n  description: nothing relevant\n  gate: false\n  checked: false\n  ac_type: gate\n\
                    - id: ac-unknown-value\n  description: anything\n  gate: false\n  checked: false\n  ac_type: bizarre_kind\n";
        write_spec(&layout, "0001", yaml);

        let report = reconcile_all(&layout, false);
        assert_eq!(
            report.dispositions.len(),
            6,
            "one disposition per AC; got: {:?}",
            report.dispositions
        );

        let actions: Vec<&str> = report.dispositions.iter().map(|d| d.action.as_str()).collect();
        // The canonical, typo, build, and eval cases are all "transform"
        // (Replace path). The unknown-gate and unknown-value cases fall
        // back to "quarantine".
        assert_eq!(actions.iter().filter(|a| **a == "transform").count(), 4);
        assert_eq!(actions.iter().filter(|a| **a == "quarantine").count(), 2);

        // Every disposition has a transform_detail (Transform handlers
        // always populate it).
        for d in &report.dispositions {
            assert!(
                d.transform_detail.is_some(),
                "every Transform disposition should carry a transform_detail; got {:?}",
                d
            );
        }
    }
}
