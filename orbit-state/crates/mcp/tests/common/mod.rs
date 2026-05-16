//! Shared fixtures for parity tests.
//!
//! Mirror of `crates/cli/tests/common/mod.rs` — both copies MUST agree on
//! the fixture and the expected envelope. The parity claim: both surfaces
//! produce the same bytes core would produce for the same input. Both
//! surface tests assert against the same library-computed reference.

use orbit_state_core::{envelope_ok_string, SpecListResult, SpecSummary, VerbResponse};
use std::path::Path;

/// Populate `<root>/.orbit/specs/` with two specs (folder layout per
/// choice 0021):
/// - `0001/spec.yaml` — open, "first spec"
/// - `0002/spec.yaml` — closed, "second spec"
pub fn populate_two_specs(root: &Path) {
    let specs_dir = root.join(".orbit/specs");
    for (id, body) in [
        ("0001", "id: '0001'\ngoal: first spec\nstatus: open\n"),
        ("0002", "id: '0002'\ngoal: second spec\nstatus: closed\n"),
    ] {
        let folder = specs_dir.join(id);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("spec.yaml"), body).unwrap();
    }
}

/// The canonical envelope expected from `spec.list` against the two-spec
/// fixture, computed by the same library helper that the surfaces use.
pub fn expected_envelope_for_two_specs() -> String {
    let response = VerbResponse::SpecList(SpecListResult {
        specs: vec![
            SpecSummary {
                id: "0001".into(),
                goal: "first spec".into(),
                status: "open".into(),
                cards: vec![],
                labels: vec![],
            },
            SpecSummary {
                id: "0002".into(),
                goal: "second spec".into(),
                status: "closed".into(),
                cards: vec![],
                labels: vec![],
            },
        ],
    });
    envelope_ok_string(&response).expect("envelope serialisation infallible for fixture")
}

/// The expected error envelope for `--status nope`.
pub fn expected_envelope_for_invalid_status() -> String {
    use orbit_state_core::{envelope_err_string, Error};
    let err = Error::malformed("spec.list", "status must be 'open' or 'closed', got 'nope'");
    envelope_err_string(&err)
}

/// Expected envelope for `spec.show 0001` against the two-spec fixture.
pub fn expected_envelope_for_spec_show_0001() -> String {
    use orbit_state_core::schema::{Spec, SpecStatus};
    use orbit_state_core::{SpecShowResult, VerbResponse};
    let response = VerbResponse::SpecShow(SpecShowResult {
        spec: Spec {
            id: "0001".into(),
            goal: "first spec".into(),
            cards: vec![],
            status: SpecStatus::Open,
            labels: vec![],
            acceptance_criteria: vec![],
        },
    });
    orbit_state_core::envelope_ok_string(&response).expect("infallible")
}

/// Expected error envelope for `spec.show 0099` (not present).
pub fn expected_envelope_for_spec_show_missing(root: &Path) -> String {
    use orbit_state_core::{envelope_err_string, Error};
    let path = root.join(".orbit/specs/0099/spec.yaml");
    let err = Error::not_found("spec.show", format!("no spec at {}", path.display()));
    envelope_err_string(&err)
}

/// The deterministic note used by spec.note parity tests. MUST match
/// `crates/cli/tests/common/mod.rs::fixture_note` so both surface tests
/// assert against the same library-computed reference.
pub fn fixture_note() -> orbit_state_core::schema::NoteEvent {
    use orbit_state_core::schema::NoteEvent;
    NoteEvent {
        spec_id: "0001".into(),
        body: "parity test note".into(),
        labels: vec!["test".into()],
        timestamp: "2026-05-07T12:00:00Z".into(),
    }
}

pub fn expected_envelope_for_fixture_note() -> String {
    use orbit_state_core::{SpecNoteResult, VerbResponse};
    let response = VerbResponse::SpecNote(SpecNoteResult { note: fixture_note() });
    orbit_state_core::envelope_ok_string(&response).expect("infallible")
}

pub fn expected_notes_jsonl_for_fixture_note() -> String {
    orbit_state_core::canonical::serialise_json_line(&fixture_note())
        .expect("serialise_json_line is infallible for fixture")
}

