# Spec Review

**Date:** 2026-05-09
**Reviewer:** Context-separated agent (fresh session)
**Spec:** 2026-05-09-orbit-method-md
**Verdict:** REQUEST_CHANGES

---

## Review Depth

| Pass | Triggered by | Findings |
|------|-------------|----------|
| 1 — Structural scan | always | 2 |
| 2 — Assumption & failure | content signals (cross-project distribution, backwards compat) + Pass 1 gaps | 4 |
| 3 — Adversarial | downstream-impact gap (legacy CLAUDE.md drift) surfaced in Pass 2 | 2 |

All five gate ACs (ac-01, ac-03, ac-04, ac-05, ac-06) pass the deterministic gate-description check (non-empty, no placeholder token, ≥20 chars — actual lengths 380–952 chars).

## Findings

### [HIGH] Legacy CLAUDE.md drift after re-run is unaddressed
**Category:** missing-requirement
**Pass:** 2
**Description:** Existing downstream projects that ran the old §6 already have inline `## Workflow (orbit)`, `## Orbit vocabulary`, and `## Current Sprint` blocks in their CLAUDE.md. The new §6 (per ac-03) appends an `@.orbit/METHOD.md` import but says nothing about removing or migrating those legacy blocks. After re-run, those projects end up with stale inline content beside fresh METHOD.md content — the two will diverge over plugin versions and contradict one another. This is the same drift problem the current §6 Case B migration was designed to handle for the vocabulary block; the new spec drops that migration with no replacement.
**Evidence:** Current `plugins/orb/skills/setup/SKILL.md:128–184` has Case A/B/C handling for `## Workflow (orbit)` + vocabulary block. ac-03 says §6 is "rewritten end-to-end" and inline blocks "are removed from SKILL.md" — but doesn't define what happens when an existing project's CLAUDE.md already contains those blocks. ac-07 only tests a "synthetic empty project", missing the legacy-migration path entirely.
**Recommendation:** Add an AC (or extend ac-03) covering legacy-CLAUDE.md migration: detect `## Workflow (orbit)` / `## Orbit vocabulary` / `## Current Sprint` markers in CLAUDE.md, prompt to remove them when the `@.orbit/METHOD.md` import is being added, and add a Case-B-equivalent test to ac-07.

### [MEDIUM] "Differs from canonical" comparison method is undefined
**Category:** test-gap
**Pass:** 2
**Description:** ac-03(c) prompts on drift; ac-07(a) asserts "byte-for-byte" on first run; ac-07(c) asserts "no METHOD.md content drift" on idempotent re-run; ac-07(d) asserts the prompt fires after author edits. None of these specifies the comparison method. Byte-equal? Hash? Whitespace-tolerant? Does the "How to update" line (per ac-02) participate in the comparison? Without this, the implement and test surfaces can diverge: the implementation could use `cmp -s` while the test asserts SHA equality, both passing, both incompatible.
**Evidence:** ac-03(c), ac-07(a), ac-07(c), ac-07(d) — four touchpoints, no comparison spec.
**Recommendation:** Pin the comparison rule in ac-03 explicitly. Suggest: "byte-for-byte equality of the entire file including the 'How to update' line" — and note that ac-07 must use the same primitive.

### [MEDIUM] AC ordering is implicit; ac-05 depends on ac-01
**Category:** failure-mode
**Pass:** 3
**Description:** ac-05 dogfoods orbit-repo CLAUDE.md by deleting substrate sections and replacing them with `@.orbit/METHOD.md`. If implementation runs ac-05 before ac-01 (METHOD.md template exists in the plugin source), the orbit-repo CLAUDE.md @-imports a non-existent file mid-implementation. The spec lists ACs in roughly the right order but doesn't declare the dependency, and `/orb:implement` can claim ACs in any order it chooses.
**Evidence:** ac-01 creates `plugins/orb/skills/setup/METHOD.md` (the canonical template). ac-05 expects the orbit repo to dogfood — but the orbit repo's `.orbit/METHOD.md` is itself created by running setup against the canonical, which only exists after ac-01. The dependency chain is ac-01 → ac-03 → (run setup) → ac-05.
**Recommendation:** Add a one-line dependency note to ac-05: "Depends on ac-01 (canonical template exists) and ac-03 (setup writes .orbit/METHOD.md). Run `/orb:setup` against the orbit repo as part of ac-05's implementation."

