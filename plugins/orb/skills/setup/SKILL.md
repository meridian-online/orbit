---
name: setup
description: Set up a project for the orbit workflow — creates orbit/ directory with artefact subdirs on greenfield, or interactively migrates bare-layout repos to the orbit/ folder on brownfield
---

# /orb:setup

Set up a project for the orbit specification-driven workflow.

Workflow artefacts live under a single top-level `orbit/` folder — `.orbit/cards/`, `.orbit/specs/`, `.orbit/choices/`, and (when created ad-hoc) `.orbit/discovery/`. This keeps workflow state separated from source code and standard repo metadata.

## Usage

```
/orb:setup
```

## Instructions

### 1. Detect the Repo State

Before creating or moving anything, classify the repo into one of four mutually exclusive states by inspecting the working tree at the project root:

| State | Condition | Action |
|-------|-----------|--------|
| **greenfield** | `orbit/` absent AND none of bare `cards/`, `specs/`, `decisions/`, `discovery/` present | Create `orbit/` fresh → §2 |
| **idempotent** | `orbit/` present AND none of bare `cards/`, `specs/`, `decisions/`, `discovery/` present | No-op → §5 |
| **brownfield** | `orbit/` absent AND any of bare `cards/`, `specs/`, `decisions/`, `discovery/` present | Prompt → migrate or abort → §3 |
| **mixed** | `orbit/` present AND any of bare `cards/`, `specs/`, `decisions/`, `discovery/` also present | Refuse → §4 |

These four states cover the 2×2 of (orbit/ present?) × (any bare artefact dir present?). There is no other state.

### 2. Greenfield: Create Fresh `orbit/`

Create the following directories (skip any that already exist within `orbit/`):

```
orbit/
  cards/      # Feature cards — who needs what and why
  specs/      # Specifications, interviews, reviews, progress
  decisions/  # MADR decision records
```

Do **not** create `.orbit/discovery/` at setup time. It is created ad-hoc the first time `/orb:discovery` runs. Setup detects it during brownfield migration but never creates it eagerly.

Then proceed to §6 (CLAUDE.md snippet) and §7 (first card tutorial).

### 3. Brownfield: Interactive All-or-Nothing Migration

The repo has one or more bare artefact directories at the root from a pre-`orbit/` version of orb. Migrate them under `orbit/` in a single atomic transaction.

**3a. Enumerate detected bare dirs.** Collect the subset of `{cards, specs, decisions, discovery}` that exist as directories at the repo root.

**3b. Scan for untracked residue.** Run `git status --porcelain -- <detected-bare-dirs>` and collect any untracked paths inside them. Untracked files will be left behind by `git mv` — they need to be reported to the author so they know about the residue.

**3c. Present a single all-or-nothing prompt.** Example:

```
orbit: detected legacy layout. Ready to migrate:
  cards/       → .orbit/cards/
  specs/       → .orbit/specs/
  decisions/   → .orbit/choices/
  discovery/   → .orbit/discovery/

Untracked files inside these dirs (will remain at the old path after git mv):
  cards/scratch.md

Migrate now? (y/N)
```

If no untracked files are present, omit that section. One prompt covers all detected dirs — no per-directory confirmation. A single "y" migrates everything in one transaction; anything else aborts with no filesystem changes.

**Dirty-tree handling is deliberate: setup does NOT refuse on a dirty working tree.** `git mv` preserves tracked-but-modified files' modifications, so there is no correctness risk. The author may have reasons for mid-work migrations; respect that. If `git status --porcelain` reports uncommitted changes outside the migration scope, proceed regardless.

**3d. On confirm — run `git mv` in one transaction.**

```bash
mkdir -p orbit
git mv cards .orbit/cards
git mv specs .orbit/specs
git mv decisions .orbit/choices
git mv discovery .orbit/discovery
```

Run only the `git mv` lines for directories that were actually detected in 3a. If any `git mv` fails (e.g. a target already exists from a half-completed prior migration), abort and surface the error. This state should have been caught as "mixed" in §1, but defence-in-depth applies.

**3e. On decline — abort cleanly.** Do nothing. Assert no filesystem changes occurred (`git status --porcelain` compares equal to pre-invocation). Tell the author how to re-run setup when ready.

**3f. After migration — report residue.** If any untracked files were detected in 3b, surface them explicitly in the completion message:

```
orbit: migration complete.
  Moved: 4 directories under orbit/
  Untracked residue: cards/scratch.md (file remains at old path)
    Consider: git add .orbit/cards/ or move manually
```

When no residue exists, the completion message is quiet about it.

Then proceed to §6 (CLAUDE.md snippet) and §7 (first card tutorial).

### 4. Mixed State: Refuse With Clear Error

If both `orbit/` and any bare artefact dir exist, the repo is in a transitional state setup cannot safely resolve automatically. Do not attempt silent reconciliation — the all-or-nothing migration model depends on clean pre- and post-states.

Refuse with a message naming each collision:

