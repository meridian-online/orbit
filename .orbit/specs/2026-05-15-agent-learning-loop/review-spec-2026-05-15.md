# Spec Review

**Date:** 2026-05-15
**Reviewer:** Context-separated agent (fresh session)
**Spec:** 2026-05-15-agent-learning-loop
**Verdict:** REQUEST_CHANGES

---

## Review Depth

| Pass | Triggered by | Findings |
|------|-------------|----------|
| 1 — Structural scan | always | 2 |
| 2 — Assumption & failure | content signals (schema migration, cross-system hooks, shared config) + Pass 1 medium finding | 5 |
| 3 — Adversarial | Pass 2 surfaced a loop-doesn't-close gap and a concurrency hazard | 1 |

## Findings

### [HIGH] Loop cannot close — agents have counts but no correction text
**Category:** missing-requirement
**Pass:** 2
**Description:** ac-03 records a `correction: Option<String>` per invocation. ac-04's `recurrence` verb returns per-outcome counts and timestamps but explicitly does **not** return the correction strings (output shape names `count` and `timestamps`, nothing else). ac-08 then directs the agent to edit SKILL.md when recurrence ≥ 2. But the agent has no read-path back to the correction text that describes *what went wrong* and therefore cannot draft a sensible SKILL.md edit. The "skill self-improvement" half of the spec — the headline of Track A — does not close with the current ACs.
**Evidence:** ac-04 output shape: `{"by_outcome": {"worked": {"count": N, "timestamps": [...]}, ...}}` — no `corrections` key. ac-08 instructs agent-judgment edits triggered by recurrence count. Goal text claims the loop is "Implement v1 of the agent learning loop"; without correction-text retrieval, agents only have a counter.
**Recommendation:** Either (a) extend ac-04's response to include the `correction` strings alongside timestamps per outcome (e.g. `{"by_outcome": {"incorrect": {"count": 2, "entries": [{"timestamp": ..., "correction": ...}, ...]}, ...}}`), or (b) add a new AC for `orbit skill list-invocations <skill_id> [--outcome <x>] [--since <ts>]` that returns the full rows. Option (a) is cheaper and keeps the verb count the same.

### [HIGH] Session-id deletion ownership is ambiguous between ac-05, ac-07, ac-09
**Category:** missing-requirement
**Pass:** 2
**Description:** ac-07 states "the Stop hook reads the file then deletes it as part of distillation". ac-05 (`orbit session distill` verb contract) does not specify any delete behaviour. ac-09 (Stop hook command in `.claude/settings.json`) says the command is `orbit session distill --from /tmp/session-distillate.md` (or pipe directly) — no explicit delete step. Three valid interpretations exist, each with different failure modes:
1. The verb deletes `.orbit/.session-id` after a successful distill. Side-effect not in ac-05's contract.
2. The hook command does the delete as a chained shell command. Not in ac-09's command shape.
3. The SessionStart hook unconditionally overwrites the file on next start. Then a session that crashes between Stop-firing-distill and next-SessionStart leaks the prior session_id, and the *next* `orbit skill record-invocation` after a crash writes against the wrong session_id.
**Evidence:** ac-07 line: "The SessionStart hook ... writes it to `.orbit/.session-id` at session start; the Stop hook (per ac-09) reads the file then deletes it as part of distillation." ac-05: no delete clause. ac-09: command snippet is `orbit session distill --from /tmp/session-distillate.md` — single step.
**Recommendation:** Make the deletion explicit. Best fit: add to ac-05 — "After a successful distill, `.orbit/.session-id` is deleted (best-effort; missing file is not an error)." Also update ac-09 to explicitly call out the delete as part of the Stop pipeline so the implementer doesn't drop it.

