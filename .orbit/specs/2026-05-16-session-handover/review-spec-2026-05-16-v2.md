# Spec Review

**Date:** 2026-05-16
**Reviewer:** Context-separated agent (fresh session)
**Spec:** 2026-05-16-session-handover
**Verdict:** APPROVE

---

## Review Depth

| Pass | Triggered by | Findings |
|------|-------------|----------|
| 1 — Structural scan | always | 0 |
| 2 — Assumption & failure | content signals (schema migration, settings.json, cross-system Stop hook) revisit of cycle-1 findings | 0 |
| 3 — Adversarial | not triggered — no structural concerns after cycle-1 edits | — |

## Cycle-1 → Cycle-2 finding closeout

Re-checking each cycle-1 finding against the edited spec:

| Cycle-1 finding | Severity | Status | Evidence in cycle-2 spec |
|-----------------|----------|--------|---------------------------|
| AC-02 collides with shipped 0.2 → 0.3 migration | HIGH | **CLOSED** | AC-02 now targets `0.3 → 0.4`; goal restated to "schema-version bumps 0.3 → 0.4"; verification renames cover `migrate_0_3_to_0_4_writes_new_version_...`, `migrate_already_at_0_4_is_noop`, and `migrate_0_1_to_0_4_chains_through_0_2_and_0_3` (all three steps asserted in order); a new `migrate_0_2_to_0_4_runs_remaining_steps` asserts the ac-taxonomy `time_gated` step still fires for fixtures created before this spec. The 0.3 → 0.4 framing is explicitly called out as "extends the chain rather than colliding". |
| AC-08 grep assertion will not match actual Stop hook | HIGH | **CLOSED** | AC-08 now quotes the actual baseline verbatim: `(orbit session distill 2>/dev/null && rm -f .orbit/.session-id) || true`, names the parens + `2>/dev/null` + `\|\| true` wrapper as MUST-preserve regression risk, and the grep assertion is rewritten with `-F` (literal match) against the full new command `(orbit session distill 2>/dev/null && rm -f .orbit/.session-id .orbit/.session-card) || true`. The negative grep `grep -F "rm -f .orbit/.session-id)" .claude/settings.json` returns zero hits — catches the case where the implementer leaves the closing paren in the wrong spot. |
| Stale line-number citations | MEDIUM | **CLOSED** | All `schema.rs:NNN` and `verbs.rs:NNN` citations are gone. AC-01 now reads `locate via grep -n "Session::FIELDS" orbit-state/crates/core/src/schema.rs`. AC-07 now reads `locate via grep -n "fn session_prime" verbs.rs` and refers to the `item_bound` formula by name + relative position ("a few dozen lines below the fn header"). Symbol-name references are durable; the staleness risk is eliminated. |
| AC-03 / AC-08 contract drift on `.session-card` deletion | MEDIUM | **CLOSED** | AC-10 gains explicit clauses (g) "mid-session re-set-card is legal and overwrites — latest write wins" and (h) "direct verb-call leak case named — distill is read-only on `.session-card`, the hook owns deletion". The convention now closes both gaps. |
| AC-05 lacks non-UTF-8 stdin test | MEDIUM | **CLOSED** | AC-05 algorithm rewritten to `String::from_utf8_lossy(&buf).into_owned()` — the verb never panics on non-UTF-8 input; U+FFFD replacements land in the distillate. New test `distill_non_utf8_stdin_does_not_panic` feeds invalid-UTF-8 bytes and asserts exit=0 with the lossy-converted output. |
| AC-07 next_step prepend brittleness | MEDIUM | **CLOSED** | AC-07 description now mandates "tests assert via `starts_with` on the sentinel prefix, NOT full-string equality". Verification test renamed `prime_next_step_starts_with_handover_sentinel_when_present` and the description spells out "uses `starts_with`, NOT full-string equality, so future copy edits to the suggestion text do not break the test". |
| AC-11 memory key versioning | LOW | **CLOSED** | Memory key now shaped `session-handover-brew-smoke-passed-<version>` (with `<version>` taken from `orbit --version` output). The version suffix prevents overwriting on subsequent smoke runs. |

## Pass 1 — Structural scan (cycle-2 re-run)