```
orbit: cannot migrate — inconsistent layout detected.
  .orbit/cards/ exists AND bare cards/ also exists at root
  .orbit/specs/ exists AND bare specs/ also exists at root

Resolve manually before re-running /orb:setup. Typical causes: an aborted prior migration,
a manually-created orbit/ directory, or a partial downstream pull.
```

No filesystem changes. Exit with a non-zero status so the author sees it as a refusal, not a completion.

### 5. Idempotent State: No-Op on Filesystem

The orbit/ layout is already in place. The filesystem needs no changes:

- Do not recreate `orbit/` or any subdir
- Do not run the first-card tutorial unless the author explicitly asks

**Still run §6's CLAUDE.md check** — an author on a newer plugin version may have an older snippet that lacks the vocabulary glossary. §6 detects this and offers a targeted migration. If no migration is needed (or the author declines), tell the author setup is already complete and offer `/orb:card`.

### 6. METHOD.md: Copy and Import

The canonical orbit method overview lives at `plugins/orb/skills/setup/METHOD.md` (in the plugin source). Setup copies it into the project and ensures CLAUDE.md @-imports it. METHOD.md is the single source of truth for vocabulary, pipeline, substrate rules, four pillars, and the BLUF prose contract — never inline that content into CLAUDE.md.

The operations in 6a/6b/6c below are also implemented as a single shell script for one-step execution and testing:

```
plugins/orb/scripts/setup-method.sh --project-root <project>
```

The script performs the same steps in the same order with the same atomic semantics. Use it directly for non-interactive runs (the script supports `--answer-legacy y|n` and `--answer-drift y|n` for scripted contexts).

Run the steps below **in order**. Legacy detection runs before any file is written so a refused migration leaves no orphan METHOD.md.

**6a. Legacy-CLAUDE.md detection (atomic).** Scan `CLAUDE.md` for any of these legacy markers anywhere in the file:

- `## Workflow (orbit)`
- `## Orbit vocabulary`
- `## Current Sprint`

If any are present, prompt:

```
orbit: CLAUDE.md contains legacy workflow blocks (## Workflow (orbit) / ## Orbit vocabulary / ## Current Sprint).
Migration removes them and adds @.orbit/METHOD.md as the single source of truth.
Migrate now? (y/N)
```

- **`y`:** continue to 6b. The legacy blocks will be removed in 6c's transaction.
- **anything else (atomic refuse):** REFUSE the entire setup operation. Do NOT copy METHOD.md, do NOT add any @-import. Print:
  ```
  orbit: setup aborted. Re-run /orb:setup once you have removed the legacy blocks, or accept the migration prompt.
  ```
  Exit non-zero. The author retains full control of the legacy content.

If no legacy markers are present, fall through to 6b.

**6b. Copy METHOD.md.** Copy `plugins/orb/skills/setup/METHOD.md` (from the plugin) to `.orbit/METHOD.md` in the project.

If `.orbit/METHOD.md` already exists, compare it to the canonical via **byte-for-byte equality of the entire file including the "How to update" line** (no whitespace tolerance, no hash-only comparison). If they match, no-op. If they differ, prompt:

```
orbit: .orbit/METHOD.md differs from the canonical (the plugin has updated, or the file has been edited locally).
Overwrite with canonical? (y/N)
```

- **`y`:** overwrite.
- **anything else:** keep the local copy (the prompt is informational; setup does not refuse on decline).

**6c. Ensure CLAUDE.md @-import.** If CLAUDE.md does not contain the line `@.orbit/METHOD.md` anywhere, append it on its own line at end-of-file with a single blank line above. Match the existing `@.orbit/STYLE.md` shape — **no marker heading**.

If 6a fired the migration prompt and the author accepted, this step also removes the legacy blocks (`## Workflow (orbit)`, `## Orbit vocabulary`, `## Current Sprint` and their bodies) in the same transaction as adding the @-import. Either both edits land or neither does.

If `@.orbit/METHOD.md` is already present, no-op (idempotent).

### 7. First Card Tutorial

Walk the author through writing their first feature card using `/orb:card`. Explain:

- A card captures **who** needs something, **why** it matters, and **what they'd expect to see**
- Scenarios are written in user language, not engineering language
- Cards are the intake layer — they survive context loss and ground future interviews

Then invoke `/orb:card` to interactively write the first card.

## Idempotency

This skill is idempotent. Running it again on an already-initialised project:

- Does NOT recreate existing directories
- Does NOT duplicate the CLAUDE.md snippet
- On brownfield-then-idempotent (migrate, then re-run immediately): the second run enters §5 (idempotent no-op), not §3 (brownfield) — because there are no bare dirs left to detect

## Why `orbit/`?

One folder name, one convention, not configurable. See spec `.orbit/specs/2026-04-20-orbit-artefact-folder/spec.yaml` (constraint #4) and card `.orbit/cards/0008-consolidated-orbit-artefact-folder.yaml` for the decision record.

---

**Next step:** Write feature cards with `/orb:card`, then refine them with `/orb:design`.