/// Populate `<root>/.orbit/cards/` with two cards joined by a `feeds`
/// relation: 0001-alpha → 0002-beta. Used by card.tree parity tests.
pub fn populate_two_related_cards(root: &Path) {
    let cards_dir = root.join(".orbit/cards");
    std::fs::create_dir_all(&cards_dir).unwrap();
    std::fs::write(
        cards_dir.join("0001-alpha.yaml"),
        "id: 0001-alpha\nfeature: alpha\ngoal: alpha goal\nmaturity: planned\nrelations:\n- card: 0002-beta\n  type: feeds\n  reason: alpha feeds beta\n",
    )
    .unwrap();
    std::fs::write(
        cards_dir.join("0002-beta.yaml"),
        "id: 0002-beta\nfeature: beta\ngoal: beta goal\nmaturity: planned\n",
    )
    .unwrap();
}

/// Expected canonical envelope for `card.tree` with `slug=0001-alpha` and
/// `depth=1` against the two-related-cards fixture.
pub fn expected_envelope_for_card_tree_alpha_depth1() -> String {
    use orbit_state_core::{CardTreeEdge, CardTreeNode, CardTreeResult, VerbResponse};
    let response = VerbResponse::CardTree(CardTreeResult {
        root: "0001-alpha".into(),
        depth: 1,
        tree: CardTreeNode {
            slug: "0001-alpha".into(),
            feature: "alpha".into(),
            outgoing: vec![CardTreeEdge {
                kind: "feeds".into(),
                reason: "alpha feeds beta".into(),
                target: CardTreeNode {
                    slug: "0002-beta".into(),
                    feature: "beta".into(),
                    outgoing: vec![],
                    incoming: vec![],
                    truncated: true,
                },
            }],
            incoming: vec![],
            truncated: false,
        },
    });
    orbit_state_core::envelope_ok_string(&response).expect("infallible")
}

/// Expected error envelope for `card.tree` with an unknown numeric id.
pub fn expected_envelope_for_card_tree_unknown(cards_dir: &Path) -> String {
    use orbit_state_core::{envelope_err_string, Error};
    let err = Error::not_found(
        "card.tree",
        format!("no entry matching `9999-*` in {}", cards_dir.display()),
    );
    envelope_err_string(&err)
}

pub fn expected_envelope_for_card_specs_unknown(cards_dir: &Path) -> String {
    use orbit_state_core::{envelope_err_string, Error};
    let err = Error::not_found(
        "card.specs",
        format!("no entry matching `9999-*` in {}", cards_dir.display()),
    );
    envelope_err_string(&err)
}

pub fn expected_envelope_for_graph_unknown(cards_dir: &Path) -> String {
    use orbit_state_core::{envelope_err_string, Error};
    let err = Error::not_found(
        "graph",
        format!("no entry matching `9999-*` in {}", cards_dir.display()),
    );
    envelope_err_string(&err)
}

/// Populate `<root>/.orbit/` with one card (`0001-alpha`) listing a spec
/// (`s1`) that back-references it. Used by card.specs parity tests.
pub fn populate_card_with_linked_spec(root: &Path) {
    let cards_dir = root.join(".orbit/cards");
    std::fs::create_dir_all(&cards_dir).unwrap();
    std::fs::write(
        cards_dir.join("0001-alpha.yaml"),
        "id: 0001-alpha\nfeature: alpha\ngoal: alpha goal\nmaturity: planned\nspecs:\n- .orbit/specs/s1/spec.yaml\n",
    )
    .unwrap();
    let spec_dir = root.join(".orbit/specs/s1");
    std::fs::create_dir_all(&spec_dir).unwrap();
    std::fs::write(
        spec_dir.join("spec.yaml"),
        "id: s1\ngoal: spec one\ncards:\n- 0001-alpha\nstatus: open\n",
    )
    .unwrap();
}

/// Populate `<root>/.orbit/cards/0001-alpha.yaml` with a top-level unknown
/// field. Used by audit.drift parity tests.
pub fn populate_card_with_drift(root: &Path) {
    let cards_dir = root.join(".orbit/cards");
    std::fs::create_dir_all(&cards_dir).unwrap();
    std::fs::write(
        cards_dir.join("0001-alpha.yaml"),
        "id: 0001-alpha\nfeature: alpha\ngoal: alpha goal\nmaturity: planned\nlegacy_field: x\n",
    )
    .unwrap();
}

