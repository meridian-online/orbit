# Pre-Merge Review

**Date:** 2026-05-09
**Reviewer:** Context-separated agent (fresh session)
**Branch:** drive/orbit-method-md
**Spec:** 2026-05-09-orbit-method-md
**Verdict:** APPROVE

---

## Test Results

| Check | Result | Details |
|-------|--------|---------|
| Test suite | PASS | `plugins/orb/scripts/tests/test-setup-method.sh` — all 4 scenarios green (fresh, drift-prompt, legacy-accept, legacy-refuse) |
| AC coverage | 9/9 | See report below; non-code ACs (template / audit-note / spec-note / dogfood / order-spec) are doc/gate-shaped, not test-shaped |
| Byte-equality of canonical vs project METHOD.md | PASS | `cmp .orbit/METHOD.md plugins/orb/skills/setup/METHOD.md` exits 0 |
| Single `@.orbit/METHOD.md` import in dogfood CLAUDE.md | PASS | exactly 1 line; sits alongside `@.orbit/STYLE.md` per ac-05 |
| Legacy markers absent in dogfood CLAUDE.md | PASS | grep for `^## (Workflow \(orbit\)|Orbit vocabulary|Current Sprint)$` returns 0 hits |
| Edge case: missing CLAUDE.md | PASS | script creates CLAUDE.md with single `@.orbit/METHOD.md` line |
| Edge case: pre-existing @-import on first run | PASS | idempotent — count stays at 1 |

## AC Coverage Report

| AC | Status | Test(s) / evidence |
|----|--------|--------------------|
| ac-01 | doc-shaped (gate) | `plugins/orb/skills/setup/METHOD.md` lands all required sections in the declared order: one-line "what orbit is" → pipeline (memo→…→ship; drive/rally variants) → vocabulary table (card/memo/choice/spec/interview/review/drive/rally state) → card-vs-choice-vs-spec-vs-memo decision tree → substrate rules → four pillars → BLUF skeleton inlined verbatim (TL;DR / Recommendation / Why / Detail / Confidence). 73 lines, single screen. |
| ac-02 | doc-shaped | Top-of-file "How to update" line present at line 1 of `plugins/orb/skills/setup/METHOD.md` and participates in byte comparisons (smoke scenarios 1.t1 and 3a.t1.method confirm). |
| ac-03 | covered (gate) | Smoke scenario 1 (fresh non-interactive), 2 (drift-prompt fires), 3a (legacy accept), 3b (legacy refuse atomic). Detect-before-write sequencing verified by 3b — no orphan `.orbit/METHOD.md` after refuse. SKILL.md §6 rewritten to match script semantics; old Snippet/Case A/B/C block removed. |
| ac-04 | doc-shaped (gate) | Card 0017 amended: greenfield `then` clause now reads "writes `.orbit/METHOD.md` and ensures CLAUDE.md @-imports it"; new "METHOD.md drift triggers prompt on re-run" scenario added; new "Legacy CLAUDE.md blocks trigger atomic migrate-or-refuse prompt" scenario added; `relations:feeds` → `0028-four-pillars` with reason naming pillar 2. No parallel `pillars:` field — relations are the canonical wire (consistent with choice 0019). Maturity stays `planned`. |
| ac-05 | dogfood (gate) | `CLAUDE.md` shrinks 99 → 32 lines; substrate sections (Key Concepts, Orbit vocabulary, Orbit-state Substrate quick ref, Session Completion mandatory workflow) deleted and replaced by `@.orbit/METHOD.md`; `@.orbit/STYLE.md` import retained alongside; "# orbit" intro / Working in This Repo / Deployment kept; new tight 4-line "Push discipline" block lands per ac-06's verdict. |
| ac-06 | spec-note (gate) | Audit verdict recorded as spec note (timestamp 2026-05-09T04:24:45Z) — rule-by-rule classification covers (i) substrate-shaped → delete, (ii) git/CI discipline → keep tight block, (iii) stale → delete. ac-05's edits map cleanly to the verdict. |
| ac-07 | covered | `plugins/orb/scripts/tests/test-setup-method.sh` — three scenarios with sub-asserts t1/t2/t3 as required; runs green; sub-label scheme `t1/t2/t3` distinct from ac-03's `a/b/c/d`. |
| ac-08 | spec-note | Principle note recorded (timestamp 2026-05-09T04:28:30Z) — orbit-shaped vs project-specific discrimination rule captured before close. |
| ac-09 | order-spec (gate) | Implementation order declarative: ac-01 → ac-02 → ac-03 → ac-06 → ac-05 → ac-04 → ac-07 → ac-08. Notes-jsonl sequence is consistent with the declared order (ac-06's audit note precedes ac-05's CLAUDE.md edits). |

