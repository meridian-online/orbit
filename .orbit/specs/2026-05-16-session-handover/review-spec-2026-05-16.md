# Spec Review

**Date:** 2026-05-16
**Reviewer:** Context-separated agent (fresh session)
**Spec:** 2026-05-16-session-handover
**Verdict:** REQUEST_CHANGES

---

## Review Depth

| Pass | Triggered by | Findings |
|------|-------------|----------|
| 1 — Structural scan | always | 4 |
| 2 — Assumption & failure | structural concerns + content signals (schema migration, settings.json, cross-system Stop hook) | 3 |
| 3 — Adversarial | not triggered — Pass 2 findings are spec-precision bugs, not cascade-or-impact problems | — |

## Findings

### [HIGH] AC-02 collides with the already-shipped 0.2 → 0.3 migration
**Category:** constraint-conflict
**Pass:** 1
**Description:** AC-02 instructs the implementer to bump `CURRENT_SCHEMA_VERSION` from `"0.2"` to `"0.3"` and register a new `("0.2", "0.3", migrate_add_card_id_to_session)` step. That slot is already taken on `main` — `CURRENT_SCHEMA_VERSION` is **already `"0.3"`**, and the registry already contains `Migration { from: "0.2", to: "0.3", apply: migrate_time_gated_to_ac_type }` from spec `2026-05-16-ac-taxonomy` ac-03. Running AC-02 as written would either silently overwrite the ac-taxonomy migration (data corruption: existing `time_gated` ACs never get remapped on future clones) or be physically impossible because the version constant is already at the target. The whole AC needs re-aiming at 0.3 → 0.4, including the chained-migration test and the `migrate_already_at_0_3_is_noop` framing.
**Evidence:**
- `orbit-state/crates/core/src/migrations.rs:24` — `pub const CURRENT_SCHEMA_VERSION: &str = "0.3";`
- `orbit-state/crates/core/src/migrations.rs:52-65` — registry already holds the 0.1→0.2 and 0.2→0.3 entries.
- `.orbit/schema-version` reads `version: '0.3'` on disk in this repo.
- Spec text (ac-02): *"`CURRENT_SCHEMA_VERSION` ... bumps from `"0.2"` to `"0.3"`. A new migration step `("0.2", "0.3", ...)` registers ..."* — both claims are out of date.
**Recommendation:** Rewrite AC-02 to bump 0.3 → 0.4 with the new step `("0.3", "0.4", migrate_add_card_id_to_session)`. Rename the unit tests accordingly: `migrate_0_3_to_0_4_writes_new_version_...`, `migrate_already_at_0_4_is_noop`, `migrate_0_1_to_0_4_chains_through_0_2_and_0_3`. The chained-migration test must assert all three steps fire in order, not just two.

### [HIGH] AC-08 grep assertion will not match the actual Stop hook command
**Category:** test-gap
**Pass:** 1
**Description:** AC-08 specifies the current Stop-hook command as `orbit session distill && rm -f .orbit/.session-id` and proposes the new command `orbit session distill && rm -f .orbit/.session-id .orbit/.session-card`. The actual `.claude/settings.json` line is `(orbit session distill 2>/dev/null && rm -f .orbit/.session-id) || true` — wrapped in parens, with stderr suppression, and a `|| true` fallthrough so a missing `orbit` binary on a fresh clone does not abort SessionStop. The AC's grep verification (`grep -E "orbit session distill.*rm -f .orbit/.session-id .orbit/.session-card" .claude/settings.json` returns one hit) **will accidentally match** if the implementer naively appends `.session-card` after the `.session-id` token inside the existing wrapper — but the AC text reads as a full-command-replacement instruction. The implementer is being told to write one shape while the verification accepts a different shape. Worse, if they take the AC literally and replace the line with the spec's shorter form, the error-tolerance wrapper is lost and a future fresh clone's missing-orbit-binary case re-breaks SessionStop.
**Evidence:**
- `.claude/settings.json:33` — `"command": "(orbit session distill 2>/dev/null && rm -f .orbit/.session-id) || true"`
- AC-08 description quotes the wrong baseline (`orbit session distill && rm -f .orbit/.session-id`), missing the wrapper.
**Recommendation:** Rewrite AC-08's description to quote the actual baseline verbatim and specify that **only the `rm -f` argument list changes** — the parens, the `2>/dev/null`, and the `|| true` wrapper stay. The new command should read `(orbit session distill 2>/dev/null && rm -f .orbit/.session-id .orbit/.session-card) || true`. Update the grep assertion to anchor on that exact substring.