/// Expected canonical envelope for `audit.drift` against the
/// card-with-drift fixture.
pub fn expected_envelope_for_audit_drift_one_unknown() -> String {
    use orbit_state_core::{AuditDriftResult, DriftEntry, VerbResponse};
    let response = VerbResponse::AuditDrift(AuditDriftResult {
        drift: vec![DriftEntry {
            path: ".orbit/cards/0001-alpha.yaml".into(),
            kind: "card".into(),
            field: "legacy_field".into(),
            disposition: "quarantine".into(),
        }],
    });
    orbit_state_core::envelope_ok_string(&response).expect("infallible")
}

/// Expected canonical envelope for `graph` (mermaid, unscoped) against
/// the two-related-cards fixture.
pub fn expected_envelope_for_graph_mermaid_two_related_cards() -> String {
    use orbit_state_core::{GraphResult, VerbResponse};
    let text = String::from(
        "graph LR\n\
         \x20\x20c_0001_alpha[\"0001-alpha: alpha\"]\n\
         \x20\x20c_0002_beta[\"0002-beta: beta\"]\n\
         \x20\x20c_0001_alpha -->|feeds| c_0002_beta\n",
    );
    let response = VerbResponse::Graph(GraphResult {
        format: "mermaid".into(),
        text,
    });
    orbit_state_core::envelope_ok_string(&response).expect("infallible")
}

/// Expected canonical envelope for `overview` against the two-related-cards
/// fixture (alpha feeds beta; both planned; no specs; no memories).
pub fn expected_envelope_for_overview_two_related_cards() -> String {
    use orbit_state_core::{
        CardMaturityCounts, MostConnectedCard, OverviewResult, VerbResponse,
    };
    let response = VerbResponse::Overview(OverviewResult {
        open_spec_count: 0,
        recent_open_spec_ids: vec![],
        spec_overflow: 0,
        cards_by_maturity: CardMaturityCounts {
            planned: 2,
            emerging: 0,
            established: 0,
        },
        memories: vec![],
        most_connected_card: Some(MostConnectedCard {
            slug: "0001-alpha".into(),
            feature: "alpha".into(),
            degree: 1,
        }),
        orphans: vec!["0001-alpha".into()],
        orphan_overflow: 0,
    });
    orbit_state_core::envelope_ok_string(&response).expect("infallible")
}

/// Expected canonical envelope for `card.specs` with `slug=0001-alpha`.
pub fn expected_envelope_for_card_specs_alpha() -> String {
    use orbit_state_core::{CardSpecsEntry, CardSpecsResult, VerbResponse};
    let response = VerbResponse::CardSpecs(CardSpecsResult {
        root: "0001-alpha".into(),
        specs: vec![CardSpecsEntry {
            spec_id: "s1".into(),
            spec_path: ".orbit/specs/s1/spec.yaml".into(),
            listed_on_card: true,
            back_referenced_by_spec: true,
            status: "open".into(),
        }],
    });
    orbit_state_core::envelope_ok_string(&response).expect("infallible")
}

// ---------------------------------------------------------------------------
// spec.close AC pre-flight (spec 2026-05-13-spec-close-ac-preflight, ac-05)
// ---------------------------------------------------------------------------

/// Populate `<root>/.orbit/` with one card and one open spec carrying ACs:
/// - `ac-01` checked, non-gate, non-time-gated
/// - `ac-02` unchecked, non-gate, non-time-gated  ← blocks close
/// - `ac-03` unchecked, non-gate, time-gated      ← reported, does not block
pub fn populate_spec_close_preflight_fixture(root: &Path) {
    let cards_dir = root.join(".orbit/cards");
    std::fs::create_dir_all(&cards_dir).unwrap();
    std::fs::write(
        cards_dir.join("0020-orbit-state.yaml"),
        "id: 0020-orbit-state\nfeature: orbit-state\ngoal: substrate\nmaturity: planned\n",
    )
    .unwrap();
    let spec_dir = root.join(".orbit/specs/0001");
    std::fs::create_dir_all(&spec_dir).unwrap();
    std::fs::write(
        spec_dir.join("spec.yaml"),
        "id: '0001'\n\
         goal: g\n\
         cards:\n\
         - 0020-orbit-state\n\
         status: open\n\
         acceptance_criteria:\n\
         - id: ac-01\n  description: first\n  gate: false\n  checked: true\n\
         - id: ac-02\n  description: second\n  gate: false\n  checked: false\n\
         - id: ac-03\n  description: third\n  gate: false\n  checked: false\n  ac_type: observation\n",
    )
    .unwrap();
}

