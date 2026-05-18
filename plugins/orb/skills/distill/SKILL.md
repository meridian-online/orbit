---
name: distill
description: Extract capability cards from source material — files, directories, or a whole project
---

# /orb:distill

Extract structured feature cards from source material. Takes a memo, a document, a directory, or a whole project and identifies the capabilities it describes, presenting them as a batch for review before anything is written to disk.

## Usage

```
/orb:distill <scope>
```

Where `<scope>` is one of:
- **A file path** — `.orbit/memos/2026-04-07-progress.md` or `.orbit/specs/topic/interview.md`
- **A directory path** — `docs/` or `.` (the whole project)
- **A natural-language description** — `"the readme, docs, git history and specs"`

## Why This Exists

Ideas arrive as freeform text. Turning them into actionable cards currently requires a full `/orb:card` interview per feature. Distill bridges the gap — it reads what you've already written and extracts cards from it, so existing work product becomes actionable without re-interviewing.

## Instructions

### 1. Resolve the Scope

Interpret the author's `$ARGUMENTS` to determine what to read:

- **File path**: Read the file. If it doesn't exist or is unreadable, report a clear error and stop.
- **Directory path**: Read key artifacts in the directory — README, docs, source structure, existing cards, specs, tests. Use Glob and Read to survey broadly; don't stop at one file.
- **Natural-language description**: The author is telling you which artifacts to examine (e.g. "the readme, docs, git commit history and specs"). Resolve this into concrete files and read them.
- **No argument**: Tell the author distill requires a scope.

When scope spans multiple artifacts, build a working set of all the material before extracting. The extraction step operates on the aggregate, not file-by-file.

## The Staging Contract

Distill works in three phases: **Draft → Review → Write.** State this upfront when presenting results so the author knows the process:

> "I've drafted N cards. Nothing is written to disk until you approve the final set. Let's review them together first."

**Nothing touches disk until the author explicitly approves the final set.** This is the core contract. Cards exist only in the conversation until the write phase.

---

### 2. Draft — Extract All Cards

Before drafting, run **`orbit overview`** to see the project's existing shape (cards-by-maturity, most-connected card, orphans) and **`orbit card tree <id>`** on any card the source material seems adjacent to — this surfaces capabilities the new draft might overlap with, before the draft is written. The substrate is the canonical inventory; distilling against it avoids duplicates.

Analyse the source material and identify distinct features. A "feature" is a **capability the product provides** — something a user can do or observe.

**The first-principles lens:**

Always ask "what does this product do?" — not "what's planned next?" You are describing capabilities, not mining for TODOs. Even when the source material contains roadmap items, TODO comments, or planned enhancements, distill through the lens of **what the user gets**, not what the developer has left to build.

For example:
- ❌ "Expand phone validation to 40+ locales" (incremental TODO)
- ✅ "Locale-aware type detection" (capability the user experiences)

**Per-candidate classification — capability or choice?**

For each candidate distillation, ask: is this a *new capability* the product provides, or a *choice* about how an existing capability is implemented? If the latter (e.g. "should X live in bash or rust", "schema choice for Y", "review pattern Z", library or implementation-surface decisions), it belongs as a MADR choice file at `.orbit/choices/NNNN-<slug>.yaml`, not a card. The capability is unchanged; only the implementation surface is being decided. Worked example: a memo arguing for `orbit spec promote` to live in rust is choice-shape, not card-shape.

Surface choice-shape distillations in the Review phase the same way as cards, but flag them as `choice` (not `card`) in the numbered list so the author can confirm routing before the Write phase. See `.orbit/choices/0001-progressive-spec-review.yaml` for choice file shape.

**Rules:**
- Each feature must be **distinct** — different user need, different outcomes
- If the source contains only one feature, that's fine — produce one card
- If the source contains **no identifiable feature ideas** (e.g. a grocery list, meeting notes with no actionable features): report "No features found — nothing to distill." and stop. Do **not** hallucinate cards from non-feature content.

**Check for overlap with existing cards:** Before drafting, run a keyword scan (see `/orb:keyword-scan`) against `.orbit/cards/` using terms from the source material. If existing cards already describe a capability you're about to draft, note the overlap — it may mean updating an existing card rather than creating a new one. Surface overlaps during the Review phase.

Draft ALL cards before presenting any of them. Each card uses the standard YAML format:

```yaml
feature: "<short feature name>"
as_a: "<role>"
i_want: "<desired outcome>"
so_that: "<reason/benefit>"

scenarios:
  - name: "<scenario name>"
    given: "<precondition>"
    when: "<action or event>"
    then: "<observable outcome>"
    source_lines: "<quoted passage from source>"

goal: "<current measurable target>"   # optional — what success looks like right now

maturity: "planned"                  # planned | emerging | established

specs: []                            # specs that have addressed this capability

references:
  - "<source artifact path(s)>"
```

**Critical rules for card content:**

