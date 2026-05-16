# Spec Review

**Date:** 2026-05-16
**Reviewer:** Context-separated agent (fresh session)
**Spec:** 2026-05-16-memos-own-folder
**Verdict:** REQUEST_CHANGES

---

## Review Depth

| Pass | Triggered by | Findings |
|------|-------------|----------|
| 1 — Structural scan | always | 5 |
| 2 — Assumption & failure | content signals (substrate boundary + deployment in ac-10) and HIGH structural findings | 2 |
| 3 — Adversarial | not triggered (Pass 2 didn't surface cascading-failure structural problems beyond what ac-09 already gates) | — |

## Findings

### [HIGH] AC-09's grep gate is unreachable — ~30 tracked files with `cards/memos` references are not enumerated by any AC
**Category:** missing-requirement
**Pass:** 1
**Description:** AC-09 demands `grep -rn "cards/memos" .` returns zero hits across all tracked files (excluding `.orbit/archive/`, `.git`, `target`, `node_modules`). ACs 03-08 enumerate edits to: `/orb:memo`, `/orb:distill`, `/orb:design`, `/orb:rally`, `/orb:implement` SKILL.md files; `.orbit/METHOD.md` + `plugins/orb/skills/setup/METHOD.md`; `README.md`; cards 0001 and 0008. That list omits the bulk of the offending references.

**Evidence:** `grep -rln "cards/memos" . --exclude-dir=.git --exclude-dir=.orbit/archive --exclude-dir=target --exclude-dir=node_modules` returns 52 files. Files referenced by ACs 03-08 cover ~10 of these. The unenumerated remainder includes:

- **Other card YAMLs** (~24 files, all `references:` entries citing memo source paths): `0002-distill.yaml`, `0005-drive.yaml`, `0006-rally.yaml`, `0009-mission-resilience.yaml`, `0010-objective-functions.yaml`, `0013-playbook-fast-path.yaml`, `0014-default-merge-after-review.yaml`, `0015-research-mode-iteration-loop.yaml`, `0016-bead-native-cold-fork-reviews.yaml`, `0017-setup-is-bead-aware.yaml`, `0018-two-artefact-contract.yaml`, `0019-tabletop.yaml`, `0020-orbit-state.yaml`, `0021-tasks.yaml`, `0022-skill-curator.yaml`, `0023-memory-loop.yaml`, `0024-lean-pass.yaml`, `0025-codebase-mastery.yaml`, `0029-fan-out.yaml`, `0030-canonical-schema-and-glossary.yaml`, `0031-design-session-user-language.yaml`. AC-08 names only 0001 and 0008.
- **Live (non-archived) specs**: `.orbit/specs/2026-05-10-card-id-field-and-conventions/spec.yaml`, `.orbit/specs/2026-05-08-executive-communication-wires/{spec.yaml,interview.md}`, `.orbit/specs/2026-05-08-four-pillars-wires/{spec.yaml,interview.md}`. The "archive is canonically frozen" allow-list doesn't cover live specs.
- **Live choice**: `.orbit/choices/0021-spec-folders.yaml` (2 hits).
- **Live memories**: `.orbit/memories/four-pillars.yaml`, `.orbit/memories/fan-out-first-class.yaml`.
- **CHANGELOG.md** (3 hits at lines 350, 536, 552 — historical entries).
- **The migrated memo body itself**: `.orbit/cards/memos/2026-05-01-pilot-rollout-check-in.md:34` contains the literal string `.orbit/cards/memos/` describing where to file findings. After `git mv` to `.orbit/memos/2026-05-01-pilot-rollout-check-in.md` the string survives and trips AC-09.
- **Layout doc-comment + verify.rs comment**: AC-01 covers `layout.rs:24` and `verify.rs:105`, but `layout.rs:189` (`// Cards live directly in cards/, not under cards/memos/.`) and `layout.rs:237` (`// where we want to skip cards/memos/.`) are not named. They're a natural consequence of AC-01's wrapper-removal point but not stated.

**Recommendation:** Either (a) expand the explicit-enumeration ACs to cover every tracked file, by category — add an AC for card `references:` updates (or fold them into AC-08 with an explicit "all card files, not just 0001 and 0008"); an AC for live specs/choices/memories; an AC for the migrated-memo's own body; an AC for CHANGELOG (probably: leave as-is and amend AC-09's allow-list to exempt it, following the precedent in archive spec 2026-04-20-orbit-artefact-folder line 78) — **or** (b) loosen AC-09's allow-list to cover CHANGELOG history, memory bodies, and the migrated memos themselves, and add a tighter gate that only enforces zero hits in **path strings** (not prose history). Option (a) is mechanical but high-volume; option (b) is cleaner but needs an unambiguous allow-list grammar. Pick one and write it in.