/// Populate a fixture where only a deferrable-kind AC remains unchecked —
/// used to verify spec.close succeeds without `--force` when the sole open
/// AC is `ac_type: observation` (spec 2026-05-16-ac-taxonomy ac-02).
pub fn populate_spec_close_only_deferrable_fixture(root: &Path) {
    let cards_dir = root.join(".orbit/cards");
    std::fs::create_dir_all(&cards_dir).unwrap();
    std::fs::write(
        cards_dir.join("0020-orbit-state.yaml"),
        "id: 0020-orbit-state\nfeature: orbit-state\ngoal: substrate\nmaturity: planned\n",
    )
    .unwrap();
    let spec_dir = root.join(".orbit/specs/0001");
    std::fs::create_dir_all(&spec_dir).unwrap();
    std::fs::write(
        spec_dir.join("spec.yaml"),
        "id: '0001'\n\
         goal: g\n\
         cards:\n\
         - 0020-orbit-state\n\
         status: open\n\
         acceptance_criteria:\n\
         - id: ac-01\n  description: first\n  gate: false\n  checked: true\n\
         - id: ac-02\n  description: second\n  gate: false\n  checked: false\n  ac_type: observation\n",
    )
    .unwrap();
}

/// Expected error envelope when `spec close 0001` runs against the
/// pre-flight fixture (ac-02 is unchecked, blocking-kind).
pub fn expected_envelope_for_spec_close_unchecked_blocking() -> String {
    use orbit_state_core::{envelope_err_string, Error};
    let err = Error::conflict("spec.close", "1 unchecked blocking AC(s) in spec '0001': ac-02");
    envelope_err_string(&err)
}

/// Expected ok envelope when `spec close --force 0001` runs against the
/// pre-flight fixture. The closed spec includes the new fields:
/// `forced_unchecked: [ac-02]`, `deferrable_open: [ac-03]`.
pub fn expected_envelope_for_spec_close_force() -> String {
    use orbit_state_core::schema::{AcType, AcceptanceCriterion, Spec, SpecStatus};
    use orbit_state_core::{envelope_ok_string, SpecCloseResult, VerbResponse};
    let response = VerbResponse::SpecClose(SpecCloseResult {
        spec: Spec {
            id: "0001".into(),
            goal: "g".into(),
            cards: vec!["0020-orbit-state".into()],
            status: SpecStatus::Closed,
            labels: vec![],
            acceptance_criteria: vec![
                AcceptanceCriterion {
                    id: "ac-01".into(),
                    description: "first".into(),
                    gate: false,
                    checked: true,
                    verification: None,
                    ac_type: AcType::Code,
                },
                AcceptanceCriterion {
                    id: "ac-02".into(),
                    description: "second".into(),
                    gate: false,
                    checked: false,
                    verification: None,
                    ac_type: AcType::Code,
                },
                AcceptanceCriterion {
                    id: "ac-03".into(),
                    description: "third".into(),
                    gate: false,
                    checked: false,
                    verification: None,
                    ac_type: AcType::Observation,
                },
            ],
        },
        cards_updated: vec!["0020-orbit-state".into()],
        forced_unchecked: vec!["ac-02".into()],
        deferrable_open: vec!["ac-03".into()],
    });
    envelope_ok_string(&response).expect("infallible")
}

