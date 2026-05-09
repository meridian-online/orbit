# Spec Review

**Date:** 2026-05-09
**Reviewer:** Context-separated agent (fresh session)
**Spec:** 2026-05-09-drive-rally-sidecar-layout
**Verdict:** REQUEST_CHANGES

---

## Review Depth

| Pass | Triggered by | Findings |
|------|-------------|----------|
| 1 — Structural scan | always | 4 |
| 2 — Assumption & failure | content signal: cross-system substrate boundary (canonicaliser/index/verify); MEDIUM findings from Pass 1 | 2 |
| 3 — Adversarial | structural concern: spec-implied surface edits (rally sub-agent brief, METHOD.md, drive heartbeat prompt) live outside any AC's grep test | 1 |

## Findings

### [HIGH] Rally SKILL.md still references `<child-spec-id>/drive.yaml` — no AC catches it
**Category:** missing-requirement
**Pass:** 1
**Description:** Three lines in rally SKILL.md cite the folder-form drive sidecar path for child specs:
- Line 565: `Resume from .orbit/specs/<child-spec-id>/drive.yaml stage`
- Line 572: `update your own .orbit/specs/<child-spec-id>/drive.yaml as drive normally does`
- Line 824: `.orbit/specs/<child-spec-id>/drive.yaml's stage field`

These are **drive sidecars referenced from the rally surface**. AC-01's grep is scoped to /orb:drive SKILL.md only (`grep of the file` — singular, the drive file). AC-03's grep targets `<rally-id>/rally.yaml`, `$RALLY_ID/rally.yaml`, `*-rally/rally.yaml` — none of which match `<child-spec-id>/drive.yaml`. The implementing agent can run AC-01 and AC-03 grep checks, get clean results on both, and ship the spec while leaving rally's child-drive references stale. Cascade: a rally sub-agent following the brief will try to read `.orbit/specs/<child-spec-id>/drive.yaml` (folder form, no longer exists per ac-06's "new drives use sidecar") and fail.

**Evidence:**
- `plugins/orb/skills/rally/SKILL.md:565,572,824` — three folder-form references to drive sidecars on the rally surface.
- AC-01 description: "/orb:drive SKILL.md is updated … A grep **of the file**" — file-scoped to drive.
- AC-03 description: rally grep patterns target `rally.yaml` only, not `drive.yaml`.

**Recommendation:** Widen AC-01's grep to cover both /orb:drive SKILL.md AND /orb:rally SKILL.md (since both surfaces cite the drive sidecar path) — OR add a new sub-bullet to AC-03 explicitly covering the rally-side drive sidecar references and tightening its grep to also include `<child-spec-id>/drive.yaml`. The simplest formulation: "AC-01 grep extends to any plugin file that mentions the drive sidecar path — `grep -rn '<.*>/drive\.yaml\|\$.*/drive\.yaml' plugins/orb/` returns zero hits."

---

### [MEDIUM] METHOD.md rows 25, 27, 28 still cite folder layout — orphaned canonical statement
**Category:** missing-requirement
**Pass:** 1
**Description:** AC-08 names `.orbit/conventions/spec-layout.md` as the new canonical home and updates the in-source comment in `orbit-state/crates/core/src/layout.rs`. But the project's own `.orbit/METHOD.md` — the load-bearing methodology doc imported into CLAUDE.md and read at every session start — has a vocabulary table whose rows for Interview, Drive state, and Rally state still cite folder layout (lines 25, 27, 28). Row 26 (Review) already uses the sidecar form `<date>-<slug>.review-{spec,pr}-<date>.md`, so the table is internally inconsistent today. After this spec ships, METHOD.md will be the authoritative agent-facing doc that disagrees with both `spec-layout.md` and the in-source comment. Drift surfaces immediately on the first session that primes from METHOD.md.

**Evidence:**
- `.orbit/METHOD.md:24-28` — vocabulary table: row 26 sidecar; rows 25/27/28 folder.
- `CLAUDE.md` imports METHOD.md (`@.orbit/METHOD.md`) — every session loads this verbatim.
- AC-08 description: lists "spec-layout.md" and "in-source convention comment in layout.rs" — silent on METHOD.md.