### [HIGH] Migration of the surviving memo body string is not handled
**Category:** missing-requirement
**Pass:** 1
**Description:** `.orbit/cards/memos/2026-05-01-pilot-rollout-check-in.md` body line 34 says: *"Bug or regression in the substrate → memo here in `.orbit/cards/memos/`..."*. AC-02 moves the file via `git mv`; the body text is untouched. AC-09 then fails because that string still resolves under the search root. Same risk applies to any other memo that documents its own location.

**Evidence:** `grep -n "cards/memos" .orbit/cards/memos/2026-05-01-pilot-rollout-check-in.md` → `34:- **Bug or regression in the substrate** → memo here in \`.orbit/cards/memos/\`, then file a bead via \`bd create -t task\` and queue for the next drive.`

**Recommendation:** Add an explicit step to AC-02 (or a new AC) — after `git mv`, sweep the moved memo bodies and rewrite any literal `.orbit/cards/memos/` strings inside them. Alternatively, exempt `.orbit/memos/` from AC-09's grep gate on the basis that memo bodies are historical prose. State the choice.

### [MEDIUM] AC-01's claim that `list_yaml_files_shallow` purpose was "avoiding recursion into cards/memos/" is incorrect
**Category:** assumption
**Pass:** 1
**Description:** AC-01 says removing or re-rationalising `list_yaml_files_shallow` is fine because "its only purpose was avoiding recursion into `cards/memos/`, which becomes moot once memos are siblings". Inspecting `layout.rs:235-239`: `list_yaml_files_shallow` body is `list_yaml_files(dir)` — it's already a pass-through, not a recursion-suppressor. Neither function recurses; both use a single `read_dir` pass. The wrapper is effectively dead code today, and the comment at line 237 is misleading. Worth fixing, but the rationale needs restating.

**Evidence:** `layout.rs:206-232` (the canonical `list_yaml_files`) does not recurse — it `read_dir`s one level and filters by extension + dotted-stem. `list_yaml_files_shallow` at `layout.rs:235-239` is a trivial wrapper.

**Recommendation:** Reword AC-01's `list_yaml_files_shallow` clause: drop the "avoiding recursion" rationale (wrong) and replace with "the wrapper is now redundant dead code — remove it and replace the single caller (`list_card_files`) with a direct call to `list_yaml_files`, also updating the `layout.rs:189` comment". This is the cleanup actually being asked for.

### [MEDIUM] AC-06's byte-equality verb skips the `.orbit/conventions/spec-layout.md` reference
**Category:** test-gap
**Pass:** 1
**Description:** AC-06 verifies `.orbit/METHOD.md` and `plugins/orb/skills/setup/METHOD.md` stay byte-equal after the memo-row edit, and that's correct. But the layout doc-comment in `layout.rs:6-8` cross-references `.orbit/conventions/spec-layout.md`; if that conventions file also names `cards/memos/`, AC-09 catches it but no AC enumerates it. Quick check: file likely exists.

**Evidence:** `layout.rs:6` mentions `.orbit/conventions/spec-layout.md`. Reviewer didn't open it; if it names the memo path, AC-09 will catch it and AC-09 cannot pass without an edit there too.

**Recommendation:** Before implementation, `grep -n "cards/memos" .orbit/conventions/*.md` and either add it to the enumeration list (option a above) or place it in the allow-list (option b above).

