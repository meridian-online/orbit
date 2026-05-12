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
