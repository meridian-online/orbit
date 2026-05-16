# Pilot rollout check-in placeholder

**Date:** 2026-05-01
**Source:** session-close after orbit 0.4.0 release

## Purpose

Hold space for findings from the bead-native pilot rollout to downstream projects. Hugh expects to surface feedback within a 2-week window from his own use, so no scheduled agent is needed — this memo is the destination for whatever surfaces.

## Rollout shape

Per the `next-session-pilot-rollout` bd memory and the operator migration playbook (held in the private ops repo):

- **Arcform first** (smallest blast radius)
- After Arcform validates: the remaining downstream projects in parallel

The migration steps are documented in the operator playbook. Card 0017 (`/orb:setup` is bead-aware) folds those steps into a single `/orb:setup` invocation; until 0017 ships, the manual playbook is the canonical path.

## What to look for during the pilots

- **bd not on PATH under cron / non-interactive contexts.** Most likely macOS failure mode. Test interactively first, then validate the autonomous path with `/orb:drive ... full`.
- **Cycle-history `[x]` leak.** Documented in MADR 0013 consequences as bounded by drive ordering (review-spec runs before implement). If it surfaces during a pilot, that's evidence the bound was wrong.
- **Gate semantics on hand-promoted beads.** If a project hand-edits acceptance fields rather than going through `promote.sh`, gate markers won't propagate from card scenarios. Always promote via the script if gate semantics matter.
- **Substrate signals in the smoke-test drive** (per migration playbook step 5):
  - (a) Promote prints a bead-id
  - (b) `bd ready --type task` lists it
  - (c) review-spec brief mentions `bd show` and `parse-acceptance.sh`, NOT a snapshot path
  - (d) Verdict file lands at `orbit/reviews/<bead-id>/...`
  - (e) `parse-acceptance.sh acs` returns the AC list with `is_gate` populated
  - (f) `/orb:implement` reads ACs from the bead, no `progress.md` written

## Where to file findings

- **Bug or regression in the substrate** → memo here in `.orbit/memos/`, then file a bead via `bd create -t task` and queue for the next drive.
- **Operational friction** (e.g. `bd-init.sh` UX, plugin reload required somewhere unexpected) → fold into card 0017's scope when it gets driven.
- **Capability gap** (e.g. "I needed orbit to do X and it can't") → new card via `/orb:card`.
- **Memo-shaped finding without a clear destination yet** → memo here.

## Status

Awaiting Hugh's pilot findings within the 2-week window from 2026-05-01.

If 2 weeks pass without findings, that itself is signal worth noting (pilot didn't happen vs pilot ran clean — both information).
