# Design: spec.close AC pre-flight

**Date:** 2026-05-13
**Interviewer:** Claude (Opus 4.7)
**Card:** .orbit/cards/0034-spec-close-ac-preflight.yaml
**Mode:** open

---

## What good looks like

Closing a spec is supposed to be the moment the work is over. Today the verb only checks that no tasks are still open — it doesn't read the acceptance criteria, so a spec can flip to `closed` with one or two ACs still unchecked, and nobody finds out until someone scrolls past `[ ] ac-15` weeks later. I want close to be a real boundary: the verb reads the ACs, names anything still unchecked (gate ACs called out separately because they matter more), and either holds the close until I confirm I meant it or refuses outright unless I pass a deliberate opt-in. The clean-path close should feel exactly as it does today; the only difference appears when there is actually something to surface.

---

## Context

Card: *spec.close AC pre-flight — closure refuses to slip past unchecked acceptance criteria* — 6 scenarios, goal: `orbit spec close` reads `acceptance_criteria`, identifies entries with `checked: false`, and surfaces them by id before status flips.

Prior specs: 0 — this is the first spec for this card.

Gap: the entire capability. `orbit spec close` at `orbit-state/crates/core/src/verbs.rs:1097` reads no AC state today; it guards only on unfinished child tasks (line 1147, `Error::conflict` listing the offenders).

References on the card:
- `verbs.rs:1097` (`spec_close`) — pattern to mirror
- `schema.rs:138` (`AcceptanceCriterion`) — `checked: bool` already present, only the close-time read is missing

No accepted choice file under `.orbit/choices/` pins disposition. Design space classified as **open**.

---

## Q&A

### Q1: Where the guard lives and what its default behaviour is

**Q:** Where should the unchecked-AC guard live, and what is its default behaviour?

Options offered:
1. Block at verb + `--force` flag (mirror unfinished-tasks pattern)
2. Warn at verb, no block (surfacing is the entire mechanism)
3. Block at verb + skill-layer prompt (substrate strict, friction owned by /orb:drive)

**A:** Block at verb + `--force` flag — with a follow-up question on how to handle ACs that are time-gated (some ACs expect a period of real-world use).

### Q2: How spec.close handles time-gated ACs

**Q:** How should `spec.close` handle ACs that are legitimately expected to remain unchecked at close (time-gated, post-ship observation)?

Options offered:
1. Uniform — `--force` is the path for both forgot-to-tick and deliberately-deferred
2. New AC field at declaration (e.g. `time_gated: bool`) — spec.close ignores time-gated ACs from the unchecked count
3. Close-time allowlist flag (`--pending ac-18,ac-22`)
4. Defer the time-gated category to a separate spec, ship uniform now

**Initial agent recommendation:** Option 4 (defer), assuming time-gated ACs were rare.

**Hugh's counter-evidence:** Time-gated ACs recur regularly in adjacent project work — the rarity premise was wrong.

**Agent verification:** Confirmed via read-only inspection of an adjacent project's spec corpus. That corpus's ACs already carry a richer taxonomy distinguishing several kinds (code / configuration / operational / post-deploy-observation / research / documentation), with multiple post-deploy-observation ACs per live-wiring spec (e.g. 24-hour pre-cutover soak windows, 7-day post-cutover comparison windows). The pattern is recurring, not exceptional.

**A:** Option 2 (declaration-time field), narrow scope — add `time_gated: bool` (default `false`) to `AcceptanceCriterion`. The broader AC-type taxonomy (enum: code / ops / observation / …) is filed as a follow-up card (0035-ac-taxonomy) rather than included in this spec.

---

## Summary

### Goal

`orbit spec close` reads the spec's `acceptance_criteria`, partitions entries by `checked` and `time_gated` state, and refuses to close while non-`time_gated` ACs remain unchecked — unless `--force` is passed. Time-gated ACs are listed in the close output as a deliberate-deferral category but do not block.

### Constraints

