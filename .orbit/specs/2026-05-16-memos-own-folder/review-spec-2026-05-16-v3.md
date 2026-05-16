# Spec Review

**Date:** 2026-05-16
**Reviewer:** Context-separated agent (fresh session)
**Spec:** 2026-05-16-memos-own-folder
**Verdict:** APPROVE

---

## Review Depth

| Pass | Triggered by | Findings |
|------|-------------|----------|
| 1 — Structural scan | always | 1 |
| 2 — Assumption & failure | not triggered (single LOW finding, no content-signal risks unaddressed) | — |
| 3 — Adversarial | not triggered | — |

This is review cycle 3. Cycle 1 raised two HIGH risks (AC enumeration insufficient; memo-body self-reference sweep missing) plus four MEDIUM/LOW issues — all resolved in cycle 2's rewrite. Cycle 2 flagged two LOW accuracy bugs in ac-08 (22-vs-23 card count; scenario-5-vs-6 mis-label) and verdicted REQUEST_CHANGES on those. The current spec has corrected both in ac-08's description; one cycle-2 trailing residue remains in ac-08's verification line. APPROVE is appropriate — the residual is one stale numeral on a self-correcting verification, the final `grep -rln` gate catches the substantive case.

## Findings

### [LOW] ac-08 verification line still says "The 22 enumerated cards" — count not updated alongside the description
**Category:** test-gap
**Pass:** 1
**Description:** ac-08's description was correctly updated in cycle 2 to "Twenty-three card files total" and the scenario-index list now correctly reads "scenarios 1, 2, 4, 6". However, the verification clause at `spec.yaml:49` was not updated to match — it still reads `The 22 enumerated cards each have at least one hit pre-edit and zero hits post-edit.` The substantive verification (`grep -rln "cards/memos" .orbit/cards/*.yaml` returns nothing) is correct and self-correcting against the true card count, so this is text-only drift. Flagging because the spec's authoritative text should match reality once implementation forks.

**Evidence:** `spec.yaml:46` (description) reads "Twenty-three card files total"; `spec.yaml:49` (verification) reads "The 22 enumerated cards". Live `grep -ln "cards/memos" .orbit/cards/*.yaml | wc -l` returns 23.

**Recommendation:** Edit ac-08's verification line from "The 22 enumerated cards" to "The 23 enumerated cards" — single-character fix, no scope change. APPROVE does not depend on this landing pre-merge; the gate-grep enforces correctness on its own.

---

## Honest Assessment

The cycle-1 HIGH-severity findings are still resolved in this revision — the enumeration ACs (ac-03 through ac-09) cover every live tracked file that holds a `cards/memos` reference. Cross-checked: `git ls-files -z | xargs -0 grep -l "cards/memos"` returns 51 tracked-file hits; every one falls cleanly into exactly one AC (ac-01 through ac-09) or the explicit ac-10 allow-list (`.orbit/archive/**`, `CHANGELOG.md`, the spec itself, the review-spec sidecars). The conventions docs the cycle-1 reviewer worried about (`.orbit/conventions/*.md`) hold zero hits, confirmed.

Code claims in ac-01 cross-check against `orbit-state/crates/core/src/layout.rs` at the cited line ranges — `memos_dir()` at lines 104-106 returns `self.cards_dir().join("memos")` as stated, `ensure_dirs()` at lines 145-159 includes `memos_dir()` in its iteration, the `list_yaml_files_shallow` wrapper at lines 235-239 is the documented pass-through (`list_yaml_files(dir)` body), and the misleading comment at line 237 (`Like list_yaml_files but explicitly does NOT recurse — used for cards/ where we want to skip cards/memos/`) is exactly the comment ac-01 says to delete. The wrapper's only caller is `list_card_files` as claimed.

Card 0001 scenario references confirmed at lines 11, 16, 25, 37 — scenarios 1, 2, 4, 6 as ac-08's prose now correctly states. The references-block hit on line 45 is what justifies card 0001 also appearing in ac-08's (b) references-level edit list.

ac-10's allow-list is the right shape and grammar mirrors the 2026-04-20 precedent. `review-spec-*.md` glob will pick up cycle-1, cycle-2, and this cycle-3 sidecar.

ac-11 and ac-12 split release and beelink-smoke cleanly. `time_gated: true` is correctly declared on ac-12. The pre-merge / post-merge boundary is explicit in both ACs' prose.

The biggest residual risk remains downstream pilot-repo coordination — older orbit-state binaries in pilot repos will silently lose memo visibility on upgrade until their working trees are migrated. That's a known cost of the relocation, acceptable scope for an orbit-internal spec, and ac-12 verifies the dogfood path on the beelink. Pilot-repo migration is a follow-up, not a blocker for this spec.

Verdict is APPROVE. The single LOW finding is text drift on a verification line whose actual gate-verb (the final `grep -rln`) enforces correctness regardless. Implementation is unambiguous.
