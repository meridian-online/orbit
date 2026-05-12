# Spec Review

**Date:** 2026-05-11
**Reviewer:** Context-separated agent (fresh session)
**Spec:** 2026-05-12-tree-views
**Verdict:** REQUEST_CHANGES

---

## Review Depth

| Pass | Triggered by | Findings |
|------|-------------|----------|
| 1 — Structural scan | always | 1 |
| 2 — Assumption & failure | content signals (cross-system boundaries: CLI + MCP + session-prime + multiple SKILL.md files; schema-via-Rust-struct) | 2 |
| 3 — Adversarial | not triggered — Pass 2 issues are local to specific ACs, no cross-AC cascades | — |

## Findings

### [MEDIUM] AC-03 unbounded `open spec count + ids` contradicts the single-screen pre-commit
**Category:** constraint-conflict
**Pass:** 2
**Description:** AC-03 names five outputs for `orbit overview` and bounds two of them with explicit caps — "recent memories (last K=10)" and "cards-by-maturity counts" — but lists "open spec count + ids" without a cap. The card 0033 "Synthesis stays bounded as the project ages" scenario (gate: false but rationally load-bearing — quoted in the spec's goal as "synthesis stays bounded") pre-commits to single-screen output regardless of project age. A mature project with 30+ open specs makes the spec-id line wrap off-screen and breaks the contract. The cap-or-truncate rule needs to be in the AC so the implement skill can test it; "open spec count + ids" today admits both bounded and unbounded implementations.
**Evidence:** AC-03 description (spec.yaml:18-21). Card 0033 scenarios (cards/0033-see-the-tree.yaml:31-35) — "Synthesis stays bounded as the project ages": `output remains single-screen; the token cost of asking "where are we?" does not grow with project age`. Goal field (spec.yaml:2) repeats "synthesis stays bounded as the project ages" verbatim.
**Recommendation:** Pin a cap in AC-03 mirroring the memory cap: replace "open spec count + ids" with "open spec count + last K=10 spec ids by date" (or whatever ordering the implementor picks, but pin one). For projects with more than K open specs, the output shows count plus the K most-recent ids and a `+N more` suffix. This makes the bounded contract mechanically testable.

### [LOW] AC-04 default render (no `--card`) is unbounded
**Category:** missing-requirement
**Pass:** 2
**Description:** AC-04 says `orbit graph` with no flags renders the full cards-choices-specs graph as mermaid to stdout. Default depth 2 only applies when `--card` is set (a depth-from-a-root makes no sense without a root). For a mature project the unscoped render is hundreds of nodes — the verb sits at odds with the "bounded synthesis" pre-commit in the goal and in card 0033. AC-04 doesn't claim the bounded contract directly (unlike AC-03), so this is sharpening, not contradiction, but it's worth pinning now to rule out scope drift at implement time.
**Evidence:** AC-04 description (spec.yaml:22-25). Card 0033 "Visual graph render on demand" (cards/0033-see-the-tree.yaml:25-29) doesn't claim bounded output either — it's positioned as the deliberately-larger render compared to `overview`.
**Recommendation:** Add one clause to AC-04: either (a) "`--card` is required for the default mermaid path; the unscoped global render requires `--all` and is explicitly unbounded", or (b) "the unscoped default render is permitted to exceed single-screen — it serves the share-or-paste use case, not the synthesis use case". Option (b) keeps the verb shape simple and accepts the trade-off honestly. Either way, the AC should name which.

### [LOW] AC-05 schema-source reflection mechanism unstated
**Category:** test-gap
**Pass:** 1
**Description:** AC-05 says the audit "compares each top-level field against the canonical schema sourced from the Rust structs (`Card`, `Spec`, `Choice`, `Memory`)". Rust has no runtime reflection — the field-name set must be declared somewhere statically (a `const FIELDS: &[&str]`, a `schemars`-derived JSONSchema, or a procedural macro emitting the field list). The AC doesn't say. This is implementable in multiple ways and the choice has small-but-real test consequences (a `const &[&str]` drifts independently from the struct if hand-maintained; a derive macro stays in lockstep). Picking the mechanism in the AC means the implement skill writes the test that catches the drift, not just the test that exercises the happy path.
**Evidence:** AC-05 description (spec.yaml:26-29). The four target structs in `orbit-state/crates/core/src/schema.rs:46-262` are plain `#[derive(Deserialize)]` with `#[serde(deny_unknown_fields)]` — no field list is exported.
**Recommendation:** Sharpen AC-05 to name the source-of-truth mechanism for the field list. Cheapest path: a derive macro (or `schemars`) that emits `Card::FIELDS: &[&str]` and equivalents — and an AC that the derive-emitted list is the *only* source consulted, so adding a new field to the struct automatically adds it to the audit's allow-set. Alternative: hand-maintained `const FIELDS` with a compile-time test that asserts the constant matches the struct's serde representation. Either is implementable; the AC should pick one.

---

## Gate-AC description check

Five gate ACs (ac-01, ac-03, ac-06, ac-07, ac-08) — all pass the deterministic structural rules: non-empty, no placeholder tokens, all far above the 20-character minimum. No deterministic findings from this rule.

---

## Honest Assessment

The revised spec is materially stronger than v1. The two HIGH/MEDIUM blockers from the first review are resolved: AC-05 now names the Rust struct as the schema source (no longer depends on a SCHEMA.md that doesn't exist); AC-06 explicitly scopes both CLI and MCP surfaces with parity-test paths for each; AC-03 pins the edge-set rule, tie-break, and orphan definition; AC-04 bounds the filter set; AC-07 puts the session-prime wire as load-bearing and names the right skills; AC-08 adds the error-envelope coverage. What remains is one MEDIUM — AC-03's spec-id list is the one unbounded element in an otherwise bounded synthesis verb, and the goal's "synthesis stays bounded" pre-commit makes that a contract gap — plus two LOWs around AC-04's unscoped render and AC-05's reflection mechanism. None is rework-grade; all three are AC text edits. The verdict is REQUEST_CHANGES rather than APPROVE on the strength of the AC-03 bound: a gate AC whose output can violate the goal's bounded-synthesis pre-commit needs the cap pinned before implement starts, not discovered at review-pr time. Fix the three AC clauses and the spec is implement-ready.
