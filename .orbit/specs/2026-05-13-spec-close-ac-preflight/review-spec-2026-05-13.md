# Spec Review

**Date:** 2026-05-13
**Reviewer:** Context-separated agent (fresh session)
**Spec:** 2026-05-13-spec-close-ac-preflight
**Verdict:** APPROVE

---

## Review Depth

| Pass | Triggered by | Findings |
|------|-------------|----------|
| 1 — Structural scan | always | 0 hard, 1 soft (canonical-serialisation deferred to implementer) |
| 2 — Assumption & failure | content signals (release surface, schema back-compat, CLI↔MCP parity, drive-skill wire) | 4 |
| 3 — Adversarial | not triggered | — |

## Findings

### [MEDIUM] Response-shape additivity is convention, not contract
**Category:** assumption
**Pass:** 2
**Description:** ac-03 adds a `forced_unchecked: [...]` field to the success payload, ac-04 adds `time_gated_open: [...]`, and ac-07 asserts "the existing close response shape is preserved for callers that don't need the new fields." The spec treats additive fields as non-breaking, which is a reasonable convention but isn't pinned anywhere in the AC contract. Downstream consumers that deserialise with `deny_unknown_fields` (or equivalent strictness) would break — and orbit's own schema parsing uses FIELDS arrays for drift detection, which suggests strict deserialisation is the house style elsewhere.
**Evidence:** ac-01 explicitly extends `AcceptanceCriterion::FIELDS` to register `"time_gated"` against the FIELDS-drift unit test; by analogy, the close-response struct probably has equivalent strictness. The spec does not name the response struct or its strictness mode.
**Recommendation:** Add a one-line note in ac-03 / ac-04 (or a new sub-AC) confirming the close-response type allows additive fields without deserialisation breakage — either by naming the response struct's serde settings, or by adding a round-trip test that proves callers parsing an old shape against a new payload don't break. If the response type uses `deny_unknown_fields`, that's a contract change worth surfacing explicitly.

### [MEDIUM] ac-09 is the canonical `time_gated: true` example and is declared `time_gated: false`
**Category:** test-gap
**Pass:** 2
**Description:** ac-09 verifies the brew-released binary end-to-end. That verification (brew upgrade, tag check, smoke commands) cannot run until *after* the code is merged, tagged, and the tap is updated — i.e. it is a post-deploy observation AC, which is the prototypical use case for the `time_gated: true` field this spec introduces. Yet ac-09 is declared `gate: true` and (implicitly) `time_gated: false`, meaning the spec cannot close via `orbit spec close` without `--force` until ac-09 is ticked, and ac-09 can't be ticked until release happens. The release happens via `/orb:release` which itself triggers after merge. This creates a chicken-and-egg between "spec close" and "release" that is solved either by (a) using `--force` at close, (b) closing the spec *after* the release smoke, or (c) declaring ac-09 `time_gated: true`. Option (c) is the dogfooding-correct answer and makes this spec the first canonical use of its own feature.
**Evidence:** ac-09 verification text — "Brew-released binary at the new version installs cleanly… The smoke run is documented in the spec's `notes.md` or progress field at close-out." That is by definition a deferred-deferred verification.
**Recommendation:** Either (a) re-declare ac-09 `time_gated: true` (recommended — dogfoods the feature and matches the interview's "post-ship observation" definition), or (b) add an implementation note that ac-09 will be the first user of `--force` and the rationale will be captured in the drive's close-step notes per ac-08. Option (a) is cleaner and proves the feature on its own delivery.

### [LOW] Parity-test file location not named
**Category:** test-gap
**Pass:** 2
**Description:** ac-05 says "three new parity cases added to the existing CLI ↔ MCP parity test (the file added during the orbit-state-v0.1 work)" without naming the path. The implementing agent will need to locate it.
**Evidence:** ac-05 verification text.
**Recommendation:** Either name the file path in ac-05 or accept that the implementing agent runs a one-step search. Probably not worth respinning the spec — flagging for visibility.

### [LOW] Lock-ordering between new guard and existing guard is unstated
**Category:** assumption
**Pass:** 2
**Description:** Interview names "between validate_spec_id / lock-acquisition block and the unfinished-tasks check at line 1147, or immediately after — order TBD by the implementing agent." ac-02 says "ordering of the two checks is the implementing agent's call but is named in a code comment." Both leave guard ordering open. If the new guard runs *before* lock acquisition, a TOCTOU race could let two concurrent close attempts both pass with different AC views. The existing pattern (unfinished-tasks check after lock) is the obvious template — but the spec doesn't pin it.
**Evidence:** ac-02 description; interview implementation notes.
**Recommendation:** Add one line to ac-02 stating "the new guard runs after lock acquisition, like the unfinished-tasks guard" — or trust the implementing agent to mirror the precedent (which is the safer default). Low-severity because mirroring the precedent is the obvious read.

---

## Honest Assessment

The plan is implementation-ready. The schema change is minimal and back-compatible, the verb-level guard mirrors an existing pattern, the test coverage is comprehensive (verb tests, CLI↔MCP parity, happy-path regression), and the release / drive-wire ACs cover the cross-system surface. The biggest risk is the ac-09 self-reference — this spec introduces `time_gated` and its own release AC is the canonical example of one. Re-declaring ac-09 as `time_gated: true` would dogfood the feature on its own delivery and remove the need to either `--force` close or wait on release-then-close. That's a small spec edit, not a blocker. APPROVE on the strength of the rest; the ac-09 dogfooding is a recommendation rather than a gate.