### [MEDIUM] AC-10 couples this spec to a release cycle and a beelink smoke that the spec author may not own
**Category:** failure-mode
**Pass:** 1
**Description:** AC-10 ships a new orbit version and runs `orbit session prime` on the beelink to confirm the new memo path resolves. The beelink runs the brewed binary, which requires brew tap propagation — that has its own latency. Three failure modes: (1) the brew tap hasn't picked up the tag yet when the smoke runs; (2) the beelink mutagen sync is stale and `.orbit/memos/` hasn't appeared on the beelink filesystem; (3) the binary upgrades but `orbit session prime` still reports the old memo count because the binary scans the path from its own layout struct — which AC-01 fixes, so the chain depends on AC-01 shipping in the released version, not a hotfix on top.

**Evidence:** AC-10 verification cites `brew upgrade meridian-online/tap/orbit`, `orbit --version`, and `orbit session prime --root /home/hugh/github/hughcameron/orbit`. It also states `ac-10's checked: flipped to true in a small follow-up commit`, acknowledging the time-gated nature.

**Recommendation:** Either (a) split AC-10 into two: AC-10a "release the new version" (gate before merge), AC-10b "beelink smoke confirms" (time-gated, post-merge follow-up commit) — making the time-gate explicit; or (b) move the beelink smoke entirely out of the spec into a follow-up memory entry, since the spec's actual deliverable is the layout move, not the release. AC-10's `time_gated: true` flag already marks it; the spec should declare what passing the implement gate looks like when AC-10 is the only outstanding one.

### [MEDIUM] CHANGELOG.md treatment is ambiguous
**Category:** constraint-conflict
**Pass:** 2
**Description:** CHANGELOG.md has three `cards/memos` references at lines 350, 536, 552 — all historical entries describing past behaviour. AC-09's grep gate names only `.git`, `.orbit/archive`, `target`, `node_modules` as exclusions. CHANGELOG is "tracked" and "live", so a literal reading of AC-09 requires editing CHANGELOG history. That's almost certainly the wrong call — changelogs are append-only, immutable history. The previous artefact-folder migration (archive spec `2026-04-20-orbit-artefact-folder/spec.yaml:78`) handled this with an explicit allow-list clause for CHANGELOG.

**Evidence:** `grep -n "cards/memos" CHANGELOG.md` → lines 350, 536, 552. Previous-migration precedent at `.orbit/archive/specs/2026-04-20-orbit-artefact-folder/spec.yaml:78` allow-lists "CHANGELOG.md entries that name legacy paths when describing the migration itself".

**Recommendation:** Add an explicit allow-list clause to AC-09 mirroring the 2026-04-20 precedent — "Allow-list (hits expected and accepted): CHANGELOG.md historical entries, `.orbit/cards/0001-memos.yaml` voice-memo example scenarios if intentionally preserved, …". The current AC-09 is too tight.

### [LOW] Cycle suffix discipline for re-reviews not stated, but the spec is single-cycle so far
**Category:** test-gap
**Pass:** 2
**Description:** Not a spec bug — observation. If this review's REQUEST_CHANGES triggers a respin, the next review on 2026-05-16 should land at `review-spec-2026-05-16-v2.md` per the skill contract. No action for the spec author; flagging for the implementer.

**Evidence:** Skill instructions §3 output-path block.

**Recommendation:** None for the spec. Implementer respinning today writes `-v2`.

---

## Honest Assessment

This is a clean, well-scoped migration that the spec author has clearly walked end-to-end — line anchors are accurate, the rationale for AC-09 as a wholesale grep gate is sound, and AC-06's byte-equality discipline catches the right paired-file invariant. The work the spec describes is right.

The biggest risk is **AC-09 cannot pass as written**. The grep gate is calibrated to a much smaller surface than actually exists. Either ACs 03-08 must enumerate the missing ~30 files (card references, live specs/choices/memories, the migrated memo body, conventions docs) **or** AC-09 needs a meaningful allow-list. Right now the spec defines a gate that fails on first pass with no path to green that isn't ad-hoc.

Secondary risk: AC-10 couples the spec to a release cycle that is intrinsically multi-stage (brew tap, mutagen, binary upgrade). The `time_gated: true` flag is the right signal but the spec doesn't say what merging looks like with AC-10 unchecked.

Fix those two and this is APPROVE-ready. The structural work itself is straightforward.
