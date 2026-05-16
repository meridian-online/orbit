# Design: Session handover — outgoing agent leaves a forward prompt the next session reads first

**Date:** 2026-05-16
**Interviewer:** orb:design (claude-opus-4-7)
**Card:** .orbit/cards/0036-session-handover.yaml
**Mode:** open

---

## What good looks like

I open a new Claude Code session on orbit and within seconds I know exactly what the last session was in the middle of. The previous agent's handover sits at the top of `orbit session prime` — one short brief naming what's in flight, the next concrete step, and any context that would cost real time to reconstruct from git, memos, or specs. I don't have to grep. I don't have to ask. I don't have to read three specs to figure out where to pick up. The session before mine paid the compression cost; I just read and start.

---

## Context

**Card:** *Session handover — outgoing agent leaves a forward prompt the next session reads first* — 4 scenarios, goal: cross-session pickup drops from "browse to reconstruct" to "read one prompt".

**Prior specs:** 0 — card was written this session (distilled from `.orbit/memos/2026-05-16-handover-prompt-skill.md`, now consumed).

**Adjacent shipped infrastructure (constraints, not questions):**
- `Session` entity at `orbit-state/crates/core/src/schema.rs:457` with fields `id`, `started_at`, `ended_at`, `distillate: String`, `labels: Vec<String>`. The `distillate` doc-comment at line 468 explicitly says *"The agent's end-of-session reflection — free-text markdown"*.
- `orbit session distill` verb (ac-05 of spec 2026-05-15-agent-learning-loop) — idempotent on `session_id`, writes `.orbit/sessions/<id>.yaml`.
- Stop hook in `.claude/settings.json` fires `orbit session distill && rm -f .orbit/.session-id` — currently pipes the raw Stop-hook JSON event into `distillate` rather than agent-curated prose.
- `orbit session prime` (`verbs.rs:2867`) at SessionStart returns `item_bound`, `open_specs`, `memories`, `next_step` — does **not** currently surface session handovers.

**Gap:** the *carrier* (Session.distillate) exists and is intent-aligned per the schema doc-comment; what's missing is (a) content discipline (curated prose, not raw JSON), (b) per-card scoping, (c) a SessionStart surfacing wire that puts the latest handover at the top of `orbit session prime`.

## Q&A

### Q1: Failure mode
**Q:** What's the failure mode the handover is mainly trying to prevent — re-discovery cost, wrong direction, or both?
**A:** **Both equally.** Re-discovery and drift are real costs. The handover must address both — terse orientation plus an explicit next step.

### Q2: Register
**Q:** How discursive should the handover prose feel — terse BLUF brief, discursive reflection, or hybrid?
**A:** **Discursive reflection.** What I tried, what didn't work, what's left, what I'd do next — multi-paragraph orientation that carries judgement, not just direction.

### Q3: Concurrency
**Q:** Should the design account for parallel sessions (rally fan-out, multiple cards in flight) or just sequential pickup?
**A:** **Per-card scope.** Handover keys by card-id (or spec-id), not just session-id. Picking up a card on a new session reads that card's last handover specifically.

### Q4: Staleness
**Q:** How should the previous handover lose its claim on the new session's attention — always-latest, decay past a boundary, or author-dismisses?
**A:** **Always the latest.** Whatever session ran last for that card is what surfaces. No time decay, no commit-boundary, no ack verb. Simplest rule.

---

## Summary

### Goal
Every session ends with a deliberate, per-card handover artefact written in a discursive reflective register. Every session that picks up a given card reads the most recent handover for that card at SessionStart. Cross-session pickup time drops from "browse to reconstruct" to "read one orientation paragraph and start".

