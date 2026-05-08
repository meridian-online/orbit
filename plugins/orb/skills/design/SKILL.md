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

1. Check `.orbit/cards/memos/` for related memos
2. If the card has `references`, read them — these may contain empirical results
3. Search the codebase for experiments, sweeps, or benchmarks related to the card's topic

**Apply the evidence hierarchy** (see `/orb:interviewer`): findings with data are constraints, not questions. Only ask about areas where evidence is silent or contradictory.

### 3. Open with the Card and Its History

Don't re-ask what the author wants. They wrote a card. Instead:

1. Summarise the card: "I've read card NNNN — *<feature name>*. Your scenarios cover: X, Y, Z."
2. Present the progress summary: where prior specs got to, what they learned, and where the goal still has gaps
3. **Anchor on the gap between current state and goal** — this is the design space. "The card's goal is X. Prior work got to Y. This session is about what closes the remaining gap."
4. If the card has `references`, surface them: "Your references include A, B, C — these represent different approaches."

**Note:** Not every design session continues the last spec's thread. The author may want to approach the goal from a different angle — infrastructure work, data quality, tooling improvements, or adjacent capabilities that indirectly advance the goal. The design session should surface which path the author intends, not assume linear progression.

### 4. Conduct the Design Session

Adopt the interviewer role (see `/orb:interviewer` for the full persona and the decision-level gate).

**The author's job is to define what good looks like. Everything else is derivable.** Only ask intent-level questions — goals, priorities, constraints, risk appetite. Implementation questions (which function, what algorithm, how to test) are recorded as implementation notes for the implementing agent.

Target: **3–5 questions** focused on:

- **Outcome priorities** — when goals or scenarios compete, what matters more? "You want both speed and correctness — when they conflict, which wins?"
- **Risk appetite** — what's the acceptable blast radius? How much breakage is tolerable? "Is this a careful surgical fix, or an aggressive refactor?"
- **Constraints** — platform, performance, compatibility *boundaries* (not implementation targets)
- **Scope boundaries** — what's explicitly out of scope for this spec? What adjacent problems should be deferred?
- **Quality of experience** — when references or scenarios imply different user-facing approaches, probe the feel. "You referenced uv and cargo — uv is quiet, cargo is verbose. Which feel?" These are UX preferences only the author can provide.

**For each question:**

1. Apply the decision-level gate: "Would the author need codebase context to answer this?" If yes, record as an implementation note — don't ask.

2. Present the question using **AskUserQuestion** with contextually relevant suggested answers:
   - When the card has references, use them as suggested answers where relevant
   - Priority questions: use the competing concerns as options
   - The author can always type a custom response

3. Record the Q&A pair in your working notes

4. After each answer, target the biggest remaining gap in **intent**

**Implementation notes:** As you explore the codebase and evidence base (§2), you'll identify implementation-level decisions — which module to modify, what patterns exist, what approach seems right. Record these under **Implementation Notes** in the interview record. They give the implementing agent a head start without burdening the author.

### 5. Ambiguity Assessment

After every 2-3 questions, assess clarity:
- **Goal Clarity**: Is the objective specific and well-defined? (card usually covers this)
- **Constraint Clarity**: Are limitations and boundaries specified?
- **Success Criteria Clarity**: Are success criteria measurable?

If all three are clear (ambiguity ≤ 0.2), suggest wrapping up. Design sessions should be tight — the card did the heavy lifting.

### 6. Surface Decisions

Design sessions are where most decisions live. When probing references and approach selection, choices will surface naturally. When a clear choice is made:

1. Note it in the record under **Decisions Surfaced**
2. Each entry should name the choice, the alternatives considered, and the rationale
3. These become MADR decision records during or after the session (the spec will reference them)

### 7. Save the Record

Save the Q&A as: `.orbit/specs/YYYY-MM-DD-<topic-slug>/interview.md`

```markdown
# Design: <Topic>

**Date:** YYYY-MM-DD
**Interviewer:** <agent name>
**Card:** .orbit/cards/NNNN-slug.yaml

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

### Open Questions
- <anything still unclear — intent-level only>
```

---

**Next step:** `/orb:spec` to generate a structured specification from this design session.