- **Extract, don't invent.** Every scenario MUST trace to something in the source material. The `source_lines` field quotes the originating passage. If you can't point to a passage that supports a scenario, don't include that scenario.
- **`source_lines` is mandatory** on every scenario. It must quote text that exists verbatim (or near-verbatim) in a source artifact. When scope spans multiple files, prefix with the file path: `"README.md: Detects 120+ semantic types"`. This is the mechanically verifiable link between the card and its source.
- **`references` always includes the source artifacts.** Every card produced by distill includes the input scope in its references list. For single-file scope, this is the file path. For broader scope, list the key artifacts the card was extracted from.
- **Scenarios describe outcomes, not solutions.** Follow the same principle as `/orb:card` — what the user observes, not how it's built.
- **Describe capabilities, not changes.** Scenarios should express what the product does for users, not what developers need to build. Frame around the user's experience of the capability.

### 3. Review — Present the Full Set

Present all drafted cards as a numbered batch. The author sees the complete taxonomy before committing to anything.

**Presentation format:**

```
Drafted N card(s) from <scope description>. Nothing is written to disk yet.

1. <feature name> — <one-line summary>
2. <feature name> — <one-line summary>
...
```

Then show the full YAML for each card, numbered to match.

**After presenting the batch, surface observations:**

- **Overlaps** — cards that may describe the same capability from different angles. "Cards 3 and 7 both touch locale handling — should they merge?"
- **Gaps** — capabilities evident in the source material that no card covers. "The source mentions X but no card captures it."
- **Low confidence** — cards where the source evidence is thin or the capability boundary is unclear. "Card 5 is based on a single TODO comment — the scope may be wrong."
- **Inconsistencies** — cards that contradict each other or use conflicting terminology.

Use **AskUserQuestion** to invite feedback:

> "Review the set above. You can ask me to merge, split, drop, rename, or edit any cards. When the set looks right, say **'write'** and I'll save them all."

### 4. Revise — Incorporate Feedback

The author's feedback applies to the batch as a whole. Common operations:

- **"Merge 3 and 7"** — combine two cards into one, reconciling scenarios
- **"Drop 5"** — remove a card from the set
- **"Split 2"** — break one card into two distinct capabilities
- **"Rename 4 to X"** — change a card's feature name
- **Free-text edits** — "Card 1 should focus on X, not Y" or "Add a scenario about Z to card 3"

After applying changes, re-present the updated set with the same numbered format. Continue until the author says **"write"** or equivalent.

**Edits and `source_lines`:** If the author requests adding a new scenario that has no corresponding passage in the source material, set `source_lines` to `"author-directed during review"`. The extract-not-invent rule applies to the *initial* extraction — author-directed edits are explicitly authored, not LLM-invented.

### 5. Write — Save Approved Cards

When the author approves the final set:

1. Read the `.orbit/cards/` directory to determine the next available `NNNN` number. If `.orbit/cards/` does not exist, create it and start at `0001`.
2. For each card in the approved set:
   - Generate a slug from the feature name (lowercase, hyphens, no special characters)
   - Save as `.orbit/cards/NNNN-<slug>.yaml`
   - Increment the number for the next card
3. Confirm what was written:

```
Distill complete:
  Scope: <scope description>
  Written: N card(s)
    - .orbit/cards/NNNN-<slug>.yaml
    - .orbit/cards/NNNN-<slug>.yaml
    ...
  Dropped: M card(s) during review
```

4. **Clean up source memos.** If the scope included files from `.orbit/memos/`, delete each consumed memo:
   ```bash
   git rm .orbit/memos/<memo-file>
   ```
   Only delete memos that produced at least one card in the approved set. The cards' `references` field preserves provenance; git history preserves the original content.

Card numbering is determined at write time. This is a single-user workflow — concurrent numbering is a known limitation, not a bug to solve.

5. **Consider topology update (conditional).** If `.orbit/topology/` exists and is populated (the canonical predicate per choice 0025 — substrate-folder shape), ask: did any of the newly-written cards correspond to a *subsystem-level capability* (a multi-file area with its own data shape, wiring, and operational surface) — rather than a prose, UX, or single-file change? If yes, invoke `/orb:topology` write-mode against the relevant subsystem name so the topology substrate accretes alongside the new card. This is quality-gated — only fire when the distillation genuinely describes a subsystem, not on every distill.

If any cards were written, suggest next step: `/orb:design` to refine a card into a spec.

## Integration with Other Skills

- **`/orb:card`** — distill produces the same YAML format, so distilled cards are interchangeable with interview-created cards
- **`.orbit/memos/`** — the primary input source; consumed memos are deleted after card extraction (§5 step 4). Git history preserves the original content
- **`/orb:design`** — the natural next step after distilling a card
- **`/orb:discovery`** — interview.md files from discovery sessions are valid distill inputs

---

**Next step:** Run `/orb:design` on an approved card to work out the technical approach.
