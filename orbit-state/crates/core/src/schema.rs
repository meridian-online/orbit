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
//! - `.orbit/schema-version`                       — single-line entity, opaque to git
//! - `.orbit/specs/<id>.yaml`                      — Spec (substrate-written)
//! - `.orbit/cards/<slug>.yaml`                    — Card (human-written; CI validated)
//! - `.orbit/choices/<slug>.yaml`                  — Choice (human-written; CI validated)
//! - `.orbit/memories/<slug>.yaml`                 — Memory (substrate-written)
//! - `.orbit/sessions/<session-id>.yaml`           — Session (substrate-written)
//! - `.orbit/specs/<id>.tasks.jsonl`               — Task event stream (append-only)
//! - `.orbit/specs/<id>.notes.jsonl`               — Note event stream (append-only)
//! - `.orbit/skills/<skill_id>.invocations.jsonl`  — Skill invocation stream (append-only)
//!
//! Tasks, notes, and skill invocations are intentionally append-only JSONL —
//! they are not round-trippable as a unit and are excluded from the CI
//! round-trip gate per ac-16.

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

impl Spec {
    /// Canonical top-level field set for the audit-drift verb (see
    /// `orbit audit drift`). Kept in lockstep with the struct via the
    /// `spec_fields_matches_struct` unit test in this module — adding a
    /// new field to `Spec` requires extending both the const and the
    /// test fixture, so the audit's allow-set cannot silently drift from
    /// the canonical schema.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "goal",
        "cards",
        "status",
        "labels",
        "acceptance_criteria",
    ];
}

impl Card {
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "feature",
        "as_a",
        "i_want",
        "so_that",
        "goal",
        "maturity",
        "scenarios",
        "specs",
        "relations",
        "references",
        "notes",
    ];
}

impl Choice {
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "title",
        "status",
        "date_created",
        "date_modified",
        "body",
        "references",
    ];
}

impl Memory {
    pub const FIELDS: &'static [&'static str] = &["key", "body", "timestamp", "labels"];
}

impl SkillInvocation {
    pub const FIELDS: &'static [&'static str] =
        &["skill_id", "session_id", "outcome", "correction", "timestamp"];
}

impl Session {
    pub const FIELDS: &'static [&'static str] =
        &["id", "started_at", "ended_at", "distillate", "labels"];
}

impl AcceptanceCriterion {
    pub const FIELDS: &'static [&'static str] =
        &["id", "description", "gate", "checked", "verification", "ac_type"];
}

impl Scenario {
    pub const FIELDS: &'static [&'static str] = &["name", "given", "when", "then", "gate"];
}

impl Relation {
    pub const FIELDS: &'static [&'static str] = &["card", "type", "reason"];
}

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
    /// The kind of evidence that closes this AC. Drives spec.close's
    /// two-band behaviour (Code/Config/Doc block close when unchecked;
    /// Ops/Observation legitimately defer) per spec
    /// 2026-05-16-ac-taxonomy.
    ///
    /// `#[serde(default)]` keeps untyped legacy corpora parseable — they
    /// deserialise as `AcType::Code` (matches the implicit assumption
    /// every untyped AC carried before this field shipped).
    /// `skip_serializing_if = "AcType::is_code"` preserves byte-identical
    /// canonical output for the dominant Code case so the migration
    /// touches only ACs that need a non-default value.
    #[serde(default, skip_serializing_if = "AcType::is_code")]
    pub ac_type: AcType,
}

