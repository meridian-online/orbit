//! Entity schemas — strongly-typed, `deny_unknown_fields` everywhere.
//!
//! Per ac-01 the parser MUST reject (not silently drop) unknown fields. This is
//! the single annotation that prevents the lossy-parse failure mode where
//! deserialise + reserialise stays byte-identical because the dropped field is
//! absent from both sides.
//!
//! Per ac-04 the schema-version file is a substrate-written entity classified
//! under `values.enforcement.substrate_written` — schema drift in the version
//! file silently breaks every migration, so it gets the same strict treatment.
//!
//! Layout on disk:
//! - `.orbit/schema-version`           — single-line entity, opaque to git
//! - `.orbit/specs/<id>.yaml`          — Spec (substrate-written)
//! - `.orbit/cards/<slug>.yaml`        — Card (human-written; CI validated)
//! - `.orbit/choices/<slug>.yaml`      — Choice (human-written; CI validated)
//! - `.orbit/memories/<slug>.yaml`     — Memory (substrate-written)
//! - `.orbit/specs/<id>.tasks.jsonl`   — Task event stream (append-only)
//!
//! Tasks are intentionally append-only JSONL — they are not round-trippable as
//! a unit and are excluded from the CI round-trip gate per ac-16.

use serde::{Deserialize, Serialize};

// ============================================================================
// schema-version
// ============================================================================

/// The on-disk schema version. Read first by the migration runner on every
/// invocation (per `values.enforcement` rationale).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaVersion {
    /// Major.minor schema identifier, e.g. `"0.1"`.
    pub version: String,
    /// Human-readable note attached to the version (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

// ============================================================================
// Spec
// ============================================================================

/// A discrete unit of work with numbered acceptance criteria. Substrate-written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    /// Slug-style identifier (e.g. `"2026-05-07-orbit-state-v0.1"`).
    pub id: String,
    /// One-sentence statement of what shipping this spec achieves.
    pub goal: String,
    /// Cards this spec advances by closure. Empty list is rare but legal.
    #[serde(default)]
    pub cards: Vec<String>,
    /// Status — `open` until close; close requires all child tasks done.
    pub status: SpecStatus,
    /// Free-text labels (`spec`, `experimental`, etc.) — matches bd's label model.
    #[serde(default)]
    pub labels: Vec<String>,
    /// Acceptance criteria, in declaration order. Gate ACs block subsequent ones.
    #[serde(default)]
    pub acceptance_criteria: Vec<AcceptanceCriterion>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpecStatus {
    Open,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceCriterion {
    pub id: String,
    pub description: String,
    #[serde(default)]
    pub gate: bool,
    #[serde(default)]
    pub checked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification: Option<String>,
}

// ============================================================================
// Task (append-only event)
// ============================================================================

/// One event in a task's append-only JSONL stream. State is reconstructed by
/// reducing events for a `task_id` and taking the last one (per ac-07).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskEvent {
    /// Logical task identifier (stable across events).
    pub task_id: String,
    /// The spec this task belongs to.
    pub spec_id: String,
    /// What happened at this event.
    pub event: TaskEventKind,
    /// Free-text body for the event (e.g. open description, update note).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// Free-text labels (e.g. `skill-author`).
    #[serde(default)]
    pub labels: Vec<String>,
    /// ISO-8601 timestamp, written by the substrate at event-append time.
    pub timestamp: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskEventKind {
    Open,
    Claim,
    Update,
    Done,
}

// ============================================================================
// Spec note (append-only event)
// ============================================================================

/// One note appended to a spec via `spec.note`. Lives in the same
/// append-only family as [`TaskEvent`] — JSONL stream, ordered by
/// position-in-file, never rewritten in place.
///
/// Layout: `.orbit/specs/<spec_id>.notes.jsonl`. Excluded from the CI
/// round-trip gate (ac-16) for the same reason tasks are: append-only
/// streams aren't round-trippable as a unit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteEvent {
    /// Spec this note attaches to.
    pub spec_id: String,
    /// Free-text body. Multi-line strings are escaped per JSON rules.
    pub body: String,
    /// Free-text labels (e.g. `migrated-from-bd`).
    #[serde(default)]
    pub labels: Vec<String>,
    /// ISO-8601 / RFC 3339 timestamp written by the substrate at append time.
    /// Migration tools may pre-supply this when porting historical notes.
    pub timestamp: String,
}