**Recommendation:** Add a sub-bullet to AC-08: "`.orbit/METHOD.md`'s vocabulary table (rows for Interview, Drive state, Rally state) is rewritten to sidecar form — row 27 becomes `.orbit/specs/<date>-<slug>.drive.yaml`, row 28 becomes `.orbit/specs/<date>-<slug>.rally.yaml`, row 25 (Interview) is either updated to sidecar or explicitly noted as a remaining folder artefact (interviews aren't covered elsewhere in this spec — pick a stance)." Without this, the canonical statement lives in three places (METHOD.md, spec-layout.md, layout.rs comment) which immediately disagree.

---

### [MEDIUM] Drive heartbeat prompt body cites `<spec-id>/drive.yaml` — embedded in CronCreate string
**Category:** failure-mode
**Pass:** 2
**Description:** The drive heartbeat prompt at `drive/SKILL.md:117-138` is the body of a `CronCreate` payload — i.e. it is a string that drive injects into a recurring agent invocation, not just SKILL.md prose. Inside the body: line 119 reads `Read .orbit/specs/<spec-id>/drive.yaml and read its stage`. AC-01's grep on /orb:drive SKILL.md WILL flag this line (it's literal text in the file), so the implementing agent will find and fix it. But the failure mode here matters: if they fix the prose mention without updating the embedded prompt body, every running heartbeat spawns an agent that tries to read a non-existent path. Worth calling out explicitly so the implementing agent treats the prompt body as code, not narrative.

**Evidence:**
- `drive/SKILL.md:117-138` — CronCreate prompt body contains `.orbit/specs/<spec-id>/drive.yaml` at line 119.
- AC-01 grep WILL match (literal-string check on the file).
- Resumption: the heartbeat body, once injected via CronCreate, is opaque to the SKILL.md surface — old heartbeats with the stale body will be running until they self-terminate or are CronDelete'd.

**Recommendation:** No new AC needed (AC-01 catches it), but add a one-line implementation note to the spec: "the heartbeat prompt body at lines 117-138 is a code surface — when AC-01 rewrites `<spec-id>/drive.yaml`, ensure the prompt body inside the CronCreate block is rewritten too." Also flag in the drive smoke test (AC-07) that running heartbeats from before the migration will fail until CronDelete'd.

---

### [MEDIUM] AC-06's "12 existing folders" is wrong by one
**Category:** test-gap
**Pass:** 1
**Description:** AC-06 says the policy is chosen "to avoid touching the 12 existing folders at migration time". `find .orbit/specs -maxdepth 2 -name 'drive.yaml' -o -name 'rally.yaml'` returns 11 matches today: 10 drive-folders + 1 rally-folder. The discrepancy is small but the AC text becomes a future evidence trail — a reader who runs the find command and gets 11 will wonder what changed. More substantively: AC-06's verification ("`orbit verify` returns clean across the repo's `.orbit/specs/` directory") depends on AC-00 having shipped the scanner-fix. AC-06's gate is `false`, but it's a verification dependency, not an implementation dependency — fine, but worth tightening.

**Evidence:**
- `find /home/hugh/github/hughcameron/orbit/.orbit/specs -maxdepth 2 -name 'drive.yaml'` returns 10 directories.
- `find /home/hugh/github/hughcameron/orbit/.orbit/specs -maxdepth 2 -name 'rally.yaml'` returns 1 directory.
- AC-06 description: "the 12 existing folders" — off by one.

**Recommendation:** Update AC-06 description: "the 11 existing folders (10 drive-folders + 1 rally-folder)". Trivial fix, but the spec's evidence trail should be accurate. No change to verification logic needed.

---

### [LOW] AC-07 smoke test scope is thin — only covers drive resumption-detection round-trip
**Category:** test-gap
**Pass:** 2
**Description:** AC-07 says the smoke test "invokes promote.sh, writes a drive.yaml at the sidecar path, then invokes /orb:drive's resumption-detection snippet and confirms the drive is found". This proves the new path is detected. It does NOT prove: (a) `orbit verify` returns clean with sidecar yaml on disk (AC-00's claim), (b) `orbit spec list` does not surface the sidecar as a spec id (AC-00's claim), (c) rally's resumption scan iterates `*.rally.yaml` correctly (AC-03 (d)), (d) the existing folder layouts are excluded by the scanner-fix (AC-06's verification). The smoke test is a useful check but markets itself as broader than it is.