### Constraints
- **Register: discursive reflection.** What was tried, what worked, what didn't, where the agent would pick up. Multi-paragraph. Carries judgement. *Distinct from* agent-to-Hugh BLUF prose — the audience is the next agent (and Hugh reading over its shoulder), not Hugh deciding from the brief.
- **Scope: per-card.** Handover is keyed by card-id (or spec-id), not by session-id alone. The session-id is metadata; the card-id is the lookup key.
- **Staleness: always-latest.** No time decay, no commit-boundary, no ack-clearing. The most recent handover for a card surfaces; older handovers are history.
- **Both failure modes addressed.** The handover format must prevent both re-discovery cost (orientation: what's the state of the world?) and wrong direction (next step: what should I do?).
- **Carrier must round-trip.** Whatever entity holds the handover lives under the orbit-state files-canonical regime — YAML or markdown, schema-versioned, deny_unknown_fields where applicable.

### Success Criteria
- A new session on orbit, opened after a previous session closed cleanly, displays the last session's handover for the most-relevant card at the top of `orbit session prime`'s envelope.
- The handover prose was written deliberately by the outgoing agent (curated reflection, not raw JSON, not transcript dump).
- Picking up a different card surfaces a different handover — per-card scope is observable, not just metadata.
- The "always-latest" rule is observable: writing a new handover for a card immediately supersedes the previous one with no decay or boundary checking.

### Decisions Surfaced
- **Register: discursive reflection chosen over terse-BLUF and hybrid.** The handover is reading-for-orientation prose, not a decision brief. Worth a choice file (`.orbit/choices/NNNN-handover-register-is-discursive.yaml`) once spec'd — explicitly noting it diverges from `.orbit/STYLE.md`'s BLUF discipline because the *audience and purpose* differ (agent orientation, not Hugh deciding).
- **Scope: per-card chosen over sequential-only and both.** Drives the schema question — current `Session` entity has no card-id field. Implementation must add one, or introduce a new `Handover` entity keyed on card.
- **Staleness: always-latest chosen over boundary-decay and ack-clearing.** Simplest rule. No retention policy, no event hook, no new verb. Older handovers remain on disk as history but do not surface.

### Implementation Notes
*Routed from observations during evidence loading and from the implementation-question filter. Starting context for the implementing agent — not author-facing.*

- **Carrier shape — three viable approaches, all implementation-level:**
  1. **Add `card_id: Option<String>` to `Session` entity** (`schema.rs:457`). Schema-version bump from 0.2 → 0.3. Single-card-per-session assumption; multi-card sessions write multiple Session files or accept "last card touched". Cheapest delta.
  2. **New `Handover` entity** at `.orbit/handovers/<card_id>/<session_id>.yaml` or similar. Cleanest separation but introduces a new entity type and folder (worth weighing against the folder-proliferation flag the author raised in this session).
  3. **Sidecar under the card's own folder** at `.orbit/cards/<NNNN-slug>/handover.md` — requires cards to become folders (they're currently flat `.orbit/cards/<id>.yaml`). Aligns with the spec-folders pattern (choice 0021) but is a substrate-shape change well beyond this card's scope.
  Recommend approach (1) for v1 — minimum delta, reuses shipped Session entity, doc-comment on `distillate` already says "agent's end-of-session reflection".
- **Surfacing wire in `orbit session prime`** (`verbs.rs:2867`) — add a `handover` field to the envelope, populated by the most recent Session with matching card_id. The "most-relevant card" lookup is the design question (heuristic: most recent card across all open specs? card touched in the previous session? card passed via env var?). Defer the heuristic choice to spec time; v1 can simply surface "last session's handover regardless of card" and add per-card lookup as a follow-on.
- **Stop hook command** in `.claude/settings.json` currently pipes raw JSON. The hook must change so the *outgoing agent* writes the handover prose to stdin before Stop fires. Two viable shapes: (a) the agent writes a markdown file the hook reads, (b) the agent puts the handover in its final assistant message and the hook extracts it. (a) is more reliable; (b) is more native to the Stop-hook protocol. Worth a choice file at spec time.
- **New `orbit session handover` CLI/MCP verb** likely needed for explicit `--card <id>` writes and for the `orbit session prime` surfacing to query by card_id without re-parsing every Session file.
- **Choice file for the register decision** (discursive vs BLUF) is worth landing alongside the spec — the STYLE.md contract is project-wide, and a deliberate "register varies by audience" call should be documented as a MADR.
- **Card 0024-lean-pass interaction:** sessions accrete monotonically. If approach (1) lands, no new folder. If approach (2) lands, `.orbit/handovers/` is a new folder and should be added to card 0024's category list before it ships.

### Open Questions
- None of consequence. The four answered questions plus the user-voice paragraph constrain the design space tightly. The remaining decisions are implementation-shaped and routed to the implementation-notes block above.
