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
| 2 — Assumption & failure | content signals (cross-project distribution, plugin marketplace, dogfood self-application) + Pass 1 finding | 3 |
| 3 — Adversarial | not triggered | — |

All six gate ACs (ac-01, ac-03, ac-04, ac-05, ac-06, ac-09) pass the deterministic gate-description rules (non-empty, no placeholder tokens, all ≥20 chars; lengths range 652–1419 chars). Cycle 1 + 2 findings are addressed per the notes stream — BLUF skeleton inlined, atomic legacy-migration, marker heading dropped, ac-09 ordering swapped (ac-06 → ac-05), STYLE.md handling pinned. The findings below are new — surfaced by reading the cycle-3 spec cold.

---

## Findings

### [HIGH] ac-09 ordering still produces a stale CLAUDE.md window between ac-06 and ac-05
**Category:** constraint-conflict
**Pass:** 2
**Description:** ac-09 sequences ac-06 (Session Completion audit) → ac-05 (orbit-repo CLAUDE.md dogfood). ac-06 says "the audit decisions are recorded via `orbit spec note` **before** the edits land". The phrase "the edits" is ambiguous: it could mean ac-06's own deletions of substrate-shaped Session Completion rules from CLAUDE.md (which would happen during ac-06), OR it could mean the broader CLAUDE.md rewrite that ac-05 performs (which would happen during ac-05). If ac-06 only records the audit and doesn't itself edit CLAUDE.md, then between ac-06 and ac-05 the orbit-repo CLAUDE.md still carries the full Session Completion section unchanged — and ac-05's own description ("any audit-survivor of 'Session Completion' (per ac-06)") implies ac-06 has already produced a survivor set, but doesn't say where that survivor set lives between ACs. The two ACs disagree about who owns the edit. This is the same root issue cycle 2 flagged in different shape — the swap fixed the *direction* but not the *seam*.
**Evidence:** ac-06 "the audit decisions are recorded via `orbit spec note 2026-05-09-orbit-method-md \"<audit verdict>\"` **before** the edits land". ac-05 "any audit-survivor of 'Session Completion' (per ac-06)". ac-09 "ac-06 (Session Completion audit, which determines what survives in orbit-repo CLAUDE.md) → ac-05 (dogfood orbit-repo CLAUDE.md, which depends on … knowing which Session Completion rules survive)". Neither AC says explicitly which one writes the surviving lines into the file.
**Recommendation:** Pick one of two clean shapes and pin it. **Option A (preferred):** ac-06 writes the survivor lines (the 1–3 line git-push block) directly into CLAUDE.md and deletes the substrate-shaped rules; ac-05 then *only* deletes the substrate-shaped sections (vocabulary, decision tree, pipeline, key concepts, four pillars) and inserts `@.orbit/METHOD.md`. The Session Completion section is fully resolved by ac-06; ac-05 leaves it alone. **Option B:** ac-06 produces the audit verdict via `orbit spec note` only — no file edits — and ac-05 makes all CLAUDE.md edits including the Session Completion shrink, citing the ac-06 note. Either way, name which AC mutates which lines of CLAUDE.md so /orb:implement can claim them in turn without overlap.