// ============================================================================
// Card (human-written; CI-validated)
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Card {
    /// Full slug (e.g. `0008-consolidated-orbit-artefact-folder`) — must equal
    /// the filename minus `.yaml`. Optional for backwards compatibility with
    /// pre-choice-0022 cards; the canonical writer fills it from the filename
    /// on the next canonicalise pass.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub feature: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub as_a: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub i_want: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub so_that: Option<String>,
    pub goal: String,
    pub maturity: CardMaturity,
    #[serde(default)]
    pub scenarios: Vec<Scenario>,
    /// Spec paths advanced by this card. Substrate appends here on `spec.close`.
    #[serde(default)]
    pub specs: Vec<String>,
    #[serde(default)]
    pub relations: Vec<Relation>,
    #[serde(default)]
    pub references: Vec<String>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardMaturity {
    Planned,
    Emerging,
    Established,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub name: String,
    pub given: String,
    pub when: String,
    pub then: String,
    #[serde(default)]
    pub gate: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Relation {
    pub card: String,
    #[serde(rename = "type")]
    pub kind: RelationKind,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RelationKind {
    DependsOn,
    Feeds,
    Supersedes,
    SupersededBy,
}

// ============================================================================
// Choice (human-written; CI-validated)
// ============================================================================

/// A choice (architectural decision in MADR shape). Human-written; the CI
/// round-trip gate (ac-16) is the format-integrity enforcement mechanism.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    pub id: String,
    pub title: String,
    pub status: ChoiceStatus,
    pub date_created: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_modified: Option<String>,
    /// MADR body — multi-line prose. The choice fixture suite (ac-01) covers
    /// the round-trip edge cases for this field.
    pub body: String,
    #[serde(default)]
    pub references: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChoiceStatus {
    Proposed,
    Accepted,
    Rejected,
    Deprecated,
    Superseded,
}

// ============================================================================
// Memory
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Memory {
    pub key: String,
    pub body: String,
    pub timestamp: String,
    #[serde(default)]
    pub labels: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_version_rejects_unknown_field() {
        // ac-01 verification: extra unknown field MUST fail parse.
        let yaml = "version: '0.1'\nnote: bootstrap\nunknown_field: oops\n";
        let err = serde_yaml::from_str::<SchemaVersion>(yaml).unwrap_err();
        assert!(err.to_string().contains("unknown"));
    }

    #[test]
    fn spec_rejects_unknown_field() {
        let yaml = r#"
id: '0001'
goal: build it
status: open
unknown_field: oops
"#;
        let err = serde_yaml::from_str::<Spec>(yaml).unwrap_err();
        assert!(err.to_string().contains("unknown"));
    }

    #[test]
    fn card_rejects_unknown_field() {
        let yaml = r#"
feature: x
goal: y
maturity: planned
unknown_field: oops
"#;
        let err = serde_yaml::from_str::<Card>(yaml).unwrap_err();
        assert!(err.to_string().contains("unknown"));
    }

    #[test]
    fn choice_rejects_unknown_field() {
        let yaml = r#"
id: '0001'
title: t
status: accepted
date_created: '2026-05-07'
body: hello
unknown_field: oops
"#;
        let err = serde_yaml::from_str::<Choice>(yaml).unwrap_err();
        assert!(err.to_string().contains("unknown"));
    }

    #[test]
    fn memory_rejects_unknown_field() {
        let yaml = r#"
key: k
body: b
timestamp: '2026-05-07T00:00:00Z'
unknown_field: oops
"#;
        let err = serde_yaml::from_str::<Memory>(yaml).unwrap_err();
        assert!(err.to_string().contains("unknown"));
    }

    #[test]
    fn task_event_rejects_unknown_field() {
        let json = r#"{"task_id":"t1","spec_id":"s1","event":"open","timestamp":"2026-05-07T00:00:00Z","unknown_field":"oops"}"#;
        let err = serde_json::from_str::<TaskEvent>(json).unwrap_err();
        assert!(err.to_string().contains("unknown"));
    }

    #[test]
    fn spec_round_trip_is_lossless() {
        let spec = Spec {
            id: "0001".into(),
            goal: "do the thing".into(),
            cards: vec!["0020-orbit-state".into()],
            status: SpecStatus::Open,
            labels: vec!["spec".into()],
            acceptance_criteria: vec![AcceptanceCriterion {
                id: "ac-01".into(),
                description: "first".into(),
                gate: true,
                checked: false,
                verification: Some("v1".into()),
            }],
        };
        let yaml = serde_yaml::to_string(&spec).unwrap();
        let parsed: Spec = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(spec, parsed);
    }
}
