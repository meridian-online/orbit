# Design: Four-pillars wires — README, schema field, skill citations, audit

**Date:** 2026-05-08
**Interviewer:** Claude Opus 4.7 (orbit /orb:design)
**Card:** .orbit/cards/0028-four-pillars.yaml

---

## Context

Card *Four pillars — the load-bearing user outcomes orbit exists to deliver* — 8 scenarios (3 gates), goal: every card carries a `pillar` field naming at least one pillar; cards that move no pillar are re-scoped or retired.

Prior specs: 0. Pillars are named in CLAUDE.md (lines 48-57) but not in README.md, not in the card schema, not cited in any SKILL.md, and `/orb:audit` doesn't check for them — currently a spec-AC traceability skill only.

Gap: card 0028's "Wired into the framework" gate names four wires (CLAUDE/README naming, schema field, distill+design+review-spec SKILL.md citations, /orb:audit pillar check). Only one (CLAUDE.md naming) is partially live. This spec scopes the rest.

Pattern in hand: 2026-05-08-executive-communication-wires shipped as 9 ACs covering file creation + CLAUDE.md import + 3 skill citations + verification + close. Card 0028's wires shape similarly but add a **schema-field** dimension (Rust changes in `orbit-state/crates/core/src/schema.rs`) the prior spec didn't have.

Choice 0019 names 0028 as the canonical case operationalising pillar 3 (state-persistence) via substrate-enforced wires, and earmarks /orb:audit for a parallel "aspirational card" column — this spec coordinates with that.

## Q&A

### Q1: Spec scope
**Q:** Card 0028 names four wires (CLAUDE/README naming, schema field, distill+design+review-spec SKILL.md citations, /orb:audit pillar check). The schema-field wire is materially heavier than the 0026 precedent — Rust changes to `core/src/schema.rs`, canonicalise round-trip, verify gate. How should this spec scope?
**A:** All four wires in one spec. Mirror the 0026 precedent at slightly larger scale; one coherent delivery rather than fragmented follow-ups.

### Q2: Existing-cards backfill
**Q:** 23 existing cards have no pillar field. Card 0028's non-gate scenario calls for backfilling them with honest pillar claims. Subjective work — judging which pillar each card moves. Where does this live?
**A:** Separate spec, sequenced after the schema lands. 23 cards × thoughtful judgement is high cognitive load that doesn't combine well with substrate plumbing.

### Q3: Enforcement strength
**Q:** When a card lacks a `pillar` field, what should happen? Pick the strictest level shipped in this spec — softer levels can layer on top.
**A:** Soft. `/orb:audit` flags missing pillars; `orbit verify` still passes. Mirrors choice 0019's audit-only enforcement for aspirational cards. Lower blast radius — the backfill spec can proceed at its own pace without breaking verify for everyone.

### Q4: README treatment
**Q:** README.md doesn't currently mention the four pillars. The gate calls for them "at top level" of CLAUDE.md and README.md. How should README treat them?
**A:** Top-level section, distilled for public reading. README.md grows a "Four pillars" H2 — short, public-voice paragraph per pillar. CLAUDE.md remains the working contract; README is the public face.

### Q5: /orb:audit scope expansion
**Q:** /orb:audit currently audits AC-to-test traceability for specs. Card 0028 wants it to flag cards with no pillar claim, and choice 0019 wants it to flag aspirational cards. These are card-level audits — different scope from the current spec/AC focus.
**A:** Extend /orb:audit with a card-mode alongside spec-mode. Single skill grows two report sections: spec/AC traceability (existing) + card health (pillar field, aspirational `specs: []`). Coherent home for all audit checks.

---

## Scope amendments after Q&A

The Q&A above settled an "all-four-wires-in-one-spec" plan. Subsequent conversation pruned this materially. Two pushbacks from Hugh:

1. **Schema-field is overreach.** Adding a `pillars` field to the Card struct in `orbit-state/crates/core/src/schema.rs` would bake orbit's own user-outcome framing into every project that installs orbit — pillar 4 in particular ("long-running R&D") is closer to Hugh's working style than a universal user outcome. Substrate-level enforcement is the wrong layer.
2. **Pillars are emergent outcomes, not agent behaviours.** Telling an agent to "be self-learning" is incantation; self-learning is what *happens* when memory tools, accretion patterns, and substrate exist. Citations in `/orb:distill`, `/orb:design`, `/orb:review-spec` SKILL.md — even framed as agent-side silent shaping — would risk performative wiring without realising the outcomes. The actual pillar wires live in the cards that deliver each pillar's mechanism (0023 memory-loop for self-learning, 0029 fan-out for R&D, 0020 orbit-state for state-persistence, 0026 executive-communication for executive interaction).

Final scope is documentation + card amendment + cleanup. The framework exists in CLAUDE.md and README; *delivering* the pillars is the job of every other card that proposes a pillar mechanism.

## Summary

### Goal

Document the four pillars publicly (README) and amend card 0028 so its gate scenarios match the realised wiring shape — pillars as a documented design heuristic, not a substrate-enforced contract or an agent-prompt instruction. Closes the documentation gate scenario; reframes the schema-field and audit gates around documentation + relations-graph; flips card 0028's `specs:` array from empty (aspirational) to populated.

### Constraints

