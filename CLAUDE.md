# orbit

This repo is the orbit workflow plugin for Claude Code. Sessions here are about **workflow refinement** — improving how orbit guides the card → design → spec → implement → review pipeline.

Agent-to-Hugh prose follows the BLUF / Decision Brief contract — see card 0026 (`.orbit/cards/0026-executive-communication.yaml`). The canonical source is `.orbit/STYLE.md`, imported below.

@.orbit/STYLE.md

## What This Is

orbit is a Claude Code plugin that provides specification-driven workflow skills (`/orb:card`, `/orb:distill`, `/orb:design`, `/orb:spec`, `/orb:implement`, `/orb:review-pr`, etc.). The skills, hooks, and card format are the product.

## The four pillars

orbit exists to deliver four user outcomes. Every card, skill, and design choice in this repo is justified by moving at least one of them. When work doesn't move a pillar, question whether it belongs in orbit at all.

1. **Executive-level interaction** — the author has clear vision but is managing multiple things and has no time to digest each artefact. Orbit's output is concise and actionable; agents pay the compression cost so the author doesn't.
2. **Agent self-learning** — agents save their own memory and grow their skillset across sessions. Discovered facts and recurring procedures accrete into the substrate so future sessions don't re-discover them.
3. **Agent state-persistence** — durable state keeps agents on track through context loss, session death, and fan-out. The author won't read most of it; its job is to serve the agent.
4. **Long-running R&D** — agents do a full session's work before checking in. Start/stop kills progress; the author wants depth between check-ins so each check-in is a real review, not a course-correction.

These are the load-bearing why-test. They are not interchangeable with "things orbit happens to do." Cards that claim a pillar should be able to defend the claim with a measurable mechanism.

## Working in This Repo

- **Skills live in** `plugins/orb/skills/<name>/SKILL.md`
- **Cards describe orbit's own capabilities** in `.orbit/cards/`
- **Specs for orbit changes** live in `.orbit/specs/`
- orbit uses itself — cards, specs, and decisions apply to orbit's own development

## Key Concepts

- **Cards are living documents.** They describe capabilities, not work items. Updated in place; git history is the audit trail.
- **First-principles lens.** Distill asks "what does this product do?" not "what's planned next?"
- **No backlogs.** Work flows through decisions and specs. Cards are the feature taxonomy.

## Orbit vocabulary

Each artefact has one job. Don't invent new names — if something doesn't fit, it probably needs a different existing artefact, not a new one.

| Artefact    | Where                                                       | What it is                                                                                                                   |
|-------------|-------------------------------------------------------------|------------------------------------------------------------------------------------------------------------------------------|
| Card        | `.orbit/cards/NNNN-<slug>.yaml`                              | A capability the product provides. Written in user language. Never closed — updated in place as the capability evolves.       |
| Memo        | `.orbit/cards/memos/<date>-<slug>.md`                        | Raw idea awaiting distillation. Freeform markdown. Turned into cards via `/distill`. Deleted after promotion.                |
| Interview   | `.orbit/specs/<date>-<slug>/interview.md`                    | Q&A record from a `/design` or `/discovery` session. Feeds the spec.                                                          |
| Spec        | `.orbit/specs/<date>-<slug>/spec.yaml`                       | A discrete unit of work with numbered acceptance criteria. One card may spawn many specs over time.                          |
| Progress    | `.orbit/specs/<date>-<slug>/progress.md`                     | AC tracker maintained during implementation. The implementation diary.                                                       |
| Review      | `.orbit/specs/<date>-<slug>/review-{spec,pr}-<date>.md`      | Verdict artefact from `/review-spec` or `/review-pr`.                                                                         |
| Decision    | `.orbit/choices/NNNN-<slug>.md`                            | MADR record of an architectural choice. Referenced by specs that respect it.                                                 |
| Rally state | `.orbit/specs/<date>-<slug>-rally/rally.yaml`                | Durable state for a multi-card rally. Owned by the rally lead. Rally folders live alongside card spec folders — no separate archive.|
| Drive state | `.orbit/specs/<date>-<slug>/drive.yaml`                      | Durable state for a single-card drive. Owned by the drive agent.                                                             |

**Cards describe *what*, specs describe *work*.** When someone asks to "make a card for X":

- Is X a capability the product provides? → card in `.orbit/cards/`.
- Is X a discrete piece of work with acceptance criteria? → spec via `/design` + `/spec`.
- Is X a rough idea you don't want to lose? → memo via `/memo`.
- Is X a retrospective, options memo, or investigation plan? → none of the above. Retrospectives update the card they're about; options memos become `/discovery` sessions; investigation plans become specs.

**Cards never close.** A card may reach `maturity: established` and stop acquiring specs, but it isn't archived or deleted. The card is the product's self-description; the specs underneath it are the work.

**"Follow-up cards" is usually wrong.** If a session surfaces follow-up work, it's almost always new specs against existing cards — not new cards. New cards are for new capabilities, not for splitting work.

## Deployment

The plugin is installed into projects via the Claude Code plugin marketplace. All development happens in this repo — installed copies in other projects receive updates via the marketplace.


<!-- BEGIN ORBIT-STATE INTEGRATION -->
## Orbit-state Substrate

This project uses **orbit-state** as its agent substrate — files-canonical state under `.orbit/` (cards, choices, specs, tasks, memories), with a SQLite index and an MCP server that share the same Rust core. Run `orbit session prime` at session start.

### Quick Reference

```bash
orbit session prime         # Surfaces open specs + recent memories
orbit task ready            # Claimable work (open, no claim)
orbit task show <id>        # Inspect a task
orbit task claim <id>       # Claim a task
orbit task done <id>        # Complete a task
orbit spec list             # Open specs
orbit memory remember <key> "<body>"  # Persist a decision across sessions (key is a short stable id)
orbit memory search <kw>    # Search prior memories
```

### Rules

- Use `orbit` verbs for ALL task and spec tracking — do NOT use TodoWrite, TaskCreate, or markdown TODO lists.
- Run `orbit session prime` at the start of every session.
- Use `orbit memory remember` for persistent knowledge — do NOT use MEMORY.md files.

## Session Completion

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File tasks for remaining work** — open new tasks under the active spec for anything that needs follow-up.
2. **Run quality gates** (if code changed) — tests, linters, builds.
3. **Update task status** — mark finished tasks done; append updates on in-progress items.
4. **PUSH TO REMOTE** — this is MANDATORY:
   ```bash
   git pull --rebase
   git push
   git status  # MUST show "up to date with origin"
   ```
5. **Clean up** — clear stashes, prune remote branches.
6. **Verify** — all changes committed AND pushed.
7. **Hand off** — provide context for next session.

**CRITICAL RULES:**
- Work is NOT complete until `git push` succeeds.
- NEVER stop before pushing — that leaves work stranded locally.
- NEVER say "ready to push when you are" — YOU must push.
- If push fails, resolve and retry until it succeeds.
<!-- END ORBIT-STATE INTEGRATION -->