### [MEDIUM] ac-03(b) atomic-refuse contract leaves the project's `.orbit/METHOD.md` orphaned
**Category:** failure-mode
**Pass:** 2
**Description:** ac-03 sequence: (a) copy METHOD.md to `.orbit/METHOD.md`; (b) legacy-detection — if legacy markers present, prompt; on decline, REFUSE to add the @-import and exit. But (a) has already happened by the time (b) runs. So a user who declines the migration prompt ends up with `.orbit/METHOD.md` written into their project but no `@-import` in CLAUDE.md to load it. The exit message names "the orphan METHOD.md and the recovery command" — so the spec acknowledges this state — but writing a file the user explicitly refused to wire up is a footgun: re-running setup will see `.orbit/METHOD.md` already present and may skip the copy step (depending on idempotency), so the orphan file persists across runs and may diverge from canonical without ever being loaded. The "atomic" framing in the AC text is misleading — the transaction is atomic about the @-import, not about the whole setup operation.
**Evidence:** ac-03 sequence is "(a) copy `plugins/orb/skills/setup/METHOD.md` to `.orbit/METHOD.md`; (b) legacy-CLAUDE.md detection … Decline → REFUSE to add the @-import (atomic — never leave dual-source drift). Exit with one-line message naming the orphan METHOD.md". ac-03(d) "compare the project's `.orbit/METHOD.md` to the canonical via byte-for-byte equality … and prompt before overwriting if they differ" — this prompt fires even though the file was never loaded.
**Recommendation:** Reorder ac-03 so legacy detection happens **before** any filesystem writes: (b') run legacy-detection first; if legacy markers are present and user declines, exit with "no changes made" — no `.orbit/METHOD.md` written. Then (a') copy METHOD.md, then (c') append the @-import. The "atomic — never leave dual-source drift" promise then holds for the whole operation, not just the @-import line. Update ac-07(f) to assert no `.orbit/METHOD.md` is written on the decline path.

### [MEDIUM] ac-07 sub-case labelling mixes letters and numbers, making the test contract hard to parse
**Category:** test-gap
**Pass:** 2
**Description:** ac-07 enumerates scenario (1) with sub-asserts (a)/(b)/(c), scenario (2) with no sub-asserts, scenario (3) with sub-cases (e)/(f). The letter sequence skips (d) entirely — there is no (d). This is almost certainly a leftover from an earlier draft that had a different scenario count. More importantly, ac-04 says "A new scenario is added for the idempotency / drift-detection behaviour described in ac-03(d)" — but ac-03(d) is the byte-for-byte comparison rule, not a numbered sub-case of ac-07. The cross-references between ACs use the same `(a)…(g)` letter scheme for unrelated things (ac-03's a/b/c/d are ordered steps; ac-07's a/b/c/e/f are assertions; ac-04 references "ac-03(d)" meaning the comparison rule). A reviewer or implementer reading "the prompt fires (per ac-07(d))" cannot tell which (d) is meant.
**Evidence:** ac-07 letter sequence reads `(a)(b)(c)` then jumps to `(e)(f)`. ac-03 has `(a)(b)(c)(d)`. ac-04 references "ac-03(d)" as the drift-detection behaviour. The two letter schemes collide.
**Recommendation:** Renumber ac-07's sub-cases as `7.1.a/7.1.b/7.1.c`, `7.2`, `7.3.a/7.3.b` (or any non-colliding scheme — `i/ii/iii` per scenario, etc.). At minimum, fix the missing (d) — either close the gap (relabel `e→d`, `f→e`) or document why the scenario count justifies a skip. Same treatment for ac-03 if cross-references stay — prefer `ac-03.step1/2/3/4` over `ac-03(a)/(b)/(c)/(d)`.

### [LOW] ac-04 doesn't say whether the `relations:feeds` edge to 0028 also adds a `pillars:` field
**Category:** missing-requirement
**Pass:** 1
**Description:** ac-04 requires "A `relations:feeds` entry pointing at card 0028 (four pillars) is added with reason naming pillar 2 (agent self-learning)". Card 0028's goal mentions "cards may declare pillar contribution via existing relations:feeds edges, optional and emergent at distill time" — so `relations:feeds` is the canonical mechanism. Good. But card 0017 may also benefit from a top-level `pillars: [2]` shorthand if other cards in the repo use it (some do — worth checking). The spec doesn't say. This is a minor documentation-shape concern; it doesn't block implementation, but it could create a follow-up "shouldn't 0017 use the shorthand too" review comment later.
**Evidence:** ac-04 specifies only `relations:feeds`. Card 0028's goal explicitly names `relations:feeds` as the canonical mechanism. No mention of any `pillars:` shorthand field.
**Recommendation:** Add a one-line note to ac-04: "If other cards in this repo use a top-level `pillars:` field as shorthand, mirror that pattern for 0017; otherwise `relations:feeds` to 0028 is sufficient." Or close the question by inspecting an existing card and pinning the convention. Either way, name the convention so the implementer doesn't have to re-derive it.

---

## Honest Assessment

The spec is structurally sound and the cycle-2 amendments addressed the substantive concerns from the prior reviews — BLUF inlined, marker heading dropped, ordering swapped, atomic semantics declared. The remaining HIGH finding is a seam problem between ac-06 and ac-05: who actually edits the Session Completion lines in CLAUDE.md? Both ACs gesture at the work but neither owns it. /orb:implement will hit this seam and either do the work twice or leave a stale window between the two ACs. Pin which AC mutates which lines and the spec is implementable end-to-end.

The MEDIUM ordering issue in ac-03 (write-then-prompt) is the second-biggest risk — it produces an orphan `.orbit/METHOD.md` on decline that subsequent runs may treat as canonical-but-stale. Cheap to fix by reordering steps within ac-03.

The LOW and the test-labelling MEDIUM are polish — worth fixing but not blockers in their own right.

Biggest risk if shipped as-is: implementing agent makes its own call about who edits Session Completion (ac-06 vs ac-05), commits intermediate state that doesn't match what either AC describes, and the PR review finds the seam after the fact. One spec edit prevents the rework.
