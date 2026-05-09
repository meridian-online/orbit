# Spec Review

**Date:** 2026-05-09
**Reviewer:** Context-separated agent (fresh session)
**Spec:** 2026-05-09-drive-rally-sidecar-layout
**Verdict:** REQUEST_CHANGES

---

## Review Depth

| Pass | Triggered by | Findings |
|------|-------------|----------|
| 1 — Structural scan | always | 2 |
| 2 — Assumption & failure | content signal: cross-system substrate boundary (canonicaliser/index/verify) + bundled-doc duplication uncovered in Pass 1 | 1 |
| 3 — Adversarial | not triggered | — |

## Cycle-2 status

All seven cycle-2 findings are addressed in the v3 spec text:

| v2 finding | Resolution |
|---|---|
| HIGH — rally cites `<child-spec-id>/drive.yaml` | AC-01 widened: `(b) /orb:rally SKILL.md — every reference to a child spec's drive sidecar`; grep is now `grep -rn -E ... plugins/orb/` (recursive, plugin-wide). |
| MEDIUM — METHOD.md vocabulary table | AC-08 (c) adds METHOD.md rewrite explicitly, with the substantive check that "no row of the METHOD.md vocabulary table cites the bd-era folder form". |
| MEDIUM — heartbeat prompt body | AC-01 calls out "the embedded CronCreate heartbeat prompt body" as a covered surface. |
| MEDIUM — "12 existing folders" off-by-one | AC-06 now says "the 11 existing folders (10 drive-folders + 1 rally-folder)". |
| LOW — AC-07 smoke test scope | AC-07 now covers (a) drive resumption, (b) rally resumption, (c) `orbit verify` clean, (d) `orbit spec list` does not surface sidecars. |
| LOW — gate-AC deterministic check | All six gates (ac-00..ac-05) pass — descriptions range ~210–770 chars, none placeholder. |
| LOW — AC-09 dogfood ordering | AC-09 now mandates pre-nomination "BEFORE the SKILL.md edits land (ac-01..ac-05 done) — this pre-nomination is the implementer's first action on this spec". |

## Findings

### [MEDIUM] `plugins/orb/skills/setup/METHOD.md` is a bundled copy of `.orbit/METHOD.md` — AC-08 does not cover it
**Category:** missing-requirement
**Pass:** 1
**Description:** `plugins/orb/skills/setup/METHOD.md` is byte-identical to `.orbit/METHOD.md` today (`diff` reports "Files are identical"). It ships in the orb plugin and is what `/orb:setup` writes into greenfield projects' `.orbit/METHOD.md` slot. AC-08 (c) mandates `.orbit/METHOD.md`'s vocabulary table is rewritten to sidecar form, but the bundled copy under `plugins/orb/skills/setup/` is silent in every AC. The two will silently drift after ship: the in-repo doc is sidecar-form, the doc shipped to new projects via `/orb:setup` is folder-form.

The bundled copy contains all three folder-form rows:
- Line 25: `<date>-<slug>/interview.md`
- Line 27: `<date>-<slug>/drive.yaml`
- Line 28: `<date>-<slug>-rally/rally.yaml`

Of these, only the **drive** line is incidentally caught by AC-01's `grep -rn -E '<[^>]+>/drive\.yaml|\$[A-Z_]+/drive\.yaml' plugins/orb/` — that grep IS plugin-wide. Good. But:

- AC-03's grep is **file-scoped to rally SKILL.md** ("A grep **of the file**" — singular, the rally file). The bundled copy's rally line escapes.
- AC-01 doesn't cover `interview.md` (out of scope by goal) or `rally.yaml`.
- AC-08 names only `.orbit/METHOD.md`, not the bundled copy.

So when this spec ships, `plugins/orb/skills/setup/METHOD.md` will have rows 25 and 28 in folder-form, row 27 in sidecar-form — internally inconsistent — and any project bootstrapped via `/orb:setup` after this spec receives the partially-stale doc. This is the same drift mode that AC-08 was added to prevent for `.orbit/METHOD.md`.

**Evidence:**
- `diff /home/hugh/github/hughcameron/orbit/.orbit/METHOD.md /home/hugh/github/hughcameron/orbit/plugins/orb/skills/setup/METHOD.md` reports "Files are identical".
- `grep -nE '<rally-id>/rally\.yaml|\$RALLY_ID/rally\.yaml|\*-rally/rally\.yaml' plugins/orb/skills/setup/METHOD.md` returns zero hits — AC-03's grep does not catch line 28's `<date>-<slug>-rally/rally.yaml` form.
- AC-08 description names "`.orbit/METHOD.md`'s vocabulary table" specifically; no mention of the bundled copy.

**Recommendation:** Add a sub-bullet to AC-08: "(d) `plugins/orb/skills/setup/METHOD.md` mirrors the rewrite of `.orbit/METHOD.md` — both files contain the same vocabulary table and must agree row-for-row. Verification: `diff .orbit/METHOD.md plugins/orb/skills/setup/METHOD.md` exits zero." Trivial to enforce, prevents the drift, and codifies the existing (verified) byte-identical relationship.

---