1. **AC testability** — every AC names a concrete test or grep assertion. No vague criteria.
2. **Constraint conflicts** — none found. The schema chain is now consistent (0.3 → 0.4 on top of the today-landed 0.2 → 0.3 ac-taxonomy step). The Stop-hook command preserves the error-tolerance wrapper. The atomic-write + hook-deletes-file ownership boundary is documented.
3. **Scope vs goal** — eleven ACs map cleanly to the goal: Session field + migration (AC-01, AC-02), CLI surface (AC-03–06), prime envelope (AC-07), settings.json (AC-08), choice + convention (AC-09, AC-10), brew smoke (AC-11). No over-specification, no under-specification.
4. **Obvious gaps** — error paths covered (`Error::not_found` on unknown card, defensive fall-through on malformed Stop payload, lossy UTF-8 on non-UTF-8 stdin, idempotent migration, `handover: null` on absent sessions dir). Rollback: AC-02's migration is structurally no-op, so the change is forward-compatible without a separate rollback path. Idempotency rules named on every mutating verb.
5. **Gate-AC description check (deterministic, no LLM judgement)**: gates are AC-01, AC-02, AC-05, AC-07, AC-08 (per `is_gate=1` from `orbit-acceptance.sh acs`). All five descriptions: non-empty, no placeholder tokens, all > 20 chars trimmed (each is several hundred chars). Pass.
6. **Content signals** — schema migration (AC-02), settings.json edit (AC-08), cross-system Stop hook (AC-05, AC-08) — all present, all addressed with explicit tests, regression assertions, and named failure modes.

## Pass 2 — Assumption & failure (re-triggered by content signals)

1. **Assumption audit** — each load-bearing assumption now has a test or named scope-out:
   - On-disk Session YAML files parse cleanly under the new schema (AC-01 `session_legacy_yaml_parses_with_none_card_id`).
   - Migration chain is forward-only and idempotent (AC-02 `migrate_already_at_0_4_is_noop` + `migrate_0_1_to_0_4_chains_through_0_2_and_0_3`).
   - Stop-hook JSON payload shape is `{ hook_event_name: "Stop", last_assistant_message: <string> }` (AC-05 fixture-driven test; non-conforming payloads fall through, never error).
   - `.session-card` ownership: written by `set-card`, read by `distill`, deleted by hook only (AC-10 clauses g + h).

2. **Failure mode analysis** —
   - Environment differences (cron vs interactive): the `|| true` wrapper preserves Stop-hook resilience to missing-binary on fresh clones (AC-08 explicit MUST-preserve).
   - Permission / path: `.orbit/.session-card` is written via `write_atomic` (AC-04) — same atomicity guarantees as the rest of the substrate.
   - Race conditions: latest-write-wins is documented for both `set-card` (AC-10 g) and `distill` (AC-03 idempotency rule).
   - Non-UTF-8 input: lossy conversion (AC-05) means the verb never panics; the operator sees U+FFFD instead of silent loss.

3. **Test adequacy** — verification methods now match the AC claims byte-for-byte:
   - AC-08's grep is anchored on the full literal wrapper (positive) and on the old-shape closing-paren-after-`.session-id`)` (negative).
   - AC-07's `starts_with` assertion decouples from future next_step copy edits.
   - AC-11's versioned memory key keeps a per-release audit trail rather than a single overwriting sign-off.

No Pass-3 triggers. No cascade, no rollback ambiguity, no impact-radius surprises.

## Findings

None.

---

## Honest Assessment

Cycle-2 of the spec closes all seven cycle-1 findings without introducing new ones. The two HIGH blockers (AC-02 schema collision, AC-08 stale Stop-hook baseline) are squarely fixed: AC-02 now extends the chain to 0.4 with a five-test verification matrix that includes a chained 0.1 → 0.4 assertion, and AC-08 preserves the error-tolerance wrapper with a literal-`grep -F` assertion plus a negative-grep regression check. The five MEDIUMs and one LOW are all closed with edits that materially improve testability — the AC-07 `starts_with` rewrite and the AC-05 `from_utf8_lossy` rewrite both narrow the implementer's path and widen the defensive surface.

The biggest residual risk is the brew-smoke AC-11 being deferred (legitimately, per `ac_type: ops`) — but that's substrate-design-correct: the verification cannot run until after a `/orb:release` cycle ships the binary. The spec.close two-band rule already handles this.

Ready for implement.
