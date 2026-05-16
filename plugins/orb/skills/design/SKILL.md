---
name: design
description: Focused design session — capture what good looks like for a feature card
---

# /orb:design

Focused design conversation that captures what good looks like for a feature card. The card already answers who, what, and why — this stage clarifies constraints, priorities, risk appetite, and scope boundaries. Implementation approach is the implementing agent's job, not the author's.

Agent prose follows the BLUF / Decision Brief contract — see card 0026 (`.orbit/cards/0026-executive-communication.yaml`).

@.orbit/STYLE.md

## Usage

```
/orb:design [card or topic]
```

## When to Use

- A card exists with ≥ 3 scenarios
- The *what* is clear but the *how* isn't decided yet

If no card exists or the card is thin, use `/orb:discovery` first.

## Instructions

### 1. Setup

- Find the matching card in `.orbit/cards/`. Read it — including scenarios and references.
- If no matching card exists, tell the author and suggest `/orb:discovery` or `/orb:card` instead.
- Identify the output directory: `.orbit/specs/YYYY-MM-DD-<topic-slug>/` (create if needed)

### 2. Load the Evidence Base

Before asking any questions, build a picture of where this capability stands:

**Read the card's `specs` array** — these are the specs that have already addressed this capability, listed in the order they were created. For each spec:

1. Read the `spec.yaml` — note its goal, constraints, and acceptance criteria
2. Check for `progress.md` — contains findings, results, and what was learned
3. Check for `review-pr-*.md` — its presence means the spec shipped

**Build a progress summary.** The specs array tells a story, but not always a linear one. Some specs build directly on the last; others enhance the capability from a different angle (infrastructure, data quality, tooling). Present what each spec contributed without assuming a single thread:

> "Card 0002 has 3 specs. The 2026-03-25 spec shipped the multi-branch pipeline at 155/190. The 2026-03-26 spec audited data quality but hasn't shipped. The 2026-04-09 spec is building an autoresearch loop. The card's goal is 170+ — current baseline is 155."

**Reconcile the specs array — keyword scan for orphaned specs:**

Before trusting the card's `specs` array as complete, verify it with a keyword scan (see `/orb:keyword-scan` for the full technique). This catches specs that address the card's topic but were never linked back.

1. Extract keywords from the card and search `.orbit/specs/` using the keyword scan technique
2. Compare hits against the card's `specs` array — any spec directory found by keyword but not in the array is a potential orphan
3. Surface orphans to the author — do not auto-link:
   > "Found .orbit/specs/2026-04-10-exit-classifier/ discussing [matched terms] — not in card's specs array. Include?"
4. If the author confirms, append the spec path to the card's `specs` array and write the updated card to disk

If no orphans are found, move on silently.

**Then load broader evidence:**

1. Check `.orbit/memos/` for related memos
2. If the card has `references`, read them — these may contain empirical results
3. Search the codebase for experiments, sweeps, or benchmarks related to the card's topic

**Apply the evidence hierarchy** (see `/orb:interviewer`): findings with data are constraints, not questions. Only ask about areas where evidence is silent or contradictory.

### 3. Assess the Design Space

The evidence loaded in §2 determines whether questions are warranted at all. Before composing any question, classify the card's design space:

| Mode | Signals | Path |
|------|---------|------|
| **closed** | An associated choice file under `.orbit/choices/` already pins the architectural approach (status `accepted`), AND prior specs declare or build on the pattern. The card's scenarios are operational consequences of a decided shape. | §4 — produce a design note, no interview |
| **open** | No associated choice file. Decisions unresolved. Multiple plausible shapes for the card. Prior specs (if any) explored adjacent angles without converging. | §5 — open with the card, then conduct the design session |
| **partial** | A choice exists but residual trade-offs remain — the choice is `proposed`, or only some scenarios are addressed by prior specs, or the card's references represent unresolved alternatives. | §5 — conduct the design session, scoped to the residual trade-offs |

**Detection heuristic.** A card is in *closed* mode when (a) at least one accepted choice file in `.orbit/choices/` directly addresses the card's domain (matched by topic keywords or by the choice citing the card or its prior specs) and (b) the card's `specs` array contains at least one shipped spec following that choice. Either signal alone is *partial*; absence of both is *open*.

State the classification and the path it selects to the author in one line before proceeding:

> "Card 0031: closed. Choice 0012 pins the intent-vs-means distinction; prior specs apply it. Producing a design note rather than running an interview."

