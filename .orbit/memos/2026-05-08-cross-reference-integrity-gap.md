# Cross-reference integrity — a gap between lean-pass and codebase-mastery

**Date:** 2026-05-08
**Source:** session-close after hydrofoil orbit-state v0.1 cutover (commit `f60be1e`)

## Observation

Hydrofoil's migration to orbit-state v0.1 left ~28 broken
cross-references behind: `.orbit/choices/<id>-<slug>.md` strings
inside test docstrings, `MISSION.md`, card `references[]` fields,
spec bodies, and one MADR — pointing at files that now have a `.yaml`
extension. Plus stale `decisions/` entries in CDK Docker
`exclude=[...]` lists for a directory that no longer exists.

`orbit verify` is clean. The cards parse. The schema is satisfied.
The substrate is sound. But the *plumbing between live artefacts*
rotted, and nothing in the orbit ecosystem currently flags it.

## Why neither existing card catches this

**0024 lean-pass** targets *accretion* — stale memories, duplicate
choices, dead skills, branches past their TTL. Mechanism: mtime,
status, dependency. The broken refs aren't accreted artefacts; the
artefacts are wanted, current, and on-mission. Lean would say
"clean."

**0025 codebase-mastery** targets *edit-time evidence* — cheap
tooling so an agent checks before duplicating. It assumes the agent
is *writing* something new and wants to know what's already there.
A cross-reference audit is post-hoc and substrate-wide; tree-sitter
and ast-grep aren't shaped for it (the references are path strings,
not AST nodes).

**0002 distill** is additive (extract signal). **0023 memory-loop**
dedupes at write-time. **0022 skill-curator** scopes to skills.

The gap is structural: stale plumbing between live artefacts.

## Where the migration tool stops short

`orbit-migrate migrate-a` rewrote ~275 files mapping `orbit/cards/` →
`.orbit/cards/` etc. But the rewrite is *prefix-only* — it didn't
follow `decisions/<id>.md` → `.orbit/choices/<id>.yaml` (extension
change), and it didn't know about content-internal references that
include the file extension (`.md` literals in docstrings, MADR
bodies, the `references[]` array on cards).

So even at migration time, the tool produces a clean substrate but
leaves a tail of broken refs that no follow-up verb sweeps.

## What would close it

Two complementary moves, neither of which is in any current card:

1. **`orbit refs check` verb.** Walks every `references[]`, every
   `.md`/`.yaml` body's path-shaped strings, every code comment with
   a `.orbit/...` literal, and validates each resolves on disk.
   Output: list of broken refs with origin file:line. Could be a
   column in `orbit verify` (CI-blocking) or a lean-pass column
   (advisory). Probably the latter — broken refs aren't substrate
   corruption, they're hygiene.

2. **Reference-aware migration.** `orbit-migrate` knows what it
   renamed. It could emit a sed-script (or apply directly) to
   rewrite all downstream string references in one pass. Closes the
   gap at migration time rather than leaving a tail.

## The adjacent, harder question

"Is this still on-mission?" is a stronger filter than "is this
stale?" or "does this resolve?" An artefact can be young, parse
clean, refs intact — and still no longer pull weight against the
project's lodestar. lean-pass implicitly assumes mission stability
(staleness signals correlate with off-mission). When the lodestar
shifts, the assumption breaks.

This probably isn't a separate skill — it's a *question lean-pass
asks at the per-release cadence*, not the per-session-close one.
But it deserves explicit naming in 0024's scenarios so it doesn't
quietly default to "if it's young and resolves, keep it."

## Where to file follow-ups

- **`orbit refs check` is the right verb.** → New card via
  `/orb:card`, depends-on 0020 (orbit-state) and feeds 0024
  (lean-pass).
- **Reference-aware migration.** → Scope-extension on the migration
  tool; could be a bead under the dogfood window or a 0.2 milestone.
- **Mission-orientation filter for lean.** → Amend 0024-lean-pass
  with an explicit scenario, not a new card.

## Status

Memo only. No card filed. Surfaced from a real migration with
real broken refs in hand — the example is concrete enough that
distillation can wait until a second project produces the same
shape (hydrofoil is N=1).
