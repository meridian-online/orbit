# Spec Review

**Date:** 2026-05-09
**Reviewer:** Context-separated agent (fresh session)
**Spec:** 2026-05-09-orbit-method-md
**Verdict:** REQUEST_CHANGES

---

## Review Depth

| Pass | Triggered by | Findings |
|------|-------------|----------|
| 1 — Structural scan | always | 1 |
| 2 — Assumption & failure | content signals (cross-project distribution, downstream backwards-compat, plugin marketplace) | 4 |
| 3 — Adversarial | sequencing conflict between ac-05 and ac-06 surfaced in Pass 2 | 1 |

All six gate ACs (ac-01, ac-03, ac-04, ac-05, ac-06, ac-09) pass the deterministic gate-description rules: non-empty, no placeholder tokens, all well over the 20-char minimum (380–952 chars). Cycle-1 findings (legacy CLAUDE.md drift, comparison method, AC ordering, STYLE.md treatment in orbit-repo, spec-note location) were addressed in cycle-2 amendments per the notes stream. The findings below are new — uncovered by reading the cycle-2 spec cold.

## Findings

### [HIGH] Decline path of legacy-migration prompt produces the drift state the spec exists to prevent
**Category:** failure-mode
**Pass:** 2
**Description:** ac-03(d) describes the legacy-CLAUDE.md migration prompt: detect `## Workflow (orbit)` / `## Orbit vocabulary` / `## Current Sprint` markers, prompt to remove them when the `@-import` is added, and on decline "leaves the legacy blocks in place but emits a one-line warning". The prompt is only about *removal* — the `@.orbit/METHOD.md` import is added unconditionally per ac-03(b). So a user who declines the prompt ends up with both the legacy inline blocks AND the new `@-import` in their CLAUDE.md. That is exactly the drift state the spec is trying to eliminate: two sources of truth, guaranteed to diverge across plugin versions. ac-07(g) tests this path and asserts only the warning fires, not that the state is recoverable. The decline path is a footgun, not a graceful degradation.
**Evidence:** ac-03(b) "ensure the project's CLAUDE.md contains an `@.orbit/METHOD.md` line — idempotent" (no decline branch). ac-03(d) "Decline leaves the legacy blocks in place but emits a one-line warning naming the drift risk." ac-07(g) "declining leaves blocks intact and emits the drift-risk warning." None of these say what happens to the `@-import` on decline.
**Recommendation:** Make the decline path coherent. Two defensible options: (1) on decline, *also* skip adding the `@-import` (treat the migration as atomic — either both or neither), with a different warning explaining that `@-import` was deferred until legacy blocks are removed; or (2) on decline, add the `@-import` but emit a louder, more specific warning naming the exact line numbers of the legacy blocks and the `orbit setup --migrate` (or equivalent) command to re-run later. Pick one and pin it in ac-03(d) and ac-07(g).

### [MEDIUM] ac-05 and ac-09 ordering forces ac-05 to preview ac-06's classification
**Category:** constraint-conflict
**Pass:** 3
**Description:** ac-05 says the orbit-repo CLAUDE.md is rewritten so substrate-shaped sections move to METHOD.md and named sections stay inline — including "any audit-survivor of 'Session Completion' (per ac-06)". ac-09 sequences ac-05 *before* ac-06. So when the implementing agent runs ac-05, it must either (a) preview ac-06's classification (which means doing ac-06's work twice — once tentatively in ac-05, again formally in ac-06), or (b) leave the existing Session Completion section completely intact in ac-05 and let ac-06 reshape it. The spec doesn't say which. The two interpretations produce different intermediate commit states and different review surfaces.
**Evidence:** ac-05 references "audit-survivor of 'Session Completion' (per ac-06)". ac-09 sequences ac-05 → ac-06. ac-06 records audit verdicts via `orbit spec note` *before* edits land — implying the classification happens during ac-06, not ac-05.
**Recommendation:** Either swap the order in ac-09 (ac-06 before ac-05, so the audit verdict is recorded and edits to Session Completion happen in ac-06, then ac-05 deletes substrate sections cleanly), or amend ac-05 to explicitly say "leave the existing 'Session Completion' / 'Mandatory Workflow' section untouched — ac-06 handles it". The first is cleaner and matches the pillar of agent-state-persistence (one AC, one well-defined state transition).

