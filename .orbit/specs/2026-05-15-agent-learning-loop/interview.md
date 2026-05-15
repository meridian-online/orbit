# Discovery: Hermes-style agent learning loop for orbit

**Date:** 2026-05-15
**Interviewer:** Claude (opus-4-7)
**Cards:** `.orbit/cards/0022-skill-curator.yaml`, `.orbit/cards/0023-memory-loop.yaml`
**Mode:** discovery

---

## Context

Hermes Agent (NousResearch) ships a "closed learning loop" — agent-curated memory with periodic nudges, autonomous skill creation after complex tasks, skills that self-improve during use, FTS5 over past conversations. Orbit's pillar 2 ("agent self-learning") names the same outcome but currently *invites* it via CLAUDE.md prose rather than *triggering* it.

Two cards already cover most of the WHAT:

- **Card 0022 skill-curator** — adopted directly from hermes-agent. Metadata-based lifecycle (`created_by`, `pinned`), 30-day flag, 90-day archive, promotion-from-memory path. `maturity: planned`, no specs.
- **Card 0023 memory-loop** — append-only memory, prime auto-injection, audit cadence, "what's not a memory" boundary. `maturity: planned`, no specs.

Three Hermes mechanics neither card addresses yet:

1. **Nudge trigger** — what event prompts the agent to write.
2. **Skill self-improvement during use** — failure modes editing SKILL.md in place.
3. **Cross-session past-conversation search** — FTS5 over transcripts.

Discovery scopes which of those three to pursue.

## Q&A

### Q1: Scope — which mechanics to pursue
**Q:** Which of the three Hermes mechanics not yet in cards 0022/0023 is the priority?
**A:** Cross-session search + skill self-improvement. The nudge trigger was dropped from scope.

### Q2: Binding pillar
**Q:** What's the binding pillar this loop should serve?
**A:** Self-learning (pillar 2).

### Q3: Skill self-improvement trigger
**Q:** What failure signal should trigger a SKILL.md edit?
**A:** Agent's own judgment — Hermes-style live edit, no human in the loop.

### Q4: Search gap — what does cross-session recall recover?
**Q:** What does cross-session search recover that orbit memory/cards/specs miss today?
**A:** Session continuity. Things have to be restated session over session. The outcome we want is *knowledge accumulation without Hugh repeating himself*. Secondary: better memory management may reduce duplication of notes across artefacts (memos, card descriptions, spec notes, memories).

### Q5: Live-edit guardrail
**Q:** What's the guardrail against an agent live-editing a SKILL.md based on a one-off failure?
**A:** Recurrence threshold — same skill, same failure, N times before edit. One-offs become memos, not skill edits.

### Q6: Where session-continuity knowledge lives
**Q:** Where should recovered session-continuity knowledge live?
**A:** Existing memory. Improve `orbit memory` (better prime, consolidation pass). No new artefact type. Heavy mechanics (FTS5 over raw transcripts) and parallel artefacts (session journals) were rejected.

### Q7: Memory mechanics that move the lever
**Q:** Which memory mechanic is the biggest lever on "stop Hugh restating"?
**A:** All four — better prime injection, end-of-session distillation, cross-artefact consolidation, mid-task remember prompts. Unsure which dominates; keep all in scope and let implementation reveal priority.

### Q8: Success criteria
**Q:** How will we know the loop is working?
**A:** Two observable signals:
- **Skill edit cadence** — non-zero (loop firing) and bounded (recurrence threshold working).
- **Memory search result-count distribution** — neither sparse (memories unused/empty) nor flooded (repetition, no consolidation).

### Q9: Runtime surface
**Q:** Where does live tracking for skill failures and session distillation live?
**A:** **orbit-state files for state, CLI verbs for the API, hooks for integration.** *Corrected 2026-05-15 after the first spec attempt was BLOCKed by /orb:review-spec* — the original answer wrongly framed orbit-state as a SQLite database with new tables. The actual orbit-state model (choice 0015) is **files canonical, SQLite as a derived index**. Concretely:

- **Skill invocations** belong in an **append-only JSONL event stream** (the existing `TaskEvent` / `NoteEvent` pattern at `.orbit/specs/<id>.{tasks,notes}.jsonl`). Likely layout: `.orbit/skills/<skill_id>.invocations.jsonl` — per-skill, append-only, not round-trippable, excluded from the CI round-trip gate. Recurrence is a `tail + filter` over the file.
- **Session summaries**, if persisted, are a new **canonical YAML entity** at `.orbit/sessions/<session-id>.yaml` with serde-typed fields — substrate-written, round-trippable, schema-versioned via `.orbit/schema-version` bump.
- **CLI verbs** call into orbit-state's existing operator pattern (read/write through MCP, never raw YAML edits by agents).
- **Hooks** are wired in `.claude/settings.json` under `Stop` / `SessionStart` / `PreCompact` events — there is no separate `plugins/orb/hooks/` directory; hooks invoke commands directly. The Stop hook fires an `orbit` CLI verb at session end.