The classification is a load-bearing pre-flight — don't skip it. "No interview" is a normal terminal outcome (§4), not a failure mode.

### 4. Closed Mode: Produce a Design Note

When the design space is closed, skip the interview entirely and produce a one-screen design note. The card already carries the intent; the choice already pins the shape; the agent's job is to surface the implementation handoff, not to manufacture Q&A.

**Output:** `.orbit/specs/YYYY-MM-DD-<topic-slug>/design-note.md`

```markdown
# Design Note: <Topic>

**Date:** YYYY-MM-DD
**Card:** .orbit/cards/NNNN-slug.yaml
**Mode:** closed
**Choice:** .orbit/choices/NNNN-slug.yaml — <one-line title>

---

## What good looks like

<User-voice paragraph — written from the user's experience, in the author's idiom, observing from outside the system. Drafted by the agent from the card's goal and scenarios, offered to the author for editing rather than extracted via questions. One paragraph, three to six sentences.>

## Pinned approach

<Cite the choice and any prior specs that establish the pattern. One to three bullets — what's already decided and why this card is operationally inside that decision.>

## Deferred items

- <Anything the card raises that this spec does not address>
- <Open question that belongs to a future spec, not this one>

## Implementation notes

- <Means-level leads from the codebase scan in §2 — starting context for the implementing agent>
```

The user-voice paragraph and the pinned approach are the load-bearing content. The design note is short on purpose — closed-space cards don't need re-litigation.

After writing the design note, exit cleanly:

> "Design note written. No interview needed — design space is closed. Run `/orb:spec` against this design note to crystallise the spec."

This is a normal terminal outcome. Do not prompt for further input.

### 5. Open or Partial Mode: Open with the Card and Its History

In open or partial mode, anchor the session before asking anything:

1. Summarise the card: "I've read card NNNN — *<feature name>*. Your scenarios cover: X, Y, Z."
2. Present the progress summary: where prior specs got to, what they learned, and where the goal still has gaps
3. **Anchor on the gap between current state and goal** — this is the design space. "The card's goal is X. Prior work got to Y. This session is about what closes the remaining gap."
4. If the card has `references`, surface them: "Your references include A, B, C — these represent different approaches."

**Draft the user-voice paragraph from the card** — don't extract it by Q&A. The card's goal and scenarios already carry the user's experience; the agent's job is to compress that into a paragraph the author can react to.

> "Here's what I read as 'what good looks like' from the card — written from your seat as the user. Edit if it's off, or accept it as the intent contract for this spec."
>
> *<draft paragraph>*

Save the (possibly edited) paragraph; it becomes the top-of-file slot in the interview record (§9). This pattern — the agent pays the compression cost, the author edits — is the load-bearing alternative to four reconstructive questions.

**Note:** Not every design session continues the last spec's thread. The author may want to approach the goal from a different angle — infrastructure work, data quality, tooling improvements, or adjacent capabilities that indirectly advance the goal. The design session should surface which path the author intends, not assume linear progression.

### 6. Conduct the Design Session

Adopt the interviewer role (see `/orb:interviewer` for the full persona and the decision-level gate).

**The author's job is to define what good looks like. Everything else is derivable.** Only ask intent-level questions — goals, priorities, constraints, risk appetite. Implementation questions (which function, what algorithm, how to test) are recorded as implementation notes for the implementing agent.

Target: **3–5 questions** focused on:

- **Outcome priorities** — when goals or scenarios compete, what matters more? "You want both speed and correctness — when they conflict, which wins?"
- **Risk appetite** — what's the acceptable blast radius? How much breakage is tolerable? "Is this a careful surgical fix, or an aggressive refactor?"
- **Constraints** — platform, performance, compatibility *boundaries* (not implementation targets)
- **Scope boundaries** — what's explicitly out of scope for this spec? What adjacent problems should be deferred?
- **Quality of experience** — when references or scenarios imply different user-facing approaches, probe the feel. "You referenced uv and cargo — uv is quiet, cargo is verbose. Which feel?" These are UX preferences only the author can provide.

#### The implementation-question filter

Every candidate question passes through the filter at composition time. Apply the test:

> **Would the author need codebase context, schema knowledge, metric vocabulary, or evaluation tooling to answer this?**

Each signal is a fail:

