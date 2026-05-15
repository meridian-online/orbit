# Spec Review

**Date:** 2026-05-15
**Reviewer:** Context-separated agent (fresh session)
**Spec:** 2026-05-15-agent-learning-loop
**Verdict:** APPROVE

---

## Review Depth

| Pass | Triggered by | Findings |
|------|-------------|----------|
| 1 — Structural scan | always | 0 |
| 2 — Assumption & failure | content signals (schema migration, cross-system hooks, shared config) | 0 |
| 3 — Adversarial | not triggered | — |

## Findings

None.

---

## Honest Assessment

This is a re-review of the spec after the prior REQUEST_CHANGES cycle (review-spec-2026-05-15.md, seven findings). Every one of those findings is closed cleanly:

1. **Loop-doesn't-close (HIGH)** — ac-04's `by_outcome.<outcome>.invocations[].correction` now returns the correction strings; ac-08 explicitly names this as the read-path the agent uses to draft a SKILL.md edit. Worked example called out in the verification surface.
2. **Session-id deletion ownership (HIGH)** — ac-07(c) pins lifecycle on the Stop hook; ac-05 explicitly states "does NOT delete `.orbit/.session-id`"; ac-09 chains the delete after distill. The three ACs are mutually consistent and cross-reference each other.
3. **`/tmp/session-distillate.md` race (HIGH)** — ac-09 mandates "no shell-fallback `uuidgen` or `/tmp/` files" and uses stdin via the Claude Code mechanism. The cross-session contamination vector is gone.
4. **MCP delivery path for distill stdin (MEDIUM)** — ac-05 now spells out the per-transport surface: CLI uses stdin / `--from`; MCP takes `SessionDistillArgs { session_id, distillate }`. CLI + MCP parity test named.
5. **`started_at` muddled prose (MEDIUM)** — ac-05 rewritten cleanly: "writes the file with `started_at = current_rfc3339_utc()`, `ended_at = current_rfc3339_utc()` (same timestamp — single-instant write)".
6. **`uuidgen` portability (MEDIUM)** — ac-07 mandates a new `orbit session start` verb (Rust `uuid` crate); ac-09 wires it as the SessionStart hook command. Shell-fallback removed.
7. **Migration 0.1 → 0.2 fresh-at-0.2 case (MEDIUM)** — ac-02 spells out three deterministic branches (0.1 → write + no-op; 0.2 → complete no-op; anything else → `Error::malformed`) and explicitly notes fresh-at-0.2 workspaces never trigger the 0.1 → 0.2 path.

Independent structural scan of the amended spec finds no new issues. All four gate ACs (ac-01, ac-02, ac-07, ac-09) carry rich, non-placeholder descriptions far above the 20-char minimum. The Track A / Track B split remains clean. Schema additions match the existing TaskEvent/NoteEvent precedent. Migration semantics are deterministic. Cross-AC invariants (session-id sourcing precedence shared via `read_session_id` helper in ac-07; idempotency of distill in ac-05; non-deletion-by-verb in ac-05 + ac-07; deletion-by-hook in ac-09) line up without contradiction.

Pass 2 surfaced two minor unverified assumptions worth noting but not blocking: (a) Claude Code Stop hooks actually deliver agent prose to stdin — ac-09's verification is manual smoke, which is the right boundary for an integration assumption that cannot be unit-tested; (b) `orbit session start` overwriting a stale `.orbit/.session-id` on crash-then-restart leaks invocations to the previous session_id between crash and next start, which is a low-impact degraded mode (rows still land, recurrence still counts correctly across sessions). Neither warrants a finding.

Plan is ready to implement. The architecture is sound, the contracts at every AC edge are explicit, and the corrections from cycle 1 are precise. No further changes requested.