### [MEDIUM] AC-07 smoke test invokes "snippets" that are not standalone callables
**Category:** test-gap
**Pass:** 2
**Description:** AC-07 step (a) says "invoke /orb:drive's resumption-detection snippet, confirm the drive is found"; step (b) says "invoke /orb:rally's resumption scan, confirm the rally is found". These "snippets" are bash blocks embedded in SKILL.md prose — not standalone scripts the smoke test can `bash -c`. Concretely:

- Drive's resumption snippet (`drive/SKILL.md` §Input contract): a `while read` loop that pipes `orbit spec list` through a `[[ -f ".orbit/specs/$sid/drive.yaml" ]]` test. After ac-05, the path becomes `$sid.drive.yaml` — but the snippet still lives in markdown prose.
- Rally's resumption snippet (`rally/SKILL.md` §Resumption): a `for f in .orbit/specs/*-rally/rally.yaml` glob loop. After ac-03 (d), the glob becomes `.orbit/specs/*.rally.yaml`.

The smoke test author has three options: (i) maintain a hand-copied duplicate of each snippet inside the test (fragile — drifts the moment the SKILL.md snippet changes), (ii) extract the snippet to a callable script and have the SKILL.md reference it (out of scope for this spec — but the cleanest fix), or (iii) execute the snippet via the skill itself (requires harness invocation, heavier than a bash smoke test). AC-07 doesn't say which, and option (i) — the path of least resistance — is exactly the failure mode that defeats the test's purpose.

**Evidence:**
- `plugins/orb/skills/drive/SKILL.md:46-48` — resumption-detection snippet is markdown prose inside `\`\`\`bash` fences.
- `plugins/orb/skills/rally/SKILL.md:55-60` — resumption scan is markdown prose.
- AC-07 description: "invoke ... snippet" — no callable surface specified.

**Recommendation:** Pick one of two tightenings. Either (a) require the resumption-detection snippets be extracted to callable scripts under `plugins/orb/scripts/` (e.g. `drive-resume-detect.sh`, `rally-resume-detect.sh`) — turns the snippet into a single source of truth — and AC-07 invokes those scripts; or (b) explicitly accept that the smoke test re-implements the path-existence check ("[[ -f .orbit/specs/$sid.drive.yaml ]]" / `for f in .orbit/specs/*.rally.yaml`) and tests the path *shape*, not the snippet itself, and document that the smoke test does not catch SKILL.md-internal regressions to the snippet body. Option (a) is the right answer; option (b) ships a less ambitious test honestly.

---

### [LOW] AC-04's section numbering (§1.1, §1.3, §3.1, §3.2) is a brittle reference
**Category:** test-gap
**Pass:** 1
**Description:** AC-04 (a) cites specific drive SKILL.md sections — "§1.1, §1.3, §3.1, §3.2" — as the surfaces that cite review-file paths. These section numbers are not stable identifiers; they're prose headings that will renumber if drive SKILL.md is restructured. The substantive verification is the path content, not the section number. The section numbers are belt-and-braces locator information for the implementer, but if the document reflows between cycle 3 and the implement step, the AC reads as "verify these four sections cite sidecar paths" against sections that no longer exist. The grep-style verifications in AC-01 and the broader path checks elsewhere in AC-04 cover the actual content. The section-number callouts are decorative.

**Evidence:**
- AC-04 description: "/orb:drive SKILL.md §1.1, §1.3, §3.1, §3.2 cite the sidecar paths".
- No grep or other content-based check tied to these section numbers — the grep-equivalent for AC-04 is the implicit "every review-file reference uses sidecar form", but it's not stated as a verification command.

**Recommendation:** Add a verification grep to AC-04: `grep -nE '\\.orbit/specs/<[^>]+>/review-(spec|pr)-' plugins/orb/skills/drive/SKILL.md plugins/orb/skills/review-spec/SKILL.md plugins/orb/skills/review-pr/SKILL.md` returns zero hits. Drop or demote the §1.1/§1.3/§3.1/§3.2 callouts to "(approximately at the cycle-1 location of these sections)" — they're useful pointers, not contract surface. The grep is the contract.

---

## Honest Assessment

This is a tight cycle-3 revision. Every cycle-2 finding is addressed in the spec text — most cleanly: AC-01's grep is now plugin-wide; AC-08 (c) makes METHOD.md non-optional; AC-07 covers all four scanner-fix surfaces; AC-09's pre-nomination is now sequencing-enforced rather than self-reported. The implementation order (ac-00 first) is explicit in ac-00 itself.

The remaining gaps are second-order: a bundled copy of METHOD.md ships with the orb plugin and AC-08 doesn't cover it (silent drift to greenfield projects); AC-07's smoke test invokes snippets that aren't standalone callables (the implementer has to choose how to test them); and AC-04 cites section numbers that may renumber. The first of these is the substantive one — `setup/METHOD.md` is the agent-facing doc that ships to every new orbit project, and shipping it half-migrated re-introduces the same drift this spec exists to prevent. Worth a one-line AC sub-bullet.

The HIGH-severity bar is not crossed in v3. Once AC-08 (d) lands and AC-07's snippet-invocation strategy is named, this spec is implementable in one drive session with deterministic verifications. Recommend REQUEST_CHANGES → tighten AC-08 (add bundled-copy sub-bullet), AC-07 (name the snippet-invocation contract), and optionally AC-04 (replace section numbers with a grep) — then approve.