**Evidence:**
- AC-07 description: scope is drive-resumption only.
- AC-00 unit test (described in AC-00) covers verify + spec list at the unit level — but no integration test combines them.
- AC-03 (d), AC-06 verification: no integration-level test.

**Recommendation:** Either widen AC-07 to include rally-side sidecar detection AND a check that `orbit verify` returns clean with both shapes on disk; or rename it to "drive smoke test" and explicitly document the gaps. Option (a) is cheap — adding a rally.yaml sidecar to the same temp `--root` and calling rally's resumption scan adds maybe 10 lines of test code.

---

### [LOW] Gate-AC description check (deterministic)
**Category:** missing-requirement
**Pass:** 1
**Description:** Pass-1 deterministic check on every gate AC. ACs 00–05 are gates (parser column-4 = `1`); all six descriptions are non-empty, none are placeholder tokens, all are well over 20 characters (range ~210 to ~770 chars). No findings.

**Evidence:** Parser output column-4 is `1` for ac-00..ac-05; descriptions range from ~210 to ~770 characters. None match the placeholder token set.

**Recommendation:** None.

---

### [LOW] AC-09 dogfood verification language could leak risk back into the test
**Category:** test-gap
**Pass:** 3
**Description:** AC-09 says "the candidate card or spec is pre-nominated in a spec note before this AC is verified, so the dogfood does not entangle with unrelated card-quality risk". Sound intent. But the AC doesn't mandate that the pre-nomination has happened before AC-09 can be marked done — only that pre-nomination is "before this AC is verified". A drive could ship the migration, mark AC-09 done by writing the integration-result note, and the pre-nomination note never lands (or lands afterward, defeating the variance-reduction). The spec note `2026-05-09T10:22:04` ("ac-09 dogfood candidate") has the right shape but doesn't pre-nominate a specific spec or card — it states the criteria for selection.

**Evidence:**
- AC-09 description: "pre-nominated … before this AC is verified".
- Spec note `2026-05-09T10:22:04`: states selection criteria, doesn't name a candidate.

**Recommendation:** Add to AC-09: "the pre-nomination note must exist BEFORE the SKILL.md edits land (ac-01..ac-05 done) — implementer adds the candidate's id to a spec note as the first action of this spec, before any code or doc edits." This forces the variance-reduction to actually happen rather than being a self-reported intent.

---

## Honest Assessment

This is a strong cycle-2 revision. Cycle 1's three biggest findings — the missing scanner AC, the rally folder migration story, and the canonical-doc target — are all resolved cleanly: AC-00 names the scanner fix and the verify+index consumers; AC-03 widens to cover folder collapse, input contract, and resumption scan; AC-08 nominates `.orbit/conventions/spec-layout.md` as the canonical home. The implementation-ordering note (`2026-05-09T10:21:53`) makes the AC-00-first dependency explicit. AC-04 now covers the inline-invocation defaults in review-spec/SKILL.md and review-pr/SKILL.md. Good cycle.

The remaining gaps are second-order. The HIGH finding (rally's `<child-spec-id>/drive.yaml` references) is a scope hole — three lines in rally SKILL.md that no AC's grep targets, exactly the failure mode AC-01 and AC-03 were written to prevent. Fix: widen AC-01's grep to cover all of `plugins/orb/`, or extend AC-03. The METHOD.md finding is similar: an authoritative surface left out of AC-08. The remaining MEDIUM/LOW findings are accuracy fixes (AC-06's "12 → 11"), implementation hints (heartbeat prompt body), and tightening (AC-07 scope, AC-09 ordering).

Once the HIGH and the two scope MEDIUMs (AC-08 widening, AC-01 widening or AC-03 sub-bullet) land, the spec is implementable in one driving session with no ambiguity. Recommend REQUEST_CHANGES → tighten ACs 01 (or 03), 06, 08; small note on heartbeat prompt body and AC-09 ordering — then approve.
