# Skill self-improvement — v1 convention

This convention codifies how an agent decides to edit a `SKILL.md` file
based on recorded invocation outcomes. It is the v1 enforcement surface
because card **0022** (skill curator metadata — `created_by`, `pinned`
front-matter) has not yet shipped. When 0022 lands, the metadata-driven
enforcement supersedes this allowlist; this convention is a deliberate
stopgap.

## The substrate

Skill invocations are recorded as an append-only JSONL stream at
`.orbit/skills/<skill_id>.invocations.jsonl`. Each row is one
`SkillInvocation`:

```jsonc
{
  "skill_id": "design",
  "session_id": "5b7e…",
  "outcome": "incorrect",          // worked | partial | didnt-apply | incorrect
  "correction": "missed the cold-fork contract",  // optional, free text
  "timestamp": "2026-05-15T12:00:00Z"
}
```

Two CLI verbs operate on this stream:

- `orbit skill record-invocation <skill_id> --outcome <enum> [--correction <str>]`
  — append one row.
- `orbit skill recurrence <skill_id> [--since <iso-date>]` — read with
  per-outcome counts and the recorded invocations.

Both verbs are available equivalently over MCP.

## The recurrence threshold

**An agent edits a `SKILL.md` file only when the same-skill / same-outcome
recurrence count is at least 2 across sessions.**

The count tells the agent *whether* to edit. The `correction` text on the
recurrence response tells the agent *what to change*.

One-off failures (count < 2) route to `orbit memory remember`, not a
`SKILL.md` edit. A single bad run might be an outlier; two converging
failures with shared corrections are signal.

## Routing rules

| Recurrence | Route                                                     |
|------------|-----------------------------------------------------------|
| count < 2  | `orbit memory remember <skill-key> "<one-line finding>"`  |
| count ≥ 2  | Read `by_outcome.<outcome>.invocations[].correction`, edit `plugins/orb/skills/<skill>/SKILL.md` |

The CLI/MCP `skill.recurrence` response shape always includes every
outcome key (`worked`, `partial`, `didnt-apply`, `incorrect`) even when
the count is zero, so the agent can index without first checking for
missing keys.

## v1 allowlist

The following skills are agent-editable under this convention. They were
chosen because they already produce structured outputs the agent can
reason about with the recurrence stream:

- `card`
- `design`
- `discovery`
- `implement`
- `review-spec`
- `spec`

Skills outside this list MUST NOT be live-edited by an agent under this
convention. The author edits those by hand. When card 0022 ships its
curator metadata (`created_by`, `pinned`), the allowlist retires —
metadata becomes the enforcement surface.

## Worked example

The agent runs `/orb:design` against a card. A reviewer flags that the
design session over-indexed on edge cases instead of the happy path.
The agent records:

```bash
orbit skill record-invocation design \
  --outcome incorrect \
  --correction "over-indexed on edge cases; happy path under-specified"
```

A few sessions later, a second `/orb:design` run drifts the same way.
The agent records another `incorrect` row with a similar correction.

At the start of the next session, the agent queries:

```bash
orbit --json skill recurrence design
```

The response shows `by_outcome.incorrect.count == 2` and the two
`correction` strings. The agent reads both, notices the recurring
pattern ("happy path under-specified"), and edits
`plugins/orb/skills/design/SKILL.md` to add an explicit step that
forces a happy-path scenario before edge-case enumeration. The edit
references the corrections as the load-bearing evidence.

## What this convention does NOT do

- It does NOT modify any `SKILL.md` file at convention-install time. The
  v1 surface is the substrate + this prose. The agent decides when to
  edit, guided by the recurrence stream.
- It does NOT enforce the allowlist via metadata in skill front-matter.
  That's card 0022's job; this convention is the stopgap.
- It does NOT prescribe a specific edit format. The agent reasons about
  the corrections and writes the smallest change that addresses the
  recurring failure mode.
