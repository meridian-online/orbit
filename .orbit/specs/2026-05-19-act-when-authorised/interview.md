---
date: 2026-05-20
interviewer: Claude Opus 4.7 (rally lead)
card: .orbit/cards/0042-act-when-authorised.yaml
rally: 2026-05-19-agent-side-substrate-engagement-rally
mode: rally-design (decision-pack distillation)
---

# Design: Act when authorised

## Context

Card 0042 was carried into the `agent-side-substrate-engagement` rally as the inverse-failure-mode sibling of 0037 (consult memory at decision-time) and 0038 (consult bound state at skill-entry). Where 0037 and 0038 address "consult substrate before acting," 0042 addresses "act on substrate authorisation, don't halt." The line between them is itself a load-bearing design question (decision D6).

The decision pack at `.orbit/specs/2026-05-19-act-when-authorised/decisions.md` framed six decisions. The consolidated decision gate (Stage 3 of the rally) approved all six recommendations with one pre-flight: **D1's PreToolUse hook on AskUserQuestion is contingent on the hook surface existing in Claude Code; if it does not, fall back to the CLI verb (D1 option 3).**

## Decisions (approved)

### D1 — Where the three-question test lives

**Decision:** PreToolUse hook on `AskUserQuestion` + prose pointer in `drive/SKILL.md`. **Pre-flight: verify Claude Code supports `PreToolUse` hooks for `AskUserQuestion` before implementation begins. If not, fall back to CLI verb `orbit autonomy authorised?` (D1 option 3) with mandatory prose at every AskUserQuestion site.**

**Mechanism:** New hook at `plugins/orb/hooks/three-question-test.sh`, registered in `plugins/orb/.claude-plugin/plugin.json`. Fires when the calling agent is inside drive/rally autonomy. Prints the three questions to stderr; if `ORBIT_NONINTERACTIVE=1`, exits non-zero to suppress the halt and force the agent to either act or escalate via the structural NO-GO path.

**Rationale:** The memo's own analysis named the structural fix as highest-leverage; 0037's "Skill-prompt-only enforcement is insufficient" scenario provides cluster-coherent evidence that prose alone fails for this class of bug. A hook fires regardless of agent attention. Prose pointer alongside ensures a human reading the skill understands the rule the hook is enforcing.

**Confidence:** Medium — assumes hook surface exists. Pre-flight verification mandatory before commit.

### D2 — Three-question wording and authorisation source-of-truth

**Decision:** Substrate-typed phrasing — each question names its concrete substrate source.