Coverage: 2/9 ACs are code-shaped (ac-03, ac-07) — both have direct test coverage. Remaining 7 ACs are template/doc/dogfood/spec-note/order-spec by their description text and are not expected to carry `acNN_*` test functions; the smoke test exercises ac-03's behaviour end-to-end and indirectly validates ac-01's content (byte-equality assertion) and ac-02's "How to update" line (lives inside the byte-compared file).

## Findings

None at CRITICAL or MEDIUM. Two LOW observations recorded for completeness:

### [LOW] Drift-prompt copy-paste-equality is implicit, not asserted
**Category:** test-gap
**Description:** The drift-prompt scenario (smoke test scenario 2 t1) confirms the prompt fires and that decline keeps local edits, but does not directly assert the canonical-vs-project byte difference that triggered the prompt. If a future regression caused the script to fire the prompt unconditionally (e.g. `cmp` removed), the test would still pass.
**Evidence:** `plugins/orb/scripts/tests/test-setup-method.sh:79-101` — pipeline runs setup once, mutates `.orbit/METHOD.md`, re-runs, asserts the "differs from the canonical" line and that local edit survived.
**Recommendation:** Optional: add a sub-assert that the unmodified path (re-run without mutation, scenario 1 t3) produces no "differs from the canonical" line in the output. Not blocking.

### [LOW] `setup-method.sh` uses an open-then-overwrite pattern for the legacy strip
**Category:** edge-case
**Description:** The python3 block at `setup-method.sh:114-147` reads CLAUDE.md and writes it back without a tempfile-rename. If interrupted mid-write, the project CLAUDE.md could be truncated. Practical risk is small (atomic strip is short-lived and only runs on legacy-accept), but the rest of the script is shell-careful so this stands out.
**Evidence:** `plugins/orb/scripts/setup-method.sh:114-147` — `with open(path, 'w')` after read; no temp file or rename.
**Recommendation:** Optional follow-up spec: write to `${path}.tmp` then `os.rename`. Not blocking — the operation is short and the author is interactive at the prompt.

---

## Honest Assessment

This implementation is unusually clean for a 9-AC drive that touched the canonical agent-priming surface and dogfooded itself. The substrate-vs-project-discipline split lands as designed: `.orbit/METHOD.md` reads as a single-screen substrate distillation; `CLAUDE.md` shrinks to repo intro + skill-paths + deployment + a tight 4-line push block + two `@-imports`; the `cmp` byte-equality primitive is shared between the script (drift detection) and the smoke test (canonical fidelity). The audit-verdict / edit-execution seam between ac-06 and ac-05 holds — the spec-notes show the verdict was recorded before the edits landed, and the resulting CLAUDE.md matches the verdict's three-bucket classification rule-for-rule.

Of the 9 ACs, 7 are doc-, dogfood-, spec-note-, or order-spec-shaped — for those the artefact is the test, and the byte-equality assertion in the smoke test indirectly covers ac-01 (template content) and ac-02 (top-of-file line). The two genuinely code-shaped ACs (ac-03 setup script, ac-07 smoke test) both have direct end-to-end coverage, and the smoke test runs green on a fresh execution. Edge cases probed (missing CLAUDE.md, pre-existing @-import) all pass. The two LOW findings are quality-of-life, not correctness — neither blocks merge. The downstream pillar-2 wire (a setup that primes new agents with the substrate) is now real, and the orbit repo itself proves the wire works by consuming its own output.