- **Codebase context** — "which function should we modify", "what's the current error path", "where does the validator live"
- **Schema knowledge** — "what fields exist on the X model", "how is Y indexed", "what's the relationship between A and B"
- **Metric vocabulary** — "what's the F1 baseline", "which evaluation captured this last time"
- **Evaluation tooling** — "how do we test this", "what fixture covers this case", "which CI step catches it"

If a candidate question hits any signal, **don't ask it.** Route it to the **Implementation Notes** section of the interview record, where the implementing agent can act on it. The filter is the gate every question passes through; it is not optional and it does not relax under time pressure.

**Multiple-choice as a smell.** "Would you prefer A, B, or C?" framing between implementation alternatives is still implementation — only the surface changed. If the agent reaches for an A/B/C question between two code paths, two libraries, two function signatures, two test layouts, treat the framing as a smell and re-classify the question. UX preferences (tone, naming, visual style) are legitimate multiple-choice; implementation alternatives are not, regardless of how the question is dressed.

**Mode-switch trigger after repeated rejection.** When the author rejects a question as implementation-shaped — explicit phrasing like "that's an implementation detail", "ask the implementing agent", "I'd need codebase context", "that's a code question" — count the rejection. After two rejections in a session, the agent treats the third candidate question as a mode-switch decision rather than a third reformulation. Re-run §3's design-space assessment; the evidence is that the design space is closed or partial, not that the question wording was wrong. Repeated implementation-shape is a signal about the card, not about the agent's prose.

#### Per-question protocol

For each question that survives the filter:

1. Present the question using **AskUserQuestion** with contextually relevant suggested answers:
   - When the card has references, use them as suggested answers where relevant
   - Priority questions: use the competing concerns as options
   - The author can always type a custom response

2. Record the Q&A pair in your working notes

3. After each answer, target the biggest remaining gap in **intent**

**Implementation notes:** As you explore the codebase and evidence base (§2), and as the filter routes failed questions, you'll accumulate implementation-level decisions — which module to modify, what patterns exist, what approach seems right. Record these under **Implementation Notes** in the interview record. They give the implementing agent a head start without burdening the author.

### 7. Ambiguity Assessment

After every 2-3 questions, assess clarity:
- **Goal Clarity**: Is the objective specific and well-defined? (card usually covers this)
- **Constraint Clarity**: Are limitations and boundaries specified?
- **Success Criteria Clarity**: Are success criteria measurable?

If all three are clear (ambiguity ≤ 0.2), suggest wrapping up. Design sessions should be tight — the card did the heavy lifting.

### 8. Surface Decisions

Design sessions are where most decisions live. When probing references and approach selection, choices will surface naturally. When a clear choice is made:

1. Note it in the record under **Decisions Surfaced**
2. Each entry should name the choice, the alternatives considered, and the rationale
3. These become MADR decision records during or after the session (the spec will reference them)

### 9. Save the Record

Save the Q&A as: `.orbit/specs/YYYY-MM-DD-<topic-slug>/interview.md`

```markdown
# Design: <Topic>

**Date:** YYYY-MM-DD
**Interviewer:** <agent name>
**Card:** .orbit/cards/NNNN-slug.yaml
**Mode:** open | partial

---

## What good looks like

<User-voice paragraph — drafted by the agent from the card, edited by the author. This is the intent contract: the implementing agent reads this for prose-level user intent, not just the structured Q&A below. One paragraph, three to six sentences, written from the user's seat in the author's idiom.>

---

## Context

Card: *<feature name>* — <scenario count> scenarios, goal: <current goal>
Prior specs: <count> — <one-line summary of where they got to>
Gap: <what remains between current state and goal>

## Q&A

### Q1: <Short label>
**Q:** <question>
**A:** <answer>

[...]

---

## Summary

### Goal
<From the card — refined if the session added nuance>

### Constraints
- <constraint 1>

### Success Criteria
- <criterion 1>

### Decisions Surfaced
- <choice made>: chose X over Y because Z (→ .orbit/choices/NNNN if recorded)

### Implementation Notes
- <means-level observations from codebase exploration — starting context for the implementing agent>
- <questions that failed the implementation-question filter — routed here rather than asked>

### Open Questions
- <anything still unclear — intent-level only>
```

The **What good looks like** paragraph is the load-bearing top-of-file slot. The implementing agent and `/orb:spec` both read it as the intent contract — it survives the structured-Q&A reduction.

---

**Next step:** `/orb:spec` to generate a structured specification from this design session — or, in closed mode, from the design note.