### [HIGH] `/tmp/session-distillate.md` races across concurrent sessions
**Category:** failure-mode
**Pass:** 2 + 3
**Description:** ac-09 names `/tmp/session-distillate.md` as the wrapper convention for piping the distillate to `orbit session distill --from`. `/tmp` is shared across all sessions on a host. Hugh runs concurrent fan-out work (rallies, parallel drives — pillar 4). Two Claude Code sessions ending within seconds of each other both write `/tmp/session-distillate.md` — the last write wins. Both invocations of `distill` then read whichever distillate happened to land last; one session's reflection is silently overwritten with the other's. The cross-contamination is undetectable from inside the substrate.
**Evidence:** ac-09 line: "the hook command can be `orbit session distill --from /tmp/session-distillate.md` if a wrapper writes the file". Pillar 4 (long-running R&D) and orbit's existing `rally` / `drive` skills explicitly support concurrent agents.
**Recommendation:** Make the path session-scoped. Either `/tmp/session-distillate-${ORBIT_SESSION_ID}.md` (requires the wrapper to know the session id, which it does — it's already reading `.orbit/.session-id`) or `.orbit/.session-distillate` (per-clone, per-repo; collides only when two agents share the same checkout, which is already a degraded mode). Alternatively, document that the distillate is delivered on stdin (the verb already supports this per ac-05) and have the Stop hook pipe directly rather than via a file.

### [MEDIUM] MCP delivery path for `distill` stdin input is unspecified
**Category:** test-gap
**Pass:** 2
**Description:** ac-05 says distillate is sourced from stdin by default, or `--from <path>` if passed. The CLI parity is clear; the MCP parity is not. MCP tool calls deliver structured arguments — there is no stdin in the MCP wire format. ac-05's verification names a CLI+MCP parity test but does not name how the MCP surface accepts the distillate. The MCP test would have to either route through `--from` only (loses parity), or invent a third arg (e.g. `--distillate <inline-string>`), or the verb takes a `distillate` field in its args struct on the MCP side.
**Evidence:** ac-05 verification: "CLI + MCP parity test added." ac-05 description does not name the MCP arg.
**Recommendation:** Add an explicit `--distillate <string>` (or equivalent MCP arg field) to ac-05 with precedence rules: explicit arg > `--from` path > stdin. The MCP surface uses the explicit arg; the CLI accepts all three.

### [MEDIUM] `started_at` source on first call is muddled in ac-05
**Category:** constraint-conflict
**Pass:** 1
**Description:** ac-05 reads "A first call writes the file with `started_at` set to the file's implicit creation time (or `current_rfc3339_utc` if the file is being created)". The two clauses contradict: if it's the first call, the file *is* being created, so the "implicit creation time" branch never fires. This is dead prose that will confuse the implementer.
**Evidence:** ac-05 verbatim: "set to `started_at` set to the file's implicit creation time (or `current_rfc3339_utc` if the file is being created)".
**Recommendation:** Rewrite to: "A first call writes the file with `started_at` set to `current_rfc3339_utc()` at the moment of the write. On subsequent calls, `started_at` is preserved (read from the existing file); `ended_at` and `distillate` are updated to the new write's values." (The second half is already correct in the AC.)

### [MEDIUM] SessionStart hook command portability — `uuidgen` is not universal
**Category:** failure-mode
**Pass:** 2
**Description:** ac-09 leaves the SessionStart hook command unspecified: "a small shell command or a new `orbit session start` verb — implementer's choice". The implicit shell route is `uuidgen > .orbit/.session-id`. `uuidgen` ships by default on macOS and most desktop Linux distros but is not present on minimal/Alpine/busybox environments. Hugh runs both macOS and Linux (Beelink references in MEMORY); if the loop is ever deployed to a CI runner or container without `uuidgen`, the SessionStart hook fails silently and the verbs all return `Error::unavailable`.
**Evidence:** ac-09 hook command underspecified. ac-03 / ac-05 / ac-07 all rely on the file existing.
**Recommendation:** Mandate `orbit session start` as a new verb (adds a thin AC or absorbs into ac-09's verification). The verb wraps Rust's `uuid` crate for portability. The shell-fallback option should be removed to prevent the implementer from picking the brittle path.

### [MEDIUM] `migrate.rs` 0.1 → 0.2 path is not specified for fresh-at-0.2 workspaces
**Category:** failure-mode
**Pass:** 2
**Description:** ac-02 bumps `.orbit/schema-version` from 0.1 to 0.2. `migrate.rs` already handles forward migrations of existing workspaces. The AC says "The existing schema-version migration test extended to cover `0.1 → 0.2` cleanly" — but doesn't cover the **brand-new** workspace case (no `.orbit/schema-version` file). The current substrate creates `.orbit/sessions/` lazily on first `distill` call (per ac-05); a fresh `orbit init` (or whatever bootstraps a 0.2 workspace) needs to either pre-create the dir or rely on lazy creation. Verify lazy creation is the chosen path and is tested.
**Evidence:** ac-02 verification covers migration; ac-05 description says "creating the file ... on first call" for `.orbit/skills/` but does not mirror the language for `.orbit/sessions/`.
**Recommendation:** Either add a sentence to ac-05 confirming `.orbit/sessions/` is created lazily on first `distill` call (mirrors ac-03's "creating the file (and `.orbit/skills/` dir) on first call"), or specify that `migrate.rs` 0.1 → 0.2 creates the sessions dir at upgrade time.

### [LOW] Allowlist enforcement is documentation-only (acknowledged)
**Category:** content-signal
**Pass:** 1
**Description:** ac-08 ships the allowlist as prose in `.orbit/conventions/skill-self-improvement.md`, not as code that prevents an agent from editing a non-allowlisted SKILL.md. The AC names this as a deliberate stopgap until card 0022 ships front-matter enforcement. Worth surfacing for visibility — not a blocker, but the v1 loop relies on agent self-discipline.
**Evidence:** ac-08 verbatim: "SKILL.md files in `plugins/orb/skills/*/` are NOT modified by this AC — the convention is the v1 enforcement surface, not skill-front-matter, precisely because the metadata system is unshipped."
**Recommendation:** No change required; accept the v1 limitation. Track the dependency on card 0022 in the spec's `next_actions` or via a follow-up spec gated on card 0022 maturity.

---

## Honest Assessment

The spec is well-grounded — every cited file, line number, and substrate idiom checks out against the codebase. The first BLOCK clearly did its job: the SQLite-vs-files boundary, the labels-vs-tags resolution, and the session-id collision concern have all been corrected. The Track A and Track B split is clean. The schema additions match the existing TaskEvent/NoteEvent precedent precisely.

The biggest risk is **Finding 1 — the agent learning loop cannot actually close with these ACs.** The agent gets recurrence counts but never sees the correction text that explains what went wrong. The fix is small (extend ac-04's response shape) but if it ships unfixed, the v1 loop is half a loop — agents detect repeated failures but lack the substrate to act on them, and Hugh will still have to restate. That defeats the headline goal.

Second-tier risks are the session-id deletion ambiguity (Finding 2) and the `/tmp` race (Finding 3) — both produce silent data-corruption failure modes under realistic pillar-4 concurrent-agent workloads. Cheap to fix, very expensive to debug if shipped.

The remaining MEDIUMs are quality-of-life: tighter prose, explicit MCP delivery contract, portability of the shell hook. Worth fixing in the same pass but not loop-breaking on their own.

Plan is **close to ready** — REQUEST_CHANGES, not BLOCK. The architecture is sound; the holes are at the contract edges, not the foundation.
