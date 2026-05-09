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
| 2 — Assumption & failure | content signal: cross-system substrate boundary (canonicaliser/index/verify scan `.orbit/specs/*.yaml`); MEDIUM finding from Pass 1 | 3 |
| 3 — Adversarial | structural concern: substrate-scanner extension is a cross-cutting prerequisite to ACs 01/03/09 | 2 |

## Findings

### [HIGH] Substrate scanner blocks the sidecar layout — no AC covers the fix
**Category:** missing-requirement
**Pass:** 1
**Description:** The author already discovered this and recorded it in spec note 2026-05-09T04:09:09 ("Engineering finding"): `orbit verify` and the index rebuild scan `.orbit/specs/*.yaml` and parse every match as a `Spec`. Files at `.orbit/specs/<id>.drive.yaml` and `.orbit/specs/<id>.rally.yaml` will fail parse with "unknown field, expected one of id, goal, cards, status, labels, acceptance_criteria". I confirmed this by reading `orbit-state/crates/core/src/layout.rs:114-146` (`list_spec_files` → `list_yaml_files` filters only on `.yaml` extension, no path-shape check) and `orbit-state/crates/core/src/verify.rs:99-103` and `index.rs:166-189` (both consume `list_spec_files()` and parse each as `Spec`). The spec note acknowledges this is "the crux of the migration" and says "Add an AC covering this when implementing" — but the spec went out without that AC. ACs 01, 03, and 09 cannot pass while the canonicaliser rejects the new files: implementing the path changes without first teaching the scanner about sidecars will brick `orbit verify` for every spec that has a drive or rally.

**Evidence:**
- Spec note `2026-05-09T04:09:09.233534107Z` on this spec's notes.jsonl explicitly flags the gap.
- `orbit-state/crates/core/src/layout.rs:132-146` — `list_yaml_files` accepts every `*.yaml` under the dir.
- `orbit-state/crates/core/src/verify.rs:101-103` — verify treats every match as a `Spec`.
- `orbit-state/crates/core/src/index.rs:166-189` — index rebuild does the same; failure here surfaces as `index rebuild failed` in `orbit verify` output.
- Drive's own state file `.orbit/specs/2026-05-09-drive-rally-sidecar-layout/drive.yaml` notes "Folder layout used for drive.yaml because the canonicaliser parses .orbit/specs/<id>.drive.yaml as a malformed spec — exactly the bug this spec migrates away from."

**Recommendation:** Add an AC (insert as a new gate before AC-01, e.g. `ac-00` or renumber): the substrate scanner must filter spec YAML loads to `<id>.yaml` only — i.e. exclude any path whose stem contains a `.` (covers `<id>.drive.yaml`, `<id>.rally.yaml`, `<id>.review-spec-<date>.md` is already non-yaml so unaffected by that filter, but the implicit assumption needs codifying). Concretely, change `list_yaml_files`'s extension check to also require the stem to be a single dotless token, or split into `list_spec_yaml_files` that excludes sidecar shapes. Cover both `verify_all` and `Index::rebuild_from_files` — there is no point fixing one and not the other. Add a unit test in `orbit-state/crates/core/src/verify.rs` that creates a `<id>.drive.yaml` and confirms it is *not* parsed as a Spec. This is the load-bearing change; without it, the Skill.md edits in ACs 01/03 cannot be exercised in the dogfood test (AC-09).

---

### [MEDIUM] Rally folder convention has no migration story
**Category:** constraint-conflict
**Pass:** 1
**Description:** The rally SKILL.md uses a folder-as-rally convention that goes deeper than "rally.yaml lives in a folder". `RALLY_DIR=".orbit/specs/$(date -I)-${SLUG}-rally"` (rally SKILL.md:214) creates the folder; child specs are created inside it (lines 232–239 surrounding context); resumption iterates `.orbit/specs/*-rally/rally.yaml` (line 57); the input contract takes a "rally folder" as a CLI argument (lines 14, 49, 794, 797, 831). AC-03 only mandates renaming the path of `rally.yaml` itself — it is silent on whether the rally folder still exists, where child specs live, what the resume scan iterates, and what the "rally folder provided" branch of the input contract receives. If the rally folder is collapsed flat (`<rally-id>.rally.yaml` with no folder), the input contract's "rally folder provided" branch becomes meaningless — there is nothing to pass. If the folder persists, the goal of "one flat layout consistently" is only half-met: rallies retain a folder, drives don't.

