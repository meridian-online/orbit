# Memo — documentation topology index

**Date:** 2026-05-18
**Context:** Surfaces a substrate gap relevant to orbit's agent-side discipline cluster.

## The pattern

A multi-file subsystem's documentation lives in five-plus places: module docstrings on the authoritative code, MADRs (or whatever the project's architectural-decision-record format is), ERD / data-shape docs, demonstrator scripts that double as documentation, and DI / wiring files. A topical question — *how does subsystem X work?* — doesn't map to a single filename. File-keyed indexes (a `docs/project-structure.md` listing every file with a one-liner) name the files but don't surface them by topic.

Agents under time pressure extrapolate from one source rather than grepping for the others. The substrate has the answer; the index doesn't surface it. Sessions burn time rediscovering what already exists.

## The proposed shape

A single `docs/topology.md` (or equivalent location) keyed by **subsystem**, not by file. For each subsystem, five lines:

- authoritative code file
- owning architectural decision (MADR or equivalent)
- operational doc
- test surface
- one-sentence *what this gives you*

Pointer-only — references canonical sources, doesn't duplicate. When the authoritative file's docstring updates, the index stays correct because it carries no content of its own. Dense, single-page, designed to be the first thing read when investigating any architectural question.

## Anchored to pillar #2 (agent self-learning)

The topology index is a self-learning surface, not just a navigation aid. It compounds across three downstream effects:

- **Spec verbosity drops.** Specs that currently rehearse subsystem context — *here's how warmup works*, *here's the data shape* — can cite the topology entry instead. The intent contract stays sharp; the architectural backdrop lives in one place.
- **Skills become better targeted.** A skill investigating subsystem X knows exactly which canonical files to load (authoritative code, MADR, operational doc, test surface). The skill prose can route to the right starting points instead of coaching every agent to re-grep the tree.
- **Code mastery becomes operable at the architecture level.** `/orb:code-investigate` makes the *file-level* investigation discipline cheap; the topology index makes the *architecture-level* investigation discipline cheap. Together, the agent owns the code at both granularities.

Maintenance feeds the loop. Each release that lands a new subsystem updates its topology entry; each spec that touches a subsystem can update the index in passing. The index accretes as the codebase does — substrate that compounds rather than rots.

## Behavioural complement

The doc alone doesn't close the gap — agents have to *reach for it*. A CLAUDE.md "Posture" line is the natural pair: *"Before reasoning about how a subsystem works, grep the code tree and `docs/` for it. Substrate beats extrapolation."*

Same shape as the code-investigate cluster — make the discipline cheap, then make the reach for it the default. Doc-topology is the architecture-level analogue of `/orb:code-investigate`'s file-level discipline.

## Cluster fit

Sibling to the agent-side substrate-engagement cluster shipped this week (cards 0037, 0038, today's `/orb:code-investigate`) plus today's autonomy-too-ready-to-halt memo. All address agent failure modes around substrate use:

- 0037 — memory-gates-decisions (memories ignored at decision moments)
- 0038 — skills-infer-or-prompt-before-halt (skills halt instead of inferring)
- `/orb:code-investigate` (agents skip codebase investigation)
- doc-topology (agents skip architectural-doc investigation)
- autonomy-too-ready-to-halt (agents halt when contract says proceed)

Five concrete instances now. Worth watching whether a synthesis card surfaces; not opening one preemptively.

## Open questions for distill

- Orbit-provided convention (template doc shipped with `/orb:setup`), or consumer-repo decision documented in METHOD.md as a guideline?
- Location — `docs/topology.md`, `.orbit/topology.md`, or repo root?
- Scope — does orbit ship a skill (`/orb:topology`) that scaffolds or maintains this, or is it a pure documentation convention?
- Maintenance — how does the topology index stay in sync with the substrate it indexes? Build-time check? Manual review at release boundaries?
- Relationship to existing orbit verbs (`orbit overview`, `orbit card tree`, `orbit graph`) — those already surface card/spec topology but not code/architecture topology. The new index would be the code-side counterpart.

## Carry-forward

Pure idea memo. Distill into a card if the view is that this is a capability orbit should provide. Leave as memo if it reads better as a consumer-repo convention not formalised in the plugin.