### [MEDIUM] Line-number citations across the spec are stale
**Category:** missing-requirement
**Pass:** 1
**Description:** The spec cites specific line numbers (`schema.rs:457`, `schema.rs` line 106 for `FIELDS`, `verbs.rs:2867`, `verbs.rs:2869`) as if load-bearing landmarks. Current code:
- `Session` struct is around line 458 (close enough), but `Session::FIELDS` is at line 105–107 (one line off).
- `session_prime` is at `verbs.rs:3008`, not `2867`.
- The `item_bound` formula is at `verbs.rs:3046`, not `2869`.
The line numbers are not load-bearing — symbols are found by name — but the staleness suggests the spec was drafted against an older tree, and the implementer following these cues will burn a beat verifying each. More importantly, *if* a future spec ever cites these line numbers via `<file>:<line>` and a downstream tool actually treats them as the seek target, the wrong code gets touched.
**Evidence:**
- `orbit-state/crates/core/src/verbs.rs:3008` (session_prime), :3046 (item_bound formula).
- `orbit-state/crates/core/src/schema.rs:105-107` (Session::FIELDS const).
**Recommendation:** Either delete the line-number citations and reference symbols by name only (`Session::FIELDS`, `session_prime`, the `item_bound` line of `session_prime`), or refresh them against the current tree before promoting to implement. Symbol-name references are durable; line numbers rot on every commit. Strong preference for symbol names.

### [MEDIUM] AC-03 / AC-08 contract drift: who owns `.session-card` deletion?
**Category:** assumption
**Pass:** 1
**Description:** AC-03 says the distill verb *reads* `.orbit/.session-card` when no `--card` flag is passed. AC-08 says the Stop-hook command line *deletes* `.orbit/.session-card` after distill (alongside `.session-id`). Two assumptions stack here that the spec does not validate:
1. **Distill itself does not delete the file.** Today, the equivalent `.session-id` file is *not* deleted by `orbit session distill` — it is deleted by the shell `&&`-chain in the hook. The spec implicitly assumes the same hand-off for `.session-card`. That's fine, but it makes the hook command the single source of cleanup; if the hook is misconfigured or the user invokes `orbit session distill` directly without the shell wrapper, `.session-card` leaks across sessions and the next session inherits stale card scoping.
2. **Mid-session re-`set-card` is intentional.** AC-04 says `set-card` writes atomically and a second call overwrites. AC-08's verification specifies a single `set-card` per session. The spec does not say what happens if an agent calls `set-card 0036` and then later calls `set-card 0099` in the same session — the latest wins, the distillate gets `0099`, the in-flight intent against `0036` is lost. This is a reasonable default but should be documented in the convention (AC-10) so agents do not race themselves.
**Evidence:** AC-03 ("falls back to reading `.orbit/.session-card`"), AC-04 ("atomic", "second call overwrites"), AC-08 (deletion in hook command line). No AC names the leak case.
**Recommendation:** Add one sentence to AC-10's convention text explicitly stating that (a) the agent SHOULD call `set-card` once per session, early, and (b) re-calling `set-card` mid-session is permitted but the latest wins. Also add a one-line note to AC-03's verification listing `distill_does_not_delete_session_card_file` as a defensive test — the file is the hook's responsibility, not the verb's.