### [MEDIUM] STYLE.md propagation to downstream projects is undefined
**Category:** missing-requirement
**Pass:** 2
**Description:** ac-05 fixes STYLE.md treatment *inside the orbit repo* (the `@.orbit/STYLE.md` import stays alongside `@.orbit/METHOD.md`). But it doesn't address what happens for *downstream* projects that run `/orb:setup`. ac-01 says METHOD.md contains "a one-line BLUF / Decision Brief reference pointing at .orbit/STYLE.md as the prose contract" — that reads as a prose pointer, not an `@-import`. So downstream projects get METHOD.md (with a prose pointer) but no STYLE.md file in `.orbit/`. The pointer references a file that doesn't exist. Card 0026 (executive communication) is load-bearing for the BLUF contract; silently dropping it for downstream projects regresses pillar #1 (executive-level interaction) for everyone except the orbit repo itself.
**Evidence:** ac-01 "a one-line BLUF / Decision Brief reference pointing at .orbit/STYLE.md as the prose contract" — wording is "reference", not "@-import". ac-05 only addresses orbit-repo dogfooding. ac-03 setup logic copies METHOD.md into projects but says nothing about STYLE.md. The orbit-repo CLAUDE.md currently has `@.orbit/STYLE.md` as a load-bearing import (line 7).
**Recommendation:** Pick one of three options and pin it in ac-01 or a new AC: (a) `/orb:setup` also copies a canonical STYLE.md template into `.orbit/STYLE.md` and METHOD.md uses `@.orbit/STYLE.md` (transitive import); (b) METHOD.md uses prose-only reference, downstream projects opt in by writing their own STYLE.md (current spec wording — but make this explicit so the prose pointer doesn't reference a non-existent file); (c) METHOD.md inlines a one-paragraph BLUF distillation so the contract travels even without STYLE.md. Option (a) preserves pillar #1; option (c) is the simplest. Option (b) regresses pillar #1 silently and shouldn't be the unstated default.

### [MEDIUM] '## Orbit method' marker heading in ac-03(b) conflicts with ac-05's dogfood shape
**Category:** constraint-conflict
**Pass:** 2
**Description:** ac-03(b) says the `@-import` is appended "under a single '## Orbit method' marker heading". But the orbit repo's existing `@.orbit/STYLE.md` import (CLAUDE.md line 7) sits *inline in the body*, not under a marker heading. ac-05 dogfoods the orbit-repo CLAUDE.md to use the `@-import` pattern but doesn't mention adding a `## Orbit method` heading. So either (a) ac-05 must add the heading to comply with ac-03(b)'s shape (and the existing `@.orbit/STYLE.md` becomes orphaned outside the heading), or (b) the orbit repo's dogfood deliberately diverges from the setup-produced shape, which defeats the dogfooding intent. The spec doesn't pick.
**Evidence:** ac-03(b) "append under a single '## Orbit method' marker heading". CLAUDE.md:7 currently has `@.orbit/STYLE.md` inline (no heading). ac-05 doesn't reference the marker heading.
**Recommendation:** Drop the marker-heading requirement from ac-03(b) — make `@-import` placement structure-agnostic (anywhere in the file, idempotently checked by line equality). The marker heading adds no value once the import line itself is the unique detection key. If a marker is genuinely needed for future migration tooling, use an HTML comment (`<!-- ORBIT-METHOD-IMPORT -->`) so it doesn't pollute the rendered document. Apply the same shape in ac-05's dogfood.

### [MEDIUM] ac-07 smoke test interactivity is undefined
**Category:** test-gap
**Pass:** 2
**Description:** ac-07 asserts behaviour around four interactive prompts: (c) "no prompt fired" on idempotent re-run, (d) "drift prompt fires" after author-edit, (e) "legacy-migration prompt fires" on legacy project, (f) accepting the prompt removes blocks, (g) declining the prompt leaves them. All these require the smoke test to either (i) drive interactive prompts in a subprocess, (ii) use a non-interactive flag like `--yes`/`--no`/`--assume-no`, or (iii) inspect the prompt-spawning code path without actually invoking it. The spec says "test confirms the prompt path exists; doesn't need to interact" for (d), but (f) and (g) explicitly assert the *outcome* of accepting/declining — which requires interaction. The two existing test scripts under `plugins/orb/scripts/tests/` (`test-gate-ac-verification.sh`, `test-promote-gate-propagation.sh`) don't establish a pattern for prompt-driven testing.
**Evidence:** ac-07(d) "test confirms the prompt path exists; doesn't need to interact". ac-07(f) "accepting the prompt removes the legacy blocks". ac-07(g) "declining leaves blocks intact". These three are mutually inconsistent about interactivity.
**Recommendation:** Pin the test pattern in ac-07: either (a) the implementation must accept `--assume-yes` / `--assume-no` flags (or env vars) that bypass prompts, and the test exercises both flags; or (b) all prompt tests assert only the prompt-spawning code path, not the outcome — and (f)/(g) become "code path produces a prompt with the correct text" rather than "outcome of accepting/declining". Option (a) is more honest and forces the implementation to be testable; option (b) is faster but loses confidence.

### [LOW] Pillar attribution missing
**Category:** assumption
**Pass:** 1
**Description:** Project CLAUDE.md "The four pillars" section says "Cards that claim a pillar should be able to defend the claim with a measurable mechanism." The spec doesn't declare which pillar(s) it moves. Card 0017 (the linked card) doesn't reference pillars either. The implicit answer reads as pillar #2 (agent self-learning) and pillar #3 (state-persistence) — METHOD.md is substrate distillation that survives session boundaries. But the spec should name it so the why-test is legible.
**Evidence:** Spec yaml has no pillar reference. Card 0017 has no pillar reference. CLAUDE.md "Cards that claim a pillar should be able to defend the claim with a measurable mechanism."
**Recommendation:** Add a one-line `pillar:` field to the spec (or an annotation in the goal): "primary: agent self-learning (METHOD.md is substrate distillation); secondary: executive-level interaction (single-screen target keeps the contract scannable)". Soft change, but it makes the spec defensible against the project's own why-test.

---

## Honest Assessment

The spec is in good shape after the cycle-2 amendments — those addressed the cycle-1 HIGH cleanly and most of the MEDIUMs. The notes stream documents the changes well. Nine ACs cover canonical template, setup wiring, dogfood, audit, scenario amendment, smoke test, principle note, and ordering — comprehensive.

The biggest residual risk is the decline path of the legacy-migration prompt (HIGH). The spec's purpose is to eliminate the drift state where inline blocks and `@-imports` coexist, but its decline path produces exactly that state. That's not a sharpening issue — it's a behaviour bug in the spec that will ship a regression for users who hesitate at the prompt. Fix this before implementation.

The MEDIUM findings cluster around three real ambiguities: (1) the ac-05/ac-06 ordering forces ac-05 to preview ac-06's classification work; (2) STYLE.md downstream propagation is unaddressed and silently regresses pillar #1 for downstream projects; (3) the `## Orbit method` marker heading vs the orbit-repo's existing inline pattern conflicts with the dogfood goal. Each is a one-line clarification but expensive to derive mid-implementation. The smoke-test interactivity finding is a test-pattern decision that affects the ac-03 implementation surface (whether `--assume-yes` flags are needed).

The LOW pillar-attribution finding is style discipline, not a defect — flagged for completeness against the project's own why-test rule.

Recommend: amend the spec to address the HIGH (decline path) and MEDIUM #1 (ordering) before drive moves to implement; the other MEDIUMs are sharpening, not blocking, but cheaper to bake in now.
