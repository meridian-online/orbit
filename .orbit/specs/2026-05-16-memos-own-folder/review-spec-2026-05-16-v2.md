# Spec Review

**Date:** 2026-05-16
**Reviewer:** Context-separated agent (fresh session)
**Spec:** 2026-05-16-memos-own-folder
**Verdict:** REQUEST_CHANGES

---

## Review Depth

| Pass | Triggered by | Findings |
|------|-------------|----------|
| 1 — Structural scan | always | 1 |
| 2 — Assumption & failure | content signals present (substrate layout change, release/brew deploy, cross-system pickup via beelink) | 2 |
| 3 — Adversarial | not triggered (no structural defects; verifications self-correct the accuracy bugs) | — |

This is review cycle 2. The cycle-1 review at `review-spec-2026-05-16.md` identified two HIGH risks (AC-09 unreachable; memo-body sweep missing) and four MEDIUM/LOW issues; the cycle-2 spec materially addressed all of them — enumeration ACs were split (ac-03 through ac-10), the allow-list was added with the 2026-04-20 precedent cited, memo-body sweep is now explicit in ac-02, and the release/beelink gates were split into ac-11 (pre-merge) and ac-12 (time-gated post-merge). Two minor accuracy bugs remain.

## Findings

### [LOW] ac-08 card count is off-by-one

**Category:** test-gap
**Pass:** 1
**Description:** ac-08 says "Twenty-two card files total" but the enumerated list contains 23 cards: four scenario+references cards (0001, 0002, 0008, 0017) plus nineteen references-only cards (0005, 0006, 0009, 0010, 0013, 0014, 0015, 0016, 0018, 0019, 0020, 0021, 0022, 0023, 0024, 0025, 0029, 0030, 0031) = 23. Live grep against `main` confirms 23 card files match. The verification `The 22 enumerated cards each have at least one hit pre-edit and zero hits post-edit.` therefore mis-states the pre-edit baseline if used as a sanity check.
**Evidence:** `spec.yaml:46` ("Twenty-two card files total") and `spec.yaml:49`. `grep -ln "cards/memos" .orbit/cards/*.yaml | wc -l` returns 23.
**Recommendation:** Edit ac-08 to say "Twenty-three card files total" and update the verification line accordingly. The enumeration itself is complete; only the count is wrong.

### [LOW] ac-08 mis-identifies the card-0001 scenario hits

**Category:** test-gap
**Pass:** 2
**Description:** ac-08 says card 0001 has scenario-level edits at "scenarios 1, 2, 4, 5". The actual `cards/memos` hits in card 0001 fall on scenarios **1, 2, 4, and 6**. Scenario 5 ("Memo referenced after distill", lines 29-33) has no `cards/memos` reference; scenario 6 ("Memo deleted after promotion", line 37) does — the string `archived under cards/memos/.archive/`. An implementer following the spec text literally may look for a non-existent hit in scenario 5 and miss the scenario-6 edit. The final verification (`grep -rln "cards/memos" .orbit/cards/*.yaml` returns nothing) will catch the omission, so the AC is self-correcting — but the description misleads.
**Evidence:** Read of `.orbit/cards/0001-memos.yaml`. Scenario 5 (lines 29-33) contains "the card's references field points back to the memo" — no path string. Scenario 6 (lines 34-38) contains "archived under cards/memos/.archive/" at line 37.
**Recommendation:** Change "scenarios 1, 2, 4, 5" to "scenarios 1, 2, 4, 6" in ac-08. No other changes — the rewrite scope is correct, only the scenario-index label is wrong.

### [LOW] ac-01's wrapper-removal rationale is sound but worth one extra invariant

**Category:** assumption
**Pass:** 2
**Description:** ac-01 removes `list_yaml_files_shallow` at `layout.rs:235-239` on the basis it is a pass-through. Reading the source confirms the body is exactly `list_yaml_files(dir)` — assumption correct. The risk is that a future reader who adds a nested-under-cards directory might re-introduce recursion-suppression intent and not know the wrapper was deliberately removed. Minor maintainability concern, not a correctness defect; flagging only because the spec already touches the layout module's doc-comment block.
**Evidence:** `layout.rs:235-239` reads exactly `list_yaml_files(dir)`; the wrapper's only existing caller is `list_card_files` at `layout.rs:188-191`.
**Recommendation:** Either accept as-is (preferred — the doc-comment at the top of `layout.rs` will be updated per ac-01 already), or add a sentence to ac-01 noting that the updated ascii tree at `layout.rs:24` should drop the now-meaningless `cards/memos/` line and the inlined `list_card_files` should carry a one-line comment explaining cards/ is flat. Optional polish, not a blocker.

---

## Honest Assessment

The cycle-1 review's HIGH-severity findings are resolved. The new spec correctly enumerates every affected file across ac-03 through ac-09 (cross-checked against `git ls-files | xargs grep -l "cards/memos"` — every match falls into exactly one AC or the explicit allow-list in ac-10), the allow-list mirrors the 2026-04-20 precedent verbatim, ac-02 now sweeps memo bodies for self-referential strings, ac-06 preserves the METHOD.md byte-equality discipline, and ac-11/ac-12 split the release and beelink-smoke concerns cleanly with `time_gated: true` declared on ac-12.

Code locations are accurate: `layout.rs:104-106` (memos_dir), `layout.rs:235-239` (the wrapper), `layout.rs:188-191` (list_card_files), `layout.rs:280` (ensure_dirs test), `layout.rs:350` (list_card_files_does_not_recurse_into_memos test), `verify.rs:105` (the misleading comment) — all confirmed against the working tree. The skill file line citations (memo:3/32/50/62/67, distill:17/170/172/183, design:63, rally:119, implement:230, README:55/65/67/126, METHOD:22) also resolve correctly.

Residual risks are both minor accuracy bugs in ac-08's prose: a 22-vs-23 card count and a scenario-5-vs-6 mis-label. Both are self-correcting via the final `grep -rln` gate in ac-08's verification, so merge risk is low — but spec text is the authoritative reference once implementation forks, and it should match reality.

The biggest residual deployment risk is that older orbit-state binaries in any pilot repos would silently lose memo visibility once memos move to `.orbit/memos/`. ac-11 mitigates by gating the release before merge and ac-12 verifies the beelink picks it up. For orbit's own dogfooding this is covered. Downstream pilot-repo coordination is implicit but not enumerated — acceptable scope for an orbit-internal spec.

Verdict is REQUEST_CHANGES rather than APPROVE on the strength of the two ac-08 accuracy bugs. Both are one-line edits; once they land, the spec is implement-ready.