### [MEDIUM] AC-05 lacks a "non-UTF-8 / bytes payload" test case
**Category:** failure-mode
**Pass:** 2
**Description:** AC-05's algorithm reads stdin into a `String`, then attempts `serde_json::from_str`. If a Claude Code Stop-hook delivers a payload containing non-UTF-8 bytes (rare but not impossible — a transcript path with a non-UTF-8 filename, an embedded raw byte from a tool output), `read_to_string` will fail before the JSON parse is attempted. Today's behaviour is presumably "the verb errors and the hook's `|| true` swallows the error" — meaning a non-UTF-8 distill silently loses the entire session record. The AC's defensive framing ("the verb never refuses a distill on stdin shape grounds") does not cover this path because the error happens before the algorithm runs.
**Evidence:** AC-05 description ("read stdin to a `String`; attempt `serde_json::from_str::<serde_json::Value>(&s)`") — String read is implied as fallible-and-fatal.
**Recommendation:** Add one verification line to AC-05: when stdin is not valid UTF-8, the verb falls through to a defensive distillate (e.g. "[stdin was not valid UTF-8 — handover lost]") rather than erroring out. Equivalently, document that this case is out of scope and the existing failure mode (`|| true` swallows) is accepted. Either is fine — silence is the problem.

### [MEDIUM] AC-07 next_step prepend is brittle to future text changes
**Category:** test-gap
**Pass:** 2
**Description:** AC-07 specifies the new behaviour as *"the next_step prepends `"Read the handover above before any other action."` to the existing text"*. The existing text is hard-coded at `verbs.rs:3052`: *"Run `orbit overview` for a single-screen project synthesis (open specs, cards-by-maturity, recent memories, most-connected card, orphans)."*. If a future spec changes that string (e.g. via the `orbit session prime` re-prioritisation work alluded to in spec 2026-05-15-agent-learning-loop), the prepend assertion silently breaks — either the test stops asserting on the right thing, or both strings drift. The test `prime_next_step_prepends_handover_read_when_present` should assert on the *prefix*, not the full string, and should compare the no-handover case's next_step against a captured-from-fixture baseline rather than a hard-coded literal.
**Evidence:** `verbs.rs:3052` — `next_step: "Run \`orbit overview\` ...".into()` (literal). AC-07 verification text mandates `starts with the handover-read suggestion when handover is Some, and matches the prior text when handover is None` — the second clause locks the prior text.
**Recommendation:** Soften the AC-07 verification to *prefix*-only assertions: the next_step starts with `"Read the handover above before any other action. "` when `handover.is_some()`, and the trailing suffix is identical to the no-handover case. This decouples the test from any future change to the base next_step.

### [LOW] AC-11 (ops) verification artefact is under-specified
**Category:** test-gap
**Pass:** 2
**Description:** AC-11 says the smoke sign-off is recorded as a memory `session-handover-brew-smoke-passed`. This is consistent with the ac-taxonomy precedent. Two small gaps: (a) the memory key is singular but the smoke runs once per release, so a second release running the same smoke would either overwrite (loses prior evidence) or fail (idempotency violation). (b) The description specifies "the binary version" should be captured, but does not name the source (`orbit --version`? `brew list --versions orbit`?). Both are addressable in the implementing agent's drive notes but worth pinning before promote so the smoke is reproducible.
**Evidence:** AC-11 description / verification text.
**Recommendation:** Optional: pin the memory key shape to `session-handover-brew-smoke-passed-<version>` so each release adds a fresh record rather than overwriting, and name the version source (`orbit --version` output) explicitly. Defer-friendly — does not block implement.

---

## Honest Assessment

The spec is well-shaped, the eleven ACs hang together as one coherent surface, and the AC-taxonomy types (`code` / `config` / `doc` / `ops`) are correctly assigned. The Stop-hook fix in AC-05 is the load-bearing repair and the algorithm is well-specified. The interview-to-spec compression is faithful.

The biggest risk is AC-02 — the schema-migration AC was drafted against a tree where 0.2 was still current. The ac-taxonomy spec landed an hour ago and took 0.3. The implementer reading AC-02 literally will either no-op (constant is already 0.3) or destructively overwrite the time-gated migration. This is a blocker for implement, but a 30-second rewrite of one AC to target 0.3 → 0.4. AC-08's wrong-baseline-quote of the Stop-hook command is the same shape of problem — drafted-against-stale-tree — and fixable in the same pass. The other findings are spec-precision nits that the implementing agent could route around, but are worth tightening now while the spec is being touched anyway.

Once AC-02 and AC-08 are rewritten and the line-number citations are refreshed (or replaced with symbol-names), this is good to go. No design rework needed — the design is sound; the spec text just needs to catch up with the post-ac-taxonomy tree.