/// Expected ok envelope when `spec close 0001` runs against the
/// only-deferrable fixture. Closure succeeds without `--force`;
/// `deferrable_open: [ac-02]`, `forced_unchecked` empty.
pub fn expected_envelope_for_spec_close_only_deferrable() -> String {
    use orbit_state_core::schema::{AcType, AcceptanceCriterion, Spec, SpecStatus};
    use orbit_state_core::{envelope_ok_string, SpecCloseResult, VerbResponse};
    let response = VerbResponse::SpecClose(SpecCloseResult {
        spec: Spec {
            id: "0001".into(),
            goal: "g".into(),
            cards: vec!["0020-orbit-state".into()],
            status: SpecStatus::Closed,
            labels: vec![],
            acceptance_criteria: vec![
                AcceptanceCriterion {
                    id: "ac-01".into(),
                    description: "first".into(),
                    gate: false,
                    checked: true,
                    verification: None,
                    ac_type: AcType::Code,
                },
                AcceptanceCriterion {
                    id: "ac-02".into(),
                    description: "second".into(),
                    gate: false,
                    checked: false,
                    verification: None,
                    ac_type: AcType::Observation,
                },
            ],
        },
        cards_updated: vec!["0020-orbit-state".into()],
        forced_unchecked: vec![],
        deferrable_open: vec!["ac-02".into()],
    });
    envelope_ok_string(&response).expect("infallible")
}

/// Fixed UUID for deterministic `session.start` parity tests.
pub const PARITY_SESSION_ID: &str = "00000000-0000-4000-8000-000000000001";

/// Fixed timestamp for deterministic skill-invocation parity tests.
pub const PARITY_TIMESTAMP: &str = "2026-05-15T12:00:00Z";

/// Expected ok envelope for `session start --id <PARITY_SESSION_ID>`
/// against the given root.
pub fn expected_envelope_for_session_start(root: &Path) -> String {
    use orbit_state_core::{envelope_ok_string, SessionStartResult, VerbResponse};
    let path = root.join(".orbit").join(".session-id");
    let response = VerbResponse::SessionStart(SessionStartResult {
        session_id: PARITY_SESSION_ID.into(),
        path: path.display().to_string(),
    });
    envelope_ok_string(&response).expect("infallible")
}

/// Expected ok envelope for `skill record-invocation card --outcome worked
/// --session-id <PARITY_SESSION_ID> --timestamp <PARITY_TIMESTAMP>`.
pub fn expected_envelope_for_skill_record_invocation() -> String {
    use orbit_state_core::schema::{InvocationOutcome, SkillInvocation};
    use orbit_state_core::{envelope_ok_string, SkillRecordInvocationResult, VerbResponse};
    let response = VerbResponse::SkillRecordInvocation(SkillRecordInvocationResult {
        invocation: SkillInvocation {
            skill_id: "card".into(),
            session_id: PARITY_SESSION_ID.into(),
            outcome: InvocationOutcome::Worked,
            correction: None,
            timestamp: PARITY_TIMESTAMP.into(),
        },
    });
    envelope_ok_string(&response).expect("infallible")
}

/// Expected ok envelope for `skill recurrence design` against an empty
/// (or absent) invocation file.
pub fn expected_envelope_for_skill_recurrence_empty() -> String {
    use orbit_state_core::{
        envelope_ok_string, RecurrenceByOutcome, SkillRecurrenceResult, VerbResponse,
    };
    let response = VerbResponse::SkillRecurrence(SkillRecurrenceResult {
        skill_id: "design".into(),
        by_outcome: RecurrenceByOutcome::default(),
        total: 0,
    });
    envelope_ok_string(&response).expect("infallible")
}

/// Expected ok envelope for `session distill --session-id <PARITY_SESSION_ID>`
/// with the given distillate text. Caller must read `started_at` / `ended_at`
/// from disk after the call before computing this.
pub fn expected_envelope_for_session_distill(
    distillate: &str,
    started_at: &str,
    ended_at: &str,
) -> String {
    use orbit_state_core::schema::Session;
    use orbit_state_core::{envelope_ok_string, SessionDistillResult, VerbResponse};
    let response = VerbResponse::SessionDistill(SessionDistillResult {
        session: Session {
            id: PARITY_SESSION_ID.into(),
            started_at: started_at.into(),
            ended_at: Some(ended_at.into()),
            distillate: distillate.into(),
            labels: vec![],
        },
    });
    envelope_ok_string(&response).expect("infallible")
}