/// The kind of evidence that closes an AC. Drives spec.close's two-band
/// behaviour: `Code`, `Config`, `Doc` block close when unchecked;
/// `Ops`, `Observation` are deferrable (the spec is allowed to close with
/// them open). Per spec 2026-05-16-ac-taxonomy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcType {
    /// Closes on a passing test, referenced commit, or functional
    /// artefact. The default — matches the implicit assumption every
    /// untyped AC carried before this field shipped.
    Code,
    /// Closes on a config or external-system-state change verifiable by
    /// grep, file inspection, or external query.
    Config,
    /// Closes on a written artefact (CLAUDE.md edit, card text, memo,
    /// MADR).
    Doc,
    /// Closes on operator action with a captured log line, signoff, or
    /// dashboard check. Legitimately deferred at spec.close.
    Ops,
    /// Closes on a dated window of empirical measurement (post-cutover
    /// soak, eval-run output, training-completes-and-produces-metrics).
    /// Legitimately deferred at spec.close.
    Observation,
}

impl Default for AcType {
    fn default() -> Self {
        AcType::Code
    }
}

impl AcType {
    /// True when an unchecked AC of this kind blocks `spec.close`.
    /// `Code`, `Config`, `Doc` close on artefacts that exist at commit
    /// time (a passing test, a file diff, written prose); leaving them
    /// unchecked is premature closure. `Ops`, `Observation` close on
    /// events that happen after the spec's other work lands (an
    /// operator signoff, a dated metric window); the spec is allowed to
    /// close with them open and they appear in the deferrable-open
    /// list returned by `spec.close`.
    pub fn blocks_close(&self) -> bool {
        matches!(self, Self::Code | Self::Config | Self::Doc)
    }