- **Mirror the existing pattern.** Unchecked-AC handling uses `Error::conflict` with sentence-form listing, exactly as the unfinished-tasks guard does at `verbs.rs:1147`. Same error category, same sentence shape, same caller experience on both CLI and MCP surfaces.
- **`time_gated` is additive and default-false.** Existing specs without the field parse unchanged via `#[serde(default)]`. No migration over open specs; no canonical-output churn beyond newly-authored ACs.
- **Narrow scope on the schema change.** A single `bool` field on `AcceptanceCriterion`. The broader categorical taxonomy is deferred (see card 0035).
- **Happy path is unchanged.** Specs with all ACs checked (or with the only unchecked ones flagged time-gated) close exactly as they do today — no new flags, no new prompts, no behaviour shift.
- **Gate ACs called out separately.** Per scenario 4 in card 0034: an unchecked AC with `gate: true` is a stronger signal of premature close than a non-gate AC. The disposition (block) is the same; the surfacing distinguishes.

### Success Criteria

- `orbit spec close <id>` returns `Error::conflict` and does not write any file when one or more `checked: false && !time_gated` ACs remain. Error message lists the offending AC ids and flags gate ACs separately.
- `orbit spec close <id> --force` proceeds despite unchecked ACs, with the same logging discipline (the close output names what was bypassed).
- ACs with `time_gated: true` are reported in the close output as deferred-OK but do not block — even when unchecked.
- Behaviour is identical on CLI and MCP surfaces (parity tests).
- The existing unfinished-tasks guard at `verbs.rs:1147` is unchanged; the new guard sits adjacent to it.

### Decisions Surfaced

- **Block-at-verb with explicit `--force` opt-in** (over warn-only or skill-layer prompt). Rationale: mirrors the unfinished-tasks precedent; works identically on CLI and MCP; keeps friction owned by the substrate rather than scattered across skills. Candidate for a MADR record if the warn-vs-block question recurs elsewhere.
- **Declaration-time `time_gated: bool` over close-time allowlist or uniform `--force`.** Rationale: time-gated ACs recur in adjacent project work; declaration-time amortises across every close invocation, keeps the audit trail in the spec itself, and avoids overloading `--force` with two distinct intents (forgot-to-tick vs deliberate-deferral).
- **Narrow scope — bool, not enum.** Rationale: the broader AC-type taxonomy is a separate design conversation that needs to land alongside the canonical-schema work (card 0030) and the brownfield migration on-ramp (card 0032). Card 0035 captures the deferred generalisation.

### Implementation Notes

- **Surface mirrors `verbs.rs:1097` (`spec_close`).** New guard sits between the existing `validate_spec_id` / lock-acquisition block and the unfinished-tasks check at line 1147, or immediately after — order TBD by the implementing agent. Use `Error::conflict` with a sentence-form list (`"3 unchecked AC(s) in spec 'foo': ac-04, ac-07, ac-15 (ac-04 [gate])"`).
- **`AcceptanceCriterion::FIELDS` update** at `schema.rs:97` adds `"time_gated"`. Existing FIELDS-drift unit test (added in the tree-views work) will catch any forgotten registration.
- **Serde default** — `#[serde(default)]` for the field. Skip-if-default for canonical output is a separate stylistic call; the existing convention (e.g. `gate: bool`, `checked: bool`) is to always serialise — follow that unless there's reason to deviate. Implementing agent's call.
- **`--force` flag** on the CLI `spec close` subcommand and the corresponding MCP arg. Parity test required.
- **Tests** alongside the existing `spec_close_rejects_unfinished_tasks` at `verbs.rs:3941`:
  - `spec_close_rejects_unchecked_acs`
  - `spec_close_force_proceeds_despite_unchecked`
  - `spec_close_time_gated_acs_do_not_block`
  - `spec_close_unchecked_gate_ac_flagged_in_error`
  - MCP parity equivalents (likely in the existing parity-test file).
- **`/orb:drive`'s close step** should surface the unchecked-AC error to the author with the offending ids before deciding whether to invoke `--force` — narrative wire, captured as a separate AC. This is the skill-side complement to the substrate-side guard.
- **No migration of existing specs** is required because `time_gated` defaults false. Canonical re-write of any AC will gain `time_gated: false` per the existing always-serialise convention — confirm during implement whether this is the right output convention or whether skip-if-default is preferred for this field specifically.

### Open Questions

None at intent level. All remaining questions are implementation-shaped and have been routed above.

---

## Out of Scope (deferred to card 0035)

- Broader AC-type taxonomy (code / ops / observation / research / doc / config or similar).
- Type-aware verification handoff in `/orb:review-pr`.
- Type-aware drive strategy selection.
- Brownfield AC-taxonomy absorption via `orbit canonicalise --reconcile` (waiting on card 0032 + 0030 to land first).
