# /prioritise skill — session-start priority synthesis

Open a new session, type `/prioritise`, get back a ranked list of N things to do with cost + one-line rationale per item. That's the workflow I keep expecting and there's no skill there yet.

What it should do, mechanically:

- Read `orbit audit conformance --json` (findings + remediation.verb per entry — the structural backlog signal).
- Read `orbit session prime` (open specs, recent memories, prior handover distillate).
- Read recent feedback / calendar memories for any deferred deadlines or unresolved decisions.
- Synthesise the lot into a **Decision Brief** — top 5-7 priorities, each one line of *what* + *why* + *est. effort* + *next-action verb*.

Read-only output. I pick a priority; I run the remediation verb. The skill doesn't auto-execute.

Why I want this:

- At session close I ask the agent to write priorities for next time. That writes prose into the handover. But the handover is frozen at the moment it was written — stale memos accumulate, cards age, calendar entries fire. By the time the next session opens, the synthesis is hours-to-days old.
- `/prioritise` re-derives the same kind of synthesis live, from the same substrate inputs the close-time write used.
- This is the executive-communication layer (pillar 1) over the conformance verb + session-prime envelope. The data is there; the agent-side compression to a ranked plan is what's missing.

Adjacent skills to differentiate from:

- `/orb:overview` — exists, but it's a status-snapshot (cards-by-maturity, orphans, etc.), not a ranked action plan.
- A hypothetical `/orb:next` — picks one finding and runs the remediation. `/prioritise` is the menu; `/orb:next` would be the auto-pick. The menu has higher value first.
- A hypothetical `/orb:triage` — full autonomous backlog driver. Bigger scope. `/prioritise` is the gentler version.

This memo also feeds the substrate-engagement cluster synthesis (which is 3x deferred now and at 6+ instances). The cluster pattern is "agents engage substrate signals to drive next action"; `/prioritise` is the most explicit instantiation of that pattern.