### [MEDIUM] @.orbit/STYLE.md import treatment is ambiguous
**Category:** missing-requirement
**Pass:** 2
**Description:** The orbit-repo CLAUDE.md currently has `@.orbit/STYLE.md` near the top (line 7). ac-01 says METHOD.md will include "a one-line BLUF / Decision Brief reference pointing at .orbit/STYLE.md". ac-05 lists what stays inline in orbit-repo CLAUDE.md ('# orbit' intro, Working in This Repo, Deployment, audit-survivor of Session Completion) but is silent on whether `@.orbit/STYLE.md` stays in CLAUDE.md, moves into METHOD.md as an `@-import`, or is replaced by a prose pointer. Each choice has different downstream behaviour: a project that imports METHOD.md but no STYLE.md gets only the pointer, not the contract.
**Evidence:** Current `CLAUDE.md:7` has `@.orbit/STYLE.md`. ac-01 mentions STYLE.md as a "one-line reference" in METHOD.md. ac-05 doesn't address STYLE.md. The existing card 0026 contract (per ac-08 framing) is load-bearing — losing it on downstream projects would silently regress the BLUF discipline.
**Recommendation:** State explicitly in ac-01 or ac-05 whether METHOD.md uses `@.orbit/STYLE.md` (so the contract travels with it) or a prose reference (so projects opt in). Either is defensible — the spec needs to pick one.

### [MEDIUM] Spec note location is implicit
**Category:** test-gap
**Pass:** 1
**Description:** ac-06 says "audit decisions are recorded as a spec note before the edits land". ac-08 says "a spec note records the load-bearing principle". The spec yaml has an empty `notes: []` field — but neither AC says these notes go there (vs the spec folder, vs an `orbit memory remember`, vs an inline progress.md note). The verification surface for ac-06 and ac-08 is therefore non-unique: a reviewer can't tell whether the spec is "done" by checking a single canonical location.
**Evidence:** Spec yaml `notes: []` is empty. ac-06 and ac-08 say "spec note" without naming the field/file.
**Recommendation:** Pin the location: "recorded under the spec yaml's `notes:` field" (or "in `progress.md` under a `## Notes` heading", whichever fits the substrate). One location, named.

### [LOW] ac-05's "key concepts" section migration loses repo-specific framing
**Category:** assumption
**Pass:** 3
**Description:** ac-05 lists "key concepts" as a section that moves to METHOD.md. The current orbit-repo CLAUDE.md `## Key Concepts` (lines 31–35) contains "Cards are living documents", "First-principles lens", "No backlogs" — these are substrate-shaped and fine to migrate. But the section header `Key Concepts` in METHOD.md will sit alongside the four pillars, the vocabulary table, the decision tree, and substrate rules — the single-screen target in ac-01 is at risk. The trimmable section per ac-01 is "the table"; if "key concepts" overflows, the table goes first, but the table is the most-referenced surface. Soft conflict between ac-01's "single screen" and ac-05's "move all substrate sections in".
**Evidence:** ac-01 lists METHOD.md content in seven blocks plus the "what orbit is" line. ac-05 adds "key concepts" on top. Single-screen budget tight at best.
**Recommendation:** Either drop "key concepts" from ac-05's migration list (those three bullets fit naturally in the decision-tree prose), or relax ac-01's single-screen target to a soft goal.

---

## Honest Assessment

The spec is well-shaped and scoped — eight ACs cover canonical template, setup wiring, card amendment, dogfooding, audit, test, and the discrimination rule. Five gates pass the structural check with rich descriptions. The work is unambiguously a single-spec piece, not a rally.

The biggest risk is downstream drift: existing projects that ran the old `/orb:setup` will end up with stale inline blocks in CLAUDE.md beside the new METHOD.md import, and the spec is silent on migrating them. The current §6 has Case-A/B/C migration for exactly this — dropping it without replacement is a regression for already-onboarded projects. Add legacy-CLAUDE.md handling and the spec is implementation-ready. The MEDIUM findings are sharpening, not blocking — but adding them before implementation will save iteration cycles later.

The four other MEDIUM findings (comparison method, AC ordering, STYLE.md treatment, spec-note location) are each one-line clarifications. Easy to bake in now; expensive to derive mid-implementation.