---

## Summary

### Goal

Agents accumulate competence across sessions through two mechanisms, both grounded in orbit's existing substrate:

1. **Skill self-improvement** — agent-judgment live edits to SKILL.md gated by a same-skill-same-failure recurrence threshold.
2. **Memory-driven session continuity** — Hugh stops restating because the substrate captures, consolidates, and re-injects what was learned. Achieved through four reinforcing mechanics in `orbit memory`: prime injection, end-of-session distillation, cross-artefact consolidation, mid-task remember prompts.

The binding pillar is **agent self-learning (pillar 2)**. Heavy Hermes mechanics (transcript FTS5, periodic nudges, parallel artefact stores) are out of scope.

### Constraints

- **No new artefact types.** Knowledge accumulates in existing `.orbit/memories/`, not a new session-journal or transcript-index store.
- **Recurrence-gated skill edits.** Same skill + same failure mode, N times, before any SKILL.md mutation. One-offs go to memory or memos.
- **Curator (card 0022) handles long-term lifecycle.** Live edits are evaluated at 30-day flag and 90-day archive thresholds like any agent-authored content.
- **orbit-state is the durable store.** New tables (skill invocation log, session summary) live in `.orbit/state.db`. Runtime hooks fire CLI verbs; CLI handles persistence.
- **Runtime-agnostic.** Claude Code hooks are the *integration* layer, not the system of record. Cron jobs, future runtimes, other agent frameworks must work via the same CLI surface.

### Success Criteria

- **Skill edit cadence is non-zero and bounded** — observable from git history on `plugins/orb/skills/*/SKILL.md`. Zero = loop not firing; flood = recurrence threshold too loose. Healthy = 3–10 edits per quarter (calibrate against actual usage).
- **Memory search returns right-sized result sets** — `orbit memory search <query>` should typically return 1–5 hits. Empty results signal memories aren't being written or aren't being matched; flooded results signal duplication / no consolidation. Both are degradation signals.
- **Restatement frequency trends down** — qualitative read from Hugh, captured as a periodic check rather than a metric.

### Decisions Surfaced

- **Skill mutation triggered by agent judgment, gated by recurrence** — chose Hermes-style live edit over propose-don't-edit, edit-then-flag, and pinned-skills-locked. Rationale: live edit is what makes the loop tight; the recurrence threshold is the safety mechanism. Pinned-skills-locked behaviour is *already* in card 0022 (the curator only touches `created_by:agent && pinned!=true`).
- **Session continuity reuses memory, not a new artefact** — chose "existing memory" over new session journal, raw transcript index, or end-of-session distillation only. Rationale: substrate already captures the right shape; the gap is mechanics around it (prime relevance, dedupe, mid-task capture), not a missing artefact type.
- **Nudge trigger out of scope for this discovery** — agent self-judgment for skill edits and CLI/hooks for memory writes are sufficient. Periodic timer-based nudges were explicitly dropped — likely too noisy in long-running R&D sessions (pillar 4 friction).
- **orbit-state (SQLite) for tracking, CLI for verbs, hooks for integration** — matches existing pattern. Worth recording as a MADR (`.orbit/choices/NNNN-learning-loop-runtime-boundary.yaml`) before spec generation so the implementing agent doesn't re-litigate it.

### Implementation Notes

(Means-level observations — starting context for the implementing agent, not requirements. **Revised 2026-05-15** after the first spec attempt was BLOCKed; corrections reflect the actual orbit-state model verified against `schema.rs` and choice 0015.)

