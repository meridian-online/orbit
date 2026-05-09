---
name: card
description: Write a feature card — capture who needs what, why, and expected behaviours as scenarios
---

# /orb:card

Interactively write a feature card that captures a user need with expected behaviours.

## Usage

```
/orb:card [topic]
```

## What a Card Is

A card captures a **feature**: who needs it, why it matters, and what they'd expect to see. It follows a Gherkin-inspired structure — a feature description with scenarios — expressed in YAML.

Cards are NOT specs. They don't prescribe solutions. "The step name is visible immediately" doesn't say "flush stdout" — it says what the user observes. The *how* comes during the interview.

## Pre-flight: Card or Choice?

Before writing anything, classify the request. Cards and choices are different artefacts and choosing the wrong one is the most common mistake at this entry point.

- **Capability-shape** — X is a *new thing the product provides for users* (an outcome, an observable behaviour, a role × need). Continue to §1.
- **Choice-shape** — X is a *decision about how* an existing capability is implemented (bash vs rust, MCP vs shell, schema choice, review pattern, library choice). The capability already exists; only the implementation surface is changing. **Stop here.** Write a MADR choice file at `.orbit/choices/NNNN-<slug>.yaml` instead — see `.orbit/choices/0001-progressive-spec-review.yaml` for the shape.

**Worked example of the trap.** "Should `orbit spec promote` live in rust as a verb instead of bash?" is choice-shape — the promote capability already exists, the question is implementation surface. File as a choice, not a card.

If unsure, ask the user explicitly: *"Is this a new capability or a decision about how an existing one is implemented?"* before proceeding.

## Instructions

### 1. Determine the Next Card Number

Read the `.orbit/cards/` directory. Find the highest existing `NNNN-*.yaml` number and increment by 1. If no cards exist, start at `0001`.

### 2. Interview for Card Content

Use **AskUserQuestion** to gather:

1. **Feature name**: What is this feature called? (short, descriptive)
2. **Role**: Who has this need? (as_a)
3. **Desire**: What do they need? Outcome, not solution. (i_want)
4. **Benefit**: Why does it matter? (so_that)
5. **Scenarios**: What would the user expect to see? Gather 2-5 scenarios, each with:
   - **name**: Short scenario label
   - **given**: Precondition
   - **when**: Action or event
   - **then**: Observable outcome (in user language, not engineering language)
   - **gate** (optional, default false): When `true`, the scenario describes a sequencing checkpoint — the corresponding bead AC blocks all subsequent ACs by declaration order. `promote.sh` propagates this into the bead acceptance field as a `[gate]` marker. Use sparingly — only for scenarios that name a decision or prerequisite that must complete before later scenarios can begin.
6. **Goal**: What does success look like right now? (optional) — a specific, measurable target at the current maturity. The `so_that` says why the capability matters (timeless); the `goal` says what you're driving toward right now. Goals evolve as the capability matures — git history tracks the progression.
7. **Maturity**: How mature is this capability? (optional)
   - `established` — built and working
   - `emerging` — partially built, some specs have addressed it
   - `planned` — not yet built (default for new cards)
8. **References**: Are there existing tools, libraries, or approaches that inspire this feature? (optional) — these are not solutions, they're prior art that provides context. Examples: "uv: fast, minimal output", "cargo: step-by-step compile progress".

### 3. Write the Card

Save as `.orbit/cards/NNNN-<slug>.yaml`:

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
    # gate: true   # optional — propagates to bead AC as [gate] via promote.sh

  - name: "<scenario name>"
    given: "<precondition>"
    when: "<action or event>"
    then: "<observable outcome>"

goal: "<current measurable target>"   # optional — what success looks like right now

maturity: "planned"                  # planned | emerging | established

specs: []                            # specs that have addressed this capability

references:                          # optional — prior art and inspiration
  - "<tool/approach>: <what's relevant about it>"
```

### 4. Check for Overlap

Before finalising, run a keyword scan (see `/orb:keyword-scan`) against `.orbit/cards/` and `.orbit/specs/` using terms from the feature name and scenarios. If an existing card already describes this capability, surface it — the author may want to update the existing card rather than create a new one.

### 5. Quality Check

Verify the card against INVEST criteria:
- **Independent**: Can be delivered without other cards
- **Negotiable**: Scenarios describe outcomes, not solutions
- **Valuable**: Clear benefit to the user
- **Estimable**: Enough detail to estimate effort
- **Small**: 2-5 scenarios (if more, suggest splitting)
- **Testable**: Each scenario has an observable outcome

### Cards Are Living Documents

Cards describe capabilities, not work items. When a capability evolves, update the card in place — git history is the audit trail. Cards are never "closed" or "delivered"; they are the current description of what the product does for its users.

The relationship between cards and work is mediated by specs: a spec references a card and prescribes how to implement or extend the capability. Multiple specs may reference the same card over time as the capability matures.

### What Gets Closed

**Specs** get completed — `progress.md` marks all ACs done, `/orb:review-pr` verifies the work, and the PR merges. That's the closure unit. The card persists because the capability persists.

When a spec produces a NO-GO or invalidates an assumption:
1. Record the finding in `progress.md` with evidence
2. Update the card's `goal` to reflect what was learned
3. Advance or maintain the card's `maturity` based on current state

The card lives on. Its goal may change, its maturity may shift, but it still describes what the product does — which now includes what you learned.

---

**Next step:** Refine this card with `/orb:design` to work out the technical approach.