- No Rust schema changes. The `Card` struct in `orbit-state/crates/core/src/schema.rs` stays as-is; pillars are not a structured field.
- No SKILL.md citations to pillars in `/orb:distill`, `/orb:design`, `/orb:review-spec`. Pillars stay as documentation; the framework doesn't tell agents to embody outcomes.
- No `/orb:audit` card-mode pillar check. Choice 0019's aspirational-card column is a separate concern under choice 0019's spec, not this one.
- Card 0028's two gate scenarios that called for schema enforcement and audit flagging are amended down to documentation + relations-graph framing. The wires gate scenario is rephrased to match the realised shape.
- Card 0028's reference to `.orbit/memos/2026-05-08-four-pillars.md` (deleted post-distill) is stale and gets removed or annotated as historical.

### Success Criteria

- `README.md` (repo root) gains a top-level "Four pillars" H2 section — public-voice prose, one short paragraph per pillar (executive interaction, self-learning, state-persistence, long-running R&D).
- Card 0028's gate scenario 2 ("Every card cites at least one pillar") is amended to drop the schema-field requirement; pillars become an optional design heuristic surfaced via existing `relations:` graph, not a required field.
- Card 0028's gate scenario 8 ("Wired into the framework") is amended to drop schema-field and audit-pillar-check from its wires list; the wires named are CLAUDE.md naming, README.md naming, and pillar-defining cards' relations graph.
- Card 0028's non-gate scenarios 4 ("Distill asks the pillar question") and 5 ("Design and review surface the parent's pillar") are amended for consistency — reframed around silent agent-side awareness via the relations graph, not user-facing pillar questions.
- Card 0028's stale memo reference is removed or annotated as historical.
- `orbit verify` passes after all changes; `orbit spec close 2026-05-08-four-pillars-wires` succeeds — appending this spec's id to card 0028's `specs:` array. Choice 0019's aspirational-card audit signal flips for 0028.

### Decisions Surfaced

- **Documentation, not substrate.** Pillars stay as prose in CLAUDE.md and README; no schema field, no audit pillar-check. Alternative: enforce via `Card.pillars` field. Rationale: the four pillars are orbit's framing for *this repo*; substrate enforcement would impose them on every project that installs orbit. Pillar 1 (executive interaction) is also asymmetric — agents pay the compression cost; making the user (or other-project users) declare pillars inverts that.
- **No SKILL.md citations.** Alternative: cite pillars as agent-side shaping in distill/design/review-spec. Rationale: pillars are emergent outcomes of mechanisms, not behaviours an agent embodies via prompt instruction. Citing them in SKILL.md risks performative wiring. The pillar mechanisms live in other cards' specs (0023, 0026, 0029, 0020) — those are the real wires.
- **Card 0028 amendment, not just spec close.** Alternative: leave the card's gate scenarios as-written and ship a partial-wires spec. Rationale: card 0028's gate scenarios pre-date this conversation; leaving them as-is creates permanent misalignment between card text and realised scope. Amending the card is honest curation.

### Implementation Notes

- **README placement.** The "Four pillars" section sits after the existing "Why a workflow at all?" intro (around line 10) and before install/usage. Public-voice — one short paragraph per pillar, no orbit-internal jargon.
- **CLAUDE.md is unchanged in this spec.** It already names the pillars (lines 48-57); no edits needed.
- **Card 0028 gate scenario 2 amendment.** Current text mandates a `pillar` field with id + claim. Reword to: cards may declare pillar contribution via existing `relations: feeds` to a pillar-defining card; not enforced; surfaced as a design heuristic at distill time, optional and emergent.
- **Card 0028 gate scenario 8 amendment.** Current wires list: CLAUDE.md/README naming, schema field, distill/design/review-spec SKILL.md citations, /orb:audit pillar-check. Reword wires to: CLAUDE.md naming, README.md naming, pillar-defining cards' relations graph (each pillar has cards that operationalise it; the relations graph is the wire). Drop schema field and audit pillar-check from the wires list.
- **Card 0028 non-gate scenarios 4 and 5.** Currently frame distill/design as "asking the pillar question" and "surfacing the parent's pillar". Reword as: pillar awareness is agent-side context derived from the relations graph; not surfaced to the user as a mid-flow question. The author may choose to add a `relations: feeds` entry to a pillar-defining card; not required.
- **Stale reference cleanup.** Card 0028's `references:` list cites `.orbit/memos/2026-05-08-four-pillars.md` — deleted post-distill (normal state). Mirror the executive-communication-wires precedent: remove or annotate as historical.
- **Card 0028 yaml round-trip.** Amending scenarios edits the yaml directly; must round-trip through the canonical writer (`orbit canonicalise` or similar) to pass `orbit verify`.

### Open Questions

- **Cross-project reach for the pillars.** README + CLAUDE.md only name pillars in *this* repo. Pillars are orbit's working framing; they may not transfer to other projects that install orbit. Probably correct — the pillars are explicitly Hugh's user-outcome framing for orbit-the-project, not a universal contract. Flagged for distill, not a follow-up spec.
- **Pillar mechanism delivery.** This spec documents the pillars; the pillars are *realised* by other cards' specs (0023 memory-loop, 0029 fan-out, etc.). Tracking which pillars are well-mechanised vs underdelivered is a curation question the framework doesn't currently surface — possibly future audit work, not this spec.

---

**Next step:** `/orb:spec` to generate a structured specification from this design session.