- **Build on cards 0022 and 0023, don't duplicate them.** Both cards capture WHAT; this discovery covers the missing HOW. Card 0022's curator already handles agent-authored lifecycle; this spec only needs to add the *creation* and *live-edit* paths feeding it.
- **Use `Memory::labels`, do not invent a `tags` field.** `Memory` in `orbit-state/crates/core/src/schema.rs:327` already carries `labels: Vec<String>` with `#[serde(default)]`. The existing `orbit memory remember/search` verbs already use it. The first spec attempt invented a parallel `tags` field and was correctly blocked. Extend `--label` filtering on `memory.search` if it isn't already there; do not add `--tag`.
- **Skill invocation is an append-only JSONL event stream, not a SQLite table.** Match the existing pattern: `TaskEvent` lives at `.orbit/specs/<id>.tasks.jsonl`; `NoteEvent` at `.orbit/specs/<id>.notes.jsonl`. Plausible layout for skill_invocation: `.orbit/skills/<skill_id>.invocations.jsonl` — per-skill, append-only, not round-trippable, excluded from CI round-trip gates per the same exemption that covers tasks/notes. Each row: `{skill_id, session_id, outcome, correction?, timestamp}`. Recurrence detection is `wc -l` + filter, not a SQL query.
- **Session summaries are a new canonical YAML entity.** If they need to persist beyond the running session: define a `Session` struct in `schema.rs` with `deny_unknown_fields`, layout at `.orbit/sessions/<session-id>.yaml`, fields `{id, started_at, ended_at, distillate, labels}`, register a `Session::FIELDS` const, add the field-drift test alongside the others (`schema.rs:509`), bump `.orbit/schema-version`. Substrate-written, round-trippable, included in CI round-trip gates.
- **Outcome classifier is a serde enum**, not a free-text string: `enum InvocationOutcome { Worked, Partial, DidntApply, Incorrect }` with `#[serde(rename_all = "kebab-case")]`. Match the precedent of `TaskEventKind` (`schema.rs:184`). The optional `correction: Option<String>` is free text and does NOT feed recurrence detection.
- **`orbit skill recurrence` should return per-outcome counts**, not an aggregate sum — the review caught this. The agent calling without `--outcome` needs the breakdown to apply the same-skill-same-outcome threshold. Output shape: `{skill_id, by_outcome: {worked: N, partial: N, ...}, total: N}`.
- **Session id sourcing must not collide on same-day runs.** The first spec attempt suggested `hash(cwd, calendar-day, host)` and was correctly blocked because two distinct sessions on the same day overwrite each other. Use `ORBIT_SESSION_ID` if set; otherwise generate a UUID at session start (the SessionStart hook is the natural author) and persist it for the session's duration (e.g. `.orbit/.session-id` written by the SessionStart hook, read by everything else).
- **Hooks are wired in `.claude/settings.json`**, not in a `plugins/orb/hooks/` directory. The current settings.json (line 1) shows the shape: `{"hooks": {"<event>": [{"hooks": [{"command": "...", "type": "command"}], "matcher": ""}]}}`. The current `bd prime` commands are stale and should be replaced with `orbit session prime` as a side-effect of this work, or in a separate sweep. Available events: SessionStart, PreCompact, Stop, UserPromptSubmit, PostToolUse.
- **Curator front-matter (card 0022) hasn't shipped.** No SKILL.md files currently carry `created_by` / `pinned` / `created_at`. AC-09 of the blocked spec depended on this. Either (a) pull the front-matter rollout into this spec as a predecessor AC, (b) sequence it as a separate predecessor spec, or (c) rewrite the live-edit invariant to use a different signal (e.g. a hardcoded allowlist of agent-authorable skills in the convention file). Option (a) is probably cheapest given they're touching the same skills.
- **End-of-session distillation runs from the `Stop` hook firing `orbit session distill`.** CLI verb should be idempotent — re-running on the same session_id updates the existing YAML (or appends to a stream if we choose the JSONL model for sessions instead). Re-evaluate the YAML-vs-JSONL choice for sessions during /orb:design — sessions don't have natural event-stream semantics like tasks do, which argues for YAML.
- **Hermes parity is not the goal.** Hermes's FTS5 over transcripts and user modelling (Honcho) are deliberately out of scope. If they become necessary later, that's a new card.

### Open Questions

(Several were resolved by the first spec attempt and its BLOCKed review; remaining questions for /orb:spec.)

- **Recurrence threshold N** — proposed value: **2 same-skill-same-outcome across sessions**. Confirm at /orb:spec.
- **Cross-session vs in-session recurrence** — proposed: **cross-session** (safer; in-session is harder to test and risks one-off over-correction). Confirm at /orb:spec.
- **Outcome classifier shape** — **resolved**: serde enum `{Worked, Partial, DidntApply, Incorrect}` with optional free-text `correction`. See implementation notes.
- **Memory tagging vs prime relevance** — **resolved by reading the schema**: memories already have `labels: Vec<String>`; no schema change needed. Prime relevance becomes "filter by labels matching current session context" — heuristic for the context labels is a separate question.
- **Sessions: YAML vs JSONL** — open. YAML if sessions are summaries (one record per session, round-trippable, idempotent updates); JSONL if sessions are event streams (open / message / tool-use / close events). The discovery scoped "session summary", which argues YAML.
- **Curator metadata sequencing** — open. AC-09 of the blocked spec required SKILL.md front-matter that hasn't shipped. Pull into this spec, predecessor spec, or rewrite the invariant?
- **Hook event for distillation** — `Stop` is the natural choice but worth verifying it fires at session-end rather than message-end in the Claude Code lifecycle.