**Phrasing:**
> 1. Do I have a **recommendation**? (a single concrete action I'm prepared to take)
> 2. Do I have **evidence**? (a memory key, an AC text, a prior decision file, or substrate I can cite)
> 3. Does the **contract** authorise me? (`drive.yaml.autonomy`, memory `mid-session-autonomy-contract-default-to-action-halt`, or the spec's `halt-conditions` for the current stage)

**Rationale:** The card's ac-05 names "memory + contract" as load-bearing authorisation; substrate-typed phrasing makes that explicit and audit-checkable. The named substrate gives the hook (D1) concrete things to check rather than asking the agent to self-report. Positive three-yes gate (memo voice); each question names its source.

### D3 — Severity-as-reviewer-language teaching

**Decision:** Both skill prose AND hook reinforcement.

**Skill prose:** `drive/SKILL.md` §1.6 gains a one-paragraph clarification:
> Severity (LOW / MEDIUM / HIGH) is reviewer-language. Under guided or full autonomy, severity does not change the routing — REQUEST_CHANGES is absorbed by the cycle budget regardless of severity. Severity informs priority of fixes within a cycle, not whether to surface to the operator.

**Hook reinforcement:** when D1's hook fires mid-cycle under guided/full autonomy with a non-APPROVE verdict, the message includes the severity-as-reviewer-language reminder.

**Rationale:** Cost of duplication is trivial; coherence is high. Skill prose explains; hook enforces. Two locations for one rule are manageable because the rule is unlikely to change.

### D4 — Pre-commit halt scope (stage vs surface)

**Decision:** Prose-only with hook reinforcement. NO schema change.

**Mechanism:** Document in `drive/SKILL.md` that pre-commit halts named in spec text apply only to the stage in which they were registered. The hook from D1 carries the stage-scope rule as part of its "is authorisation missing?" check (reads current pipeline stage from `drive.yaml` and matches against the halt-condition's stage).

**Rationale:** Spec halt-conditions are currently free-form prose. Schema field for stage scope is heavier than the failure mode warrants. Revisit Option 1 (schema with `stage:` field) only if a second instance of stage-cross widening appears.

**Confidence:** Medium. If a second instance surfaces in the wild, escalate to the schema route.

### D5 — Decision Brief framing rule

**Decision:** Prose-only update to `.orbit/STYLE.md` and `.orbit/cards/0026-executive-communication.yaml`. NO separate menu-detection mechanism (D1's hook subsumes the structural enforcement).

**Prose addition (STYLE.md):**
> The Decision Brief shape closes recommendations to the operator. Mid-autonomy in-flight decisions take the imperative single-action form (one line: `Run X on Y`); they do not present a menu of options.

Card 0026 gains the same boundary in its text, with a reference to 0042's three-question test as the structural enforcement.

**Rationale:** The failure-mode is interpretive — agent misapplies a closing-frame to an in-flight moment. The three-question test from D1 prevents the in-flight AskUserQuestion at all; if it fires, the body shape is moot. Prose anchor ensures a future reader of 0026 understands the boundary.

### D6 — Card boundary (0037 / 0038 / 0042)

**Decision:** Different surfaces. Each card touches a distinct lifecycle moment; explicit disjointness map provided below.

**Rationale:** 0037 fires at `/orb:design` open + `spec.close` block; 0038 fires at skill entry resolver; 0042 fires at halt-temptation inside drive/rally. Naming this explicitly lets the rally lead's disjointness check fire on disjoint files. The overlap is small (drive/SKILL.md potentially touched by 0042 and 0038 in different sections).

**Confidence:** Medium — assumes rally lead's disjointness check is path-level not symbol-level.

## Disjointness map (for rally Stage 4)

**Skill prose:**
- `plugins/orb/skills/drive/SKILL.md` — §1.6 severity clarification; three-question test prose introduction.
- `plugins/orb/skills/rally/SKILL.md` — pointer to the three-question test for rally sub-agents.
- `plugins/orb/skills/implement/SKILL.md` — pointer at existing "stop and ask" line reminding agent the three-question test fires first.

**Hooks (conditional on D1 pre-flight):**
- `plugins/orb/hooks/three-question-test.sh` — NEW. PreToolUse hook on AskUserQuestion.
- `plugins/orb/.claude-plugin/plugin.json` — register the hook.

**Substrate (conditional on D1 fallback path):**
- `orbit-state/crates/core/src/verbs.rs` — new `autonomy_authorised` verb (only if hook surface unavailable).
- `orbit-state/crates/cli/src/main.rs` — wire subcommand (only if hook surface unavailable).

**Style/cards:**
- `.orbit/STYLE.md` — D5 paragraph on closing-vs-in-flight framing.
- `.orbit/cards/0026-executive-communication.yaml` — mirror the same boundary.

**Rally siblings:**
- 0037 — disjoint at file level. The hook reads memories produced by `memory.match` (0037 D1); ordering: 0037 ships before 0042.
- 0038 — potential overlap in `drive/SKILL.md`: 0038 edits §"Input contract", 0042 edits §1.6. Different sections; merge cost is light. Sequence 0038 before 0042 if needed.

## Open items

- **D1 pre-flight verification.** Before implementation begins, verify Claude Code's `PreToolUse` hook surface accepts `AskUserQuestion`. If yes, ship the hook. If no, switch to D1 option 3 (CLI verb) and update the disjointness map accordingly.
- **Hook stderr vs structured envelope.** If the hook ships, the message format is fixed: three numbered questions printed to stderr; non-interactive exit code suppresses halt.
- **Stage-cross halt-widening.** D4's prose-only call is a bet; a second instance in the wild escalates to schema route.