**Evidence:**
- `plugins/orb/skills/rally/SKILL.md:14, 49, 57, 213-215, 232-239, 529, 550, 758-771, 794-831` — folder is structural, not just a path prefix.
- AC-03 description scopes the change to "every read/write of the rally sidecar uses `.orbit/specs/<rally-id>.rally.yaml`" — silent on input contract, child-spec placement, resume scan.
- Goal claims "the entire substrate uses one flat layout consistently" — rally folder-as-CLI-argument is the only structural deviation left.

**Recommendation:** Either (a) widen AC-03 to spell out: rally folder is collapsed, rally CLI argument becomes `<rally-id>` (matching drive's `<spec-id>`), resume scan becomes `for f in .orbit/specs/*.rally.yaml`, child specs continue to live as flat siblings under `.orbit/specs/` (which is already true — they have their own `<id>.yaml`); or (b) explicitly carve rally out: rallies retain the folder, only drive flattens. Either is defensible, but the spec must commit. Option (a) matches the goal's "one flat layout" claim; option (b) admits an exception.

---

### [MEDIUM] AC-06 is a deferred decision, not an acceptance criterion
**Category:** test-gap
**Pass:** 1
**Description:** AC-06 says "either (a) migrated in place, or (b) left untouched" with the decision "recorded in the spec note before any migration touches the existing folders". The test for this AC is "did someone write a note?" — not "is the substrate in a known good state". This is a known limitation of recording deferred decisions as ACs: a reviewer cannot mechanically verify the implementation chose well. A weaker but verifiable form: "after this spec ships, `orbit verify` returns clean against the repo's `.orbit/specs/` directory" — that catches the failure mode that matters (orphan folders that don't break verify can stay; ones that do must be cleaned up). The spec's first note ("ac-06 ... deferred to implementation time when the actual on-disk state can be inspected") acknowledges the deferral but doesn't substitute a verifiable check.

**Evidence:**
- Spec note `2026-05-09T03:35:36`: "ac-06 (existing-folder policy) and ac-09 (post-ship integration test) are deferred to implementation time".
- AC-06 description: criterion is met by "the decision is recorded in the spec note", not by any state property.
- 12 existing rally/drive folders exist on disk (verified via `find .orbit/specs -name 'drive.yaml' -o -name 'rally.yaml'` — 11 drive folders, 1 rally folder), each containing `drive.yaml`/`rally.yaml` plus stacks of `review-spec-*.md` and `review-pr-*.md`. Without policy commitment, the implementing agent could leave them, half-migrate them, or break verify by partial migration.

**Recommendation:** Pre-commit the policy in the spec rather than deferring: option (b) — historical folders left untouched as bd-era artefacts, only new drives/rallies use the sidecar layout — is the safer default because it's reversible and avoids touching 11 folders at migration time. Replace AC-06's "decision is recorded" framing with "policy is option (b); existing `<id>/` folders remain in place and `orbit verify` returns clean across them" (which is already true today since they only contain non-yaml or `drive.yaml`/`rally.yaml` which won't be picked up by `list_spec_files` if the AC-00 fix above filters by stem shape). If option (a) is preferred, list the 12 folders explicitly in the spec and add a verification step.

---

### [MEDIUM] AC-08 conflates location with content; canonical-doc target is ambiguous
**Category:** test-gap
**Pass:** 1
**Description:** AC-08 says update "`.orbit/conventions/` documentation (or wherever the canonical layout is documented — e.g. orbit-state README, PRIME.md, or a dedicated layout doc)". Today only one file exists in that directory: `acceptance-field.md`. There is no general layout convention doc. The disjunction "or wherever the canonical layout is documented" hands the implementing agent an open-ended search. A test for this AC reads as "did the agent update *some* doc *somewhere*" — under-specified, and downstream consumers (the next agent that wonders "where is the layout documented") inherit the same ambiguity.

**Evidence:**
- `.orbit/conventions/` contains only `acceptance-field.md` (verified via `ls .orbit/conventions/`).
- AC-08 lists three possible targets (`.orbit/conventions/`, orbit-state README, PRIME.md) plus an open "or … a dedicated layout doc" escape hatch.
- `orbit-state/crates/core/src/layout.rs:6-13` already has an in-source convention comment (`specs/<id>.yaml`, `specs/<id>.tasks.jsonl`) — that is the de facto canonical statement today.

**Recommendation:** Pick one canonical home and name it in the AC: `.orbit/conventions/spec-layout.md` is the most discoverable. Spell out the required content in the AC: "a new file `spec-layout.md` exists, lists every per-spec sidecar shape (`<id>.yaml`, `<id>.tasks.jsonl`, `<id>.notes.jsonl`, `<id>.drive.yaml`, `<id>.rally.yaml`, `<id>.review-spec-<date>.md`, `<id>.review-pr-<date>.md`), names the bd-era folder layout as deprecated, and the in-source comment in `orbit-state/crates/core/src/layout.rs` either points to the new doc or duplicates its summary". This converts the AC from "did you do a thing" to a structural check.

---

### [MEDIUM] Review-spec/review-pr SKILL.md output paths reference `<spec-folder>` — out of scope but blocks AC-04
**Category:** missing-requirement
**Pass:** 2
**Description:** AC-04 says "/orb:drive SKILL.md §1.1, §1.3, §3.1, §3.2 and the review-spec / review-pr Agent briefs all cite the sidecar paths". Drive launches review skills with an explicit output path in the brief (per the contract this very review skill documents at `review-spec/SKILL.md:144-145`), so the brief override is sufficient for forked invocation. But the inline-invocation default in both `review-spec/SKILL.md:144` and `review-pr/SKILL.md:125` reads `.orbit/specs/<spec-folder>/review-spec-<date>.md if the spec is folder-shaped, otherwise .orbit/reviews/<spec-id>/review-spec-<date>.md`. Once the folder layout is deprecated, the "folder-shaped" branch becomes dead code, and the fallback `.orbit/reviews/<spec-id>/` is a third location not covered by AC-08's canonical doc. Inline invocation by a human is the default doc-in-the-loop case; leaving the SKILL.md text inconsistent with the canonical layout will surface as drift.

**Evidence:**
- `plugins/orb/skills/review-spec/SKILL.md:144-145`, `plugins/orb/skills/review-pr/SKILL.md:125-126` — both branches reference `<spec-folder>` for inline default.
- AC-04 names "review-spec / review-pr Agent briefs" but not the SKILL.md inline-invocation defaults.
- The brief-override path takes precedence under drive (line 145), so forked invocations are safe; inline invocations are not.

**Recommendation:** Widen AC-04 (or add a sub-bullet) to also rewrite the SKILL.md inline-invocation defaults: "Inline invocation saves to `.orbit/specs/<spec-id>.review-spec-<date>.md` (sidecar form, no `-folder-shaped` branch)." Drop the `.orbit/reviews/` fallback unless there is a reason to keep it — I see none in the goal.

---

### [MEDIUM] Drive resumption scan `for sid in $(...)` will surface drive sidecars only if scanner-fix lands
**Category:** failure-mode
**Pass:** 2
**Description:** AC-05 changes the per-spec test from `[[ -f ".orbit/specs/$sid/drive.yaml" ]]` to `[[ -f ".orbit/specs/$sid.drive.yaml" ]]`. Fine in isolation. But the surrounding loop at `drive/SKILL.md:46-47` reads `orbit spec list` to enumerate `$sid` candidates. If the scanner-fix from the HIGH finding above doesn't land first, `orbit spec list` will fail (or return an error frame) the moment any spec has a `<id>.drive.yaml` sidecar — because `orbit spec list` builds against the index, and the index rebuild parses every yaml in `specs/` as a Spec. The failure mode is: ship AC-05 without the scanner-fix → first `/orb:drive` with no argument runs `orbit spec list` → index rebuild fails → no specs surfaced → user sees "no in-progress drives" even when one exists. Cascade: this also breaks AC-09's no-argument resumption check.

**Evidence:**
- `drive/SKILL.md:42-47` — the no-argument flow runs `orbit spec list --status open --output ids` then loops with the file test.
- `orbit-state/crates/core/src/index.rs:166-189` — `orbit spec list` reads from the index; index rebuild parses every yaml in `specs/`.
- AC-05 description does not name the scanner dependency.

**Recommendation:** Order the work explicitly in the spec's implementation note: scanner-fix first (the new AC-00 from the HIGH finding), then ACs 01/03/05 in any order, then AC-09 dogfood. State that AC-05's verification *requires* AC-00 to have landed — otherwise the resumption snippet will fire but the surrounding `orbit spec list` will be broken.

---

### [LOW] AC-01 / AC-03 grep tests will pass on prose mentions of the old paths
**Category:** test-gap
**Pass:** 2
**Description:** AC-01 and AC-03 verify via `grep` for the literal strings `<spec-id>/drive.yaml` etc. returning zero hits. After implementation, that grep will be clean — but a future agent describing migration history in a code comment ("the old layout was `<spec-id>/drive.yaml`") would re-introduce a hit and falsely fail the test on a re-run. This is a stylistic hygiene issue, not a soundness one; the test as written is fine for the one-shot migration but should not be run as a regression check after the spec ships.

**Evidence:** AC-01, AC-03 use literal-string grep checks.

**Recommendation:** No change required — flag this in the spec note ("AC-01/03 grep checks are migration-time only, not regression checks") so a later agent doesn't bake them into CI and find themselves chasing prose mentions.

---

### [LOW] AC-09 dogfood depends on a real card with ≥3 scenarios — picking the wrong card would be wasteful
**Category:** test-gap
**Pass:** 3
**Description:** AC-09 says "after this spec ships, a /orb:drive on a real card with ≥3 scenarios produces drive.yaml at `.orbit/specs/<spec-id>.drive.yaml`". The integration check is sound but uses a real card from the backlog as the test article — meaning the dogfood drive doubles as feature delivery for whatever card is chosen. If the chosen card itself has issues (thin scenarios, contested design, blocked dependency), the dogfood gets entangled with unrelated failure modes and the AC-09 verdict becomes ambiguous: "did the layout work, or did the card fail for other reasons?"

**Evidence:** AC-09 description; current backlog has multiple unstarted specs.

**Recommendation:** Pre-nominate the card or spec used for AC-09 in the spec note, choosing one with no open design questions and ≥3 simple scenarios. A safer alternative: drive the next unstarted spec rather than promote a fresh card — promotes are higher-variance than resumes.

---

### [LOW] Gate-AC description check (deterministic)
**Category:** missing-requirement
**Pass:** 1
**Description:** Pass-1 deterministic check on every gate AC. ACs 01–05 are gates; all five descriptions are non-empty, none are placeholder tokens, all are well over 20 characters. No findings.

**Evidence:** Parser output column-4 is `1` for ac-01..ac-05; descriptions range from ~250 to ~470 characters.

**Recommendation:** None.

---

## Honest Assessment

The plan is sound in its goal (one flat layout) and correctly identifies the four surfaces that need editing. The author has already discovered the load-bearing risk — the substrate scanner — and recorded it as a spec note, but did not promote it to an AC. Without that AC, an agent driving this spec by AC-coverage alone will edit the SKILL.md files, run `orbit verify` to confirm, and get a hard parse failure on the very files they just created. The biggest risk is ordering: scanner-fix must land before any sidecar yaml hits disk, and the spec doesn't say so.

Secondary risks cluster around scope ambiguity: rally folder convention (MEDIUM-2) and review SKILL inline defaults (MEDIUM-5) are real edits the spec implies but doesn't name; AC-06 and AC-08 defer or under-specify decisions that should be pre-committed. The fixes are mostly additive — one new gate AC (scanner), three AC clarifications, one explicit ordering note. Once those land, the spec is implementable in one driving session.

Recommended path: REQUEST_CHANGES → revise the spec to add the scanner AC and tighten ACs 03, 06, 08, plus ordering note; then re-review.