    /// Predicate used by `#[serde(skip_serializing_if = ...)]` on
    /// `AcceptanceCriterion::ac_type` so the dominant Code case stays
    /// byte-identical to today's canonical output.
    pub fn is_code(&self) -> bool {
        matches!(self, Self::Code)
    }
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

// ============================================================================
// Skill invocation (append-only event)
// ============================================================================

/// One row in a skill's append-only invocation log. Recurrence detection
/// (per spec 2026-05-15-agent-learning-loop ac-04) reduces the file by
/// counting rows per [`InvocationOutcome`].
///
/// Layout: `.orbit/skills/<skill_id>.invocations.jsonl`. Excluded from
/// the CI round-trip gate for the same reason tasks and notes are:
/// append-only streams aren't round-trippable as a unit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillInvocation {
    /// Skill slug — matches the directory name under `plugins/orb/skills/`.
    pub skill_id: String,
    /// Session this invocation belongs to (sourced via `read_session_id`).
    pub session_id: String,
    /// What happened when the agent invoked the skill.
    pub outcome: InvocationOutcome,
    /// Free-text record of what went wrong (or what was corrected). Drives
    /// the SKILL.md edit decision once the recurrence threshold is met —
    /// the count tells the agent *whether*, the corrections tell it *what*.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correction: Option<String>,
    /// ISO-8601 / RFC 3339 timestamp written by the substrate at append time.
    pub timestamp: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InvocationOutcome {
    /// Skill ran end-to-end and produced the intended result.
    Worked,
    /// Skill ran but the result needed correction.
    Partial,
    /// Skill was invoked in a context the SKILL.md does not cover.
    DidntApply,
    /// Skill produced a wrong result.
    Incorrect,
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

// ============================================================================
// Session
// ============================================================================

/// A summary record for one agent session. Substrate-written by the
/// `session.distill` verb at session end. Idempotent on `id`: re-running
/// distill on the same session overwrites the same file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Session {
    /// Session identifier — typically a UUIDv4 generated by `session.start`
    /// and persisted to `.orbit/.session-id` for the duration of the session.
    pub id: String,
    /// ISO-8601 / RFC 3339 timestamp of the first `session.distill` call.
    /// Preserved across subsequent calls for the same `id`.
    pub started_at: String,
    /// ISO-8601 / RFC 3339 timestamp of the most recent `session.distill`
    /// call. None until distill is first invoked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    /// The agent's end-of-session reflection — free-text markdown.
    pub distillate: String,
    /// Free-text labels for prime-relevance and search.
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

    /// Helper: extract sorted top-level keys from a serde_yaml::Value mapping.
    fn top_level_keys(value: &serde_yaml::Value) -> Vec<String> {
        let mut out: Vec<String> = value
            .as_mapping()
            .expect("expected mapping")
            .iter()
            .filter_map(|(k, _)| k.as_str().map(String::from))
            .collect();
        out.sort();
        out
    }

    #[test]
    fn spec_fields_matches_struct() {
        // ac-05 verification: Spec::FIELDS must equal the struct's serde
        // top-level field set. The test populates a fully-populated Spec
        // (every Option=Some, every Vec non-empty) so skip_serializing_if
        // doesn't drop fields, serialises to YAML, and compares the key
        // set against the constant. Adding a new field to Spec requires
        // extending this fixture (the struct literal won't compile
        // otherwise) AND the FIELDS const — drift between the two trips
        // this assertion.
        let spec = Spec {
            id: "id".into(),
            goal: "goal".into(),
            cards: vec!["c".into()],
            status: SpecStatus::Open,
            labels: vec!["l".into()],
            acceptance_criteria: vec![AcceptanceCriterion {
                id: "ac-01".into(),
                description: "d".into(),
                gate: false,
                checked: false,
                verification: Some("v".into()),
                ac_type: AcType::Observation,
            }],
        };
        let value = serde_yaml::to_value(&spec).unwrap();
        let got = top_level_keys(&value);
        let mut expected: Vec<String> = Spec::FIELDS.iter().map(|s| s.to_string()).collect();
        expected.sort();
        assert_eq!(got, expected, "Spec::FIELDS drifted from struct");
    }

    #[test]
    fn card_fields_matches_struct() {
        let card = Card {
            id: Some("0001-x".into()),
            feature: "f".into(),
            as_a: Some("a".into()),
            i_want: Some("i".into()),
            so_that: Some("s".into()),
            goal: "g".into(),
            maturity: CardMaturity::Planned,
            scenarios: vec![Scenario {
                name: "n".into(),
                given: "g".into(),
                when: "w".into(),
                then: "t".into(),
                gate: false,
            }],
            specs: vec!["sp".into()],
            relations: vec![Relation {
                card: "c".into(),
                kind: RelationKind::Feeds,
                reason: "r".into(),
            }],
            references: vec!["r".into()],
            notes: vec!["n".into()],
        };
        let value = serde_yaml::to_value(&card).unwrap();
        let got = top_level_keys(&value);
        let mut expected: Vec<String> = Card::FIELDS.iter().map(|s| s.to_string()).collect();
        expected.sort();
        assert_eq!(got, expected, "Card::FIELDS drifted from struct");
    }

    #[test]
    fn choice_fields_matches_struct() {
        let choice = Choice {
            id: "0001".into(),
            title: "t".into(),
            status: ChoiceStatus::Accepted,
            date_created: "2026-05-12".into(),
            date_modified: Some("2026-05-12".into()),
            body: "b".into(),
            references: vec!["r".into()],
        };
        let value = serde_yaml::to_value(&choice).unwrap();
        let got = top_level_keys(&value);
        let mut expected: Vec<String> = Choice::FIELDS.iter().map(|s| s.to_string()).collect();
        expected.sort();
        assert_eq!(got, expected, "Choice::FIELDS drifted from struct");
    }

    #[test]
    fn memory_fields_matches_struct() {
        let memory = Memory {
            key: "k".into(),
            body: "b".into(),
            timestamp: "2026-05-12T00:00:00Z".into(),
            labels: vec!["l".into()],
        };
        let value = serde_yaml::to_value(&memory).unwrap();
        let got = top_level_keys(&value);
        let mut expected: Vec<String> = Memory::FIELDS.iter().map(|s| s.to_string()).collect();
        expected.sort();
        assert_eq!(got, expected, "Memory::FIELDS drifted from struct");
    }

    #[test]
    fn session_fields_matches_struct() {
        // spec 2026-05-15-agent-learning-loop ac-02: Session::FIELDS must
        // equal the struct's serde top-level field set. Fully-populated
        // fixture so skip_serializing_if doesn't drop fields.
        let session = Session {
            id: "5f6b1a7e-7a32-4f6e-9d31-1a2b3c4d5e6f".into(),
            started_at: "2026-05-15T12:00:00Z".into(),
            ended_at: Some("2026-05-15T13:00:00Z".into()),
            distillate: "got the loop running".into(),
            labels: vec!["loop".into()],
        };
        let value = serde_yaml::to_value(&session).unwrap();
        let got = top_level_keys(&value);
        let mut expected: Vec<String> =
            Session::FIELDS.iter().map(|s| s.to_string()).collect();
        expected.sort();
        assert_eq!(got, expected, "Session::FIELDS drifted from struct");
    }

    #[test]
    fn session_rejects_unknown_field() {
        // spec 2026-05-15-agent-learning-loop ac-02: parser MUST reject
        // unknown fields rather than silently dropping them.
        let yaml = r#"
id: 5f6b1a7e-7a32-4f6e-9d31-1a2b3c4d5e6f
started_at: 2026-05-15T12:00:00Z
distillate: hello
unknown_field: oops
"#;
        let err = serde_yaml::from_str::<Session>(yaml).unwrap_err();
        assert!(err.to_string().contains("unknown"));
    }

    #[test]
    fn session_round_trips_byte_identical() {
        // spec 2026-05-15-agent-learning-loop ac-02: round-trip lossless on
        // a populated Session (both started_at and ended_at present).
        let session = Session {
            id: "5f6b1a7e-7a32-4f6e-9d31-1a2b3c4d5e6f".into(),
            started_at: "2026-05-15T12:00:00Z".into(),
            ended_at: Some("2026-05-15T13:00:00Z".into()),
            distillate: "got the loop running\n".into(),
            labels: vec![],
        };
        let yaml = serde_yaml::to_string(&session).unwrap();
        let parsed: Session = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(session, parsed);
    }

    #[test]
    fn session_optional_ended_at_skipped_when_none() {
        // spec 2026-05-15-agent-learning-loop ac-02: ended_at = None must
        // NOT appear as `ended_at: null` on disk — the skip_serializing_if
        // discipline keeps freshly-started session files lean.
        let session = Session {
            id: "5f6b1a7e-7a32-4f6e-9d31-1a2b3c4d5e6f".into(),
            started_at: "2026-05-15T12:00:00Z".into(),
            ended_at: None,
            distillate: "".into(),
            labels: vec![],
        };
        let yaml = serde_yaml::to_string(&session).unwrap();
        assert!(
            !yaml.contains("ended_at"),
            "expected no `ended_at` key when None; got: {yaml}"
        );
    }

    #[test]
    fn skill_invocation_fields_matches_struct() {
        // spec 2026-05-15-agent-learning-loop ac-01: SkillInvocation::FIELDS
        // must equal the struct's serde top-level field set. Fully-populated
        // fixture so skip_serializing_if doesn't drop fields.
        let inv = SkillInvocation {
            skill_id: "card".into(),
            session_id: "5f6b1a7e-7a32-4f6e-9d31-1a2b3c4d5e6f".into(),
            outcome: InvocationOutcome::Worked,
            correction: Some("nudged the wording".into()),
            timestamp: "2026-05-15T12:00:00Z".into(),
        };
        let value = serde_yaml::to_value(&inv).unwrap();
        let got = top_level_keys(&value);
        let mut expected: Vec<String> =
            SkillInvocation::FIELDS.iter().map(|s| s.to_string()).collect();
        expected.sort();
        assert_eq!(got, expected, "SkillInvocation::FIELDS drifted from struct");
    }

    #[test]
    fn skill_invocation_rejects_unknown_field() {
        // spec 2026-05-15-agent-learning-loop ac-01: parser MUST reject
        // unknown fields rather than silently dropping them.
        let yaml = r#"
skill_id: card
session_id: 5f6b1a7e-7a32-4f6e-9d31-1a2b3c4d5e6f
outcome: worked
timestamp: 2026-05-15T12:00:00Z
unknown_field: oops
"#;
        let err = serde_yaml::from_str::<SkillInvocation>(yaml).unwrap_err();
        assert!(err.to_string().contains("unknown"));
    }

    #[test]
    fn invocation_outcome_kebab_case_round_trip() {
        // spec 2026-05-15-agent-learning-loop ac-01: kebab-case rename_all
        // must apply to every variant — `didnt-apply` is the only one whose
        // serialised form differs from snake_case, so test it explicitly.
        let parsed: InvocationOutcome = serde_yaml::from_str("didnt-apply").unwrap();
        assert_eq!(parsed, InvocationOutcome::DidntApply);
        let serialised = serde_yaml::to_string(&parsed).unwrap();
        assert!(
            serialised.trim() == "didnt-apply",
            "expected `didnt-apply`, got `{}`",
            serialised.trim()
        );
        // Sanity-check the other variants round-trip.
        for (variant, expected) in [
            (InvocationOutcome::Worked, "worked"),
            (InvocationOutcome::Partial, "partial"),
            (InvocationOutcome::DidntApply, "didnt-apply"),
            (InvocationOutcome::Incorrect, "incorrect"),
        ] {
            let s = serde_yaml::to_string(&variant).unwrap();
            assert_eq!(s.trim(), expected, "variant {variant:?} did not round-trip");
            let back: InvocationOutcome = serde_yaml::from_str(expected).unwrap();
            assert_eq!(back, variant);
        }
    }

    #[test]
    fn skill_invocation_omits_null_correction_on_serialize() {
        // spec 2026-05-15-agent-learning-loop ac-03 / ac-04: a row with no
        // correction must serialise without a `correction:` key at all so
        // the on-disk JSONL line matches the round-trip discipline.
        let inv = SkillInvocation {
            skill_id: "card".into(),
            session_id: "5f6b1a7e-7a32-4f6e-9d31-1a2b3c4d5e6f".into(),
            outcome: InvocationOutcome::Worked,
            correction: None,
            timestamp: "2026-05-15T12:00:00Z".into(),
        };
        let serialised = serde_yaml::to_string(&inv).unwrap();
        assert!(
            !serialised.contains("correction"),
            "expected no `correction` key when None; got: {serialised}"
        );
    }

    #[test]
    fn acceptance_criterion_fields_matches_struct() {
        // ac-04 verification: AcceptanceCriterion::FIELDS must equal the
        // struct's serde top-level field set. Mirrors spec_fields_matches_struct
        // — fully-populated fixture so skip_serializing_if doesn't drop fields.
        let ac = AcceptanceCriterion {
            id: "ac-01".into(),
            description: "d".into(),
            gate: false,
            checked: false,
            verification: Some("v".into()),
            ac_type: AcType::Observation,
        };
        let value = serde_yaml::to_value(&ac).unwrap();
        let got = top_level_keys(&value);
        let mut expected: Vec<String> =
            AcceptanceCriterion::FIELDS.iter().map(|s| s.to_string()).collect();
        expected.sort();
        assert_eq!(
            got, expected,
            "AcceptanceCriterion::FIELDS drifted from struct"
        );
    }

    #[test]
    fn scenario_fields_matches_struct() {
        let scenario = Scenario {
            name: "n".into(),
            given: "g".into(),
            when: "w".into(),
            then: "t".into(),
            gate: false,
        };
        let value = serde_yaml::to_value(&scenario).unwrap();
        let got = top_level_keys(&value);
        let mut expected: Vec<String> = Scenario::FIELDS.iter().map(|s| s.to_string()).collect();
        expected.sort();
        assert_eq!(got, expected, "Scenario::FIELDS drifted from struct");
    }

    #[test]
    fn relation_fields_matches_struct() {
        let relation = Relation {
            card: "c".into(),
            kind: RelationKind::Feeds,
            reason: "r".into(),
        };
        let value = serde_yaml::to_value(&relation).unwrap();
        let got = top_level_keys(&value);
        let mut expected: Vec<String> = Relation::FIELDS.iter().map(|s| s.to_string()).collect();
        expected.sort();
        assert_eq!(got, expected, "Relation::FIELDS drifted from struct");
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
                ac_type: AcType::Code,
            }],
        };
        let yaml = serde_yaml::to_string(&spec).unwrap();
        let parsed: Spec = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(spec, parsed);
    }

    #[test]
    fn acceptance_criterion_round_trips_ac_type_observation() {
        // spec 2026-05-16-ac-taxonomy ac-01 verification: round-trip an AC
        // with ac_type: observation, serialise to canonical YAML,
        // deserialise, assert byte-identical equality on the struct.
        let ac = AcceptanceCriterion {
            id: "ac-18".into(),
            description: "Post-cutover monitoring — 7-day live behaviour window".into(),
            gate: false,
            checked: false,
            verification: Some("operator dashboard review for 7 calendar days".into()),
            ac_type: AcType::Observation,
        };
        let yaml = serde_yaml::to_string(&ac).unwrap();
        let parsed: AcceptanceCriterion = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(ac, parsed);
        // Sanity: the field surfaces in the serialised YAML.
        assert!(yaml.contains("ac_type: observation"), "ac_type must serialise: {yaml}");
    }

    #[test]
    fn acceptance_criterion_parses_legacy_yaml_without_ac_type() {
        // spec 2026-05-16-ac-taxonomy ac-01 verification: existing spec.yaml
        // content without the ac_type field parses cleanly and deserialises
        // with ac_type: Code (the default).
        let legacy_yaml = "id: ac-01\ndescription: legacy\ngate: false\nchecked: false\n";
        let parsed: AcceptanceCriterion = serde_yaml::from_str(legacy_yaml).unwrap();
        assert_eq!(parsed.ac_type, AcType::Code);
    }

    #[test]
    fn ac_type_round_trips_snake_case_for_every_variant() {
        // spec 2026-05-16-ac-taxonomy ac-01 verification: every AcType
        // variant serialises to its snake_case form and back.
        for (variant, expected) in [
            (AcType::Code, "code"),
            (AcType::Config, "config"),
            (AcType::Doc, "doc"),
            (AcType::Ops, "ops"),
            (AcType::Observation, "observation"),
        ] {
            let s = serde_yaml::to_string(&variant).unwrap();
            assert_eq!(
                s.trim(),
                expected,
                "variant {variant:?} did not serialise to expected snake_case",
            );
            let back: AcType = serde_yaml::from_str(expected).unwrap();
            assert_eq!(back, variant);
        }
    }

    #[test]
    fn ac_type_blocks_close_two_band_split() {
        // spec 2026-05-16-ac-taxonomy ac-01 verification: blocks_close()
        // returns true for Code/Config/Doc and false for Ops/Observation.
        assert!(AcType::Code.blocks_close());
        assert!(AcType::Config.blocks_close());
        assert!(AcType::Doc.blocks_close());
        assert!(!AcType::Ops.blocks_close());
        assert!(!AcType::Observation.blocks_close());
    }

    #[test]
    fn ac_type_default_is_code() {
        // spec 2026-05-16-ac-taxonomy ac-01 verification: the Default
        // impl returns Code (matches the implicit assumption every
        // untyped AC carried before this field shipped).
        assert_eq!(AcType::default(), AcType::Code);
        assert!(AcType::default().is_code());
    }
}
