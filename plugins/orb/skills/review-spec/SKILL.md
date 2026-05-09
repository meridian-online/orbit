---
name: review-spec
description: Progressive spec review — depth scales with findings, not upfront classification
context: fork
agent: general-purpose
---

# /orb:review-spec

Stress-test a specification before implementation begins. Every spec gets reviewed. The review's depth scales with what it finds — straightforward specs get a quick structural pass; complex or risky specs automatically deepen.

This skill runs in a **forked context** — a fresh agent session with zero shared conversation history.

Agent prose follows the BLUF / Decision Brief contract — see card 0026 (`.orbit/cards/0026-executive-communication.yaml`).

@.orbit/STYLE.md

## Usage

```
/orb:review-spec <spec-id>
```

The skill takes an orbit-state spec id (e.g. `orbit-state-v0.1`, `0042`) — the spec's `goal` and `acceptance_criteria` are what get reviewed.

## Why Context Separation Matters

A reviewer who watched you build something has confirmation bias. A fresh agent reads the spec cold via `orbit spec show`. Context-separated review catches problems that same-session review misses.

## Instructions

### 1. Gather the Spec

- If a spec-id is provided via $ARGUMENTS: use it.
- If not: report `no spec-id provided — review-spec requires a spec-id under the orbit-state substrate` and stop. There is no auto-discovery branch — the caller knows which spec to review.
- Run `orbit --json spec show <spec-id>` to read the spec's `goal`, `cards` (linked capability cards), `labels`, and `acceptance_criteria`.
- Run `plugins/orb/scripts/orbit-acceptance.sh acs <spec-id>` to enumerate the AC list. The parser emits one tab-separated tuple per AC: `<ac-id>\t<status>\t<description>\t<is_gate>` where `<status>` is `[ ]` or `[x]` and `<is_gate>` is `1` if the AC's `gate` field is true, `0` otherwise.
- The spec's `goal` and parsed AC list together are the authoritative source for this review. Design intent lives in the goal; supporting context (constraint history, prior decisions, related cards) is reachable via the linked card files (`orbit card show <id>`) and prior memories (`orbit memory search <keyword>`).

### 2. Progressive Review

The review runs in passes. Every spec gets Pass 1. Subsequent passes are triggered by findings or content signals — not by upfront classification.

#### Pass 1 — Structural Scan (always runs)

Quick check of spec integrity:

1. **AC testability**: Is each AC specific enough to write a test for? Flag vague criteria ("works correctly", "handles errors gracefully").
2. **Constraint conflicts**: Do any constraints contradict each other or make ACs unreachable?
3. **Scope vs goal**: Does the scope match the goal? Over-specified (ACs beyond what the goal needs)? Under-specified (goal claims more than ACs deliver)?
4. **Obvious gaps**: Error handling mentioned? Rollback plan? Monitoring? Edge cases?
5. **Gate-AC description check (deterministic — no LLM judgement).** For every AC where `orbit-acceptance.sh acs` reports `is_gate=1` (column 4 of the tab-separated output — the AC's `gate` field is true), the AC's description text (column 3 of the parser output — the AC's `description` field) must pass **all three** of the following deterministic rules. Flag a MEDIUM finding naming the gate's id and the specific rule violated if any fail:
   - **Non-empty**: the description text is present and contains at least one non-whitespace character.
   - **Not a placeholder token**: the trimmed value is not (case-insensitive) in the set `{TBD, TODO, FIXME, PLACEHOLDER, XXX, ???}`. Match the trimmed value against the literal token — a sentence that happens to contain `TBD` as a word is not a failure of this rule.
   - **Minimum length**: the trimmed value is at least 20 characters long.

   A vague-but-long description ("it works correctly when the feature is done", 49 chars) does **not** trip this check. That is accepted as a deliberate limitation of the deterministic rule — richer semantic detection is out of scope for Pass 1. The implement skill remains the runtime gate enforcer; Pass 1 adds a structural check only.

   **Substrate note.** Gate detection comes from the parser-emitted `is_gate=1` flag, sourced from the spec's `acceptance_criteria[].gate` boolean field (propagated from the card scenario's `gate: true` field at promotion time). The text-under-test is the AC's `description` field — orbit-state stores ACs as structured records, so the description is the verification statement directly (no markdown parsing).
6. **Content signal scan**: Check whether the spec touches any deepening triggers:
   - Training data, ground truth, model inputs, eval datasets
   - Deployment, infrastructure, cron, production services
   - Cross-system boundaries, shared config, other agents' domains
   - Security, auth, permissions, key management
   - Data migrations, schema changes, backwards compatibility

**After Pass 1:**

- If **zero findings AND no content signals** → APPROVE. Record the pass and stop. A clean structural scan on a well-scoped spec is a valid review.
- If **any finding ≥ MEDIUM severity OR content signals present** → proceed to Pass 2.

#### Pass 2 — Assumption & Failure Analysis (triggered)

Deeper scrutiny, triggered by Pass 1 findings or content signals:

1. **Assumption audit**: List every assumption the spec makes. For each, ask: what happens when this assumption is wrong? Flag assumptions not validated by acceptance criteria.

2. **Failure mode analysis**: For each AC, identify how it could pass in testing but fail in production:
   - Environment differences (dev vs prod, interactive vs cron)
   - Path assumptions (relative vs absolute)
   - Timing assumptions (race conditions, timeouts)
   - Permission assumptions

3. **Test adequacy**: For each AC's verification method — does it actually prove the criterion is met, or only under specific conditions?

**After Pass 2:**

- If **no structural concerns** → deliver verdict based on combined Pass 1 + Pass 2 findings. Most specs stop here.
- If **structural concerns found** (contradicted assumptions, cascading failure modes, untestable ACs, downstream impact unclear) → proceed to Pass 3.

#### Pass 3 — Adversarial Review (triggered)

Full adversarial mode. Only reached when Pass 2 reveals structural problems:

1. **Simultaneous failure**: What happens when multiple assumptions are wrong at the same time?
2. **Cascade analysis**: If AC-01 fails, what happens to AC-02..N? Are there hidden dependencies between criteria?
3. **Rollback feasibility**: Can the changes be undone? What state is left behind on failure?
4. **Impact radius**: What breaks outside the spec's declared scope? What systems downstream consume this spec's outputs?

### 3. Output

Produce a structured review:

```markdown
# Spec Review

**Date:** <today>
**Reviewer:** Context-separated agent (fresh session)
**Spec:** <spec-id>
**Verdict:** APPROVE / REQUEST_CHANGES / BLOCK

---

## Review Depth

| Pass | Triggered by | Findings |
|------|-------------|----------|
| 1 — Structural scan | always | <N> |
| 2 — Assumption & failure | <reason or "not triggered"> | <N or "—"> |
| 3 — Adversarial | <reason or "not triggered"> | <N or "—"> |

## Findings

### [SEVERITY] <title>
**Category:** assumption | failure-mode | test-gap | missing-requirement | constraint-conflict | content-signal
**Pass:** <1 | 2 | 3>
**Description:** What the problem is
**Evidence:** Why you believe this (cite spec lines, interview answers)
**Recommendation:** What to change

---

## Honest Assessment

<one paragraph — is this plan ready? what's the biggest risk?>
```

### Verdict line contract (machine-parseable)

The header line `**Verdict:** APPROVE | REQUEST_CHANGES | BLOCK` is a **contract**, not formatting. Downstream consumers — notably `/orb:drive` — parse the verdict from this line with a strict regex (`^\*\*Verdict:\*\* (APPROVE|REQUEST_CHANGES|BLOCK)\s*$`). Write the line exactly as shown, with one of the three tokens unquoted, case-sensitive, and no trailing prose on the same line. Deviation (lowercase, inline prose, frontmatter, sidecar files) silently breaks the contract.

### Output path (invoked inline vs forked)

- **Inline invocation** (a human running `/orb:review-spec <spec-id>` directly): save to the default sidecar path `.orbit/specs/<spec-id>.review-spec-<date>.md`. For re-reviews on the same date, append `-v2`, `-v3` cycle suffixes (`<spec-id>.review-spec-<date>-v2.md`).
- **Forked-Agent invocation** (e.g. launched by `/orb:drive`): the invoking agent's brief will supply an explicit output path — **use the brief's path verbatim**. It takes precedence over the default. Drive uses cycle-ordinal suffixes (`-v2.md`, `-v3.md`) to disambiguate REQUEST_CHANGES cycles; writing to the default path when the brief specified a cycle-specific path will cause drive to report the review as missing and trigger a retry.

## Verdicts

- **APPROVE**: "I couldn't find problems" (not "this is good")
- **REQUEST_CHANGES**: Specific changes needed before implementation
- **BLOCK**: Plan needs rework — return to `/orb:design` or `/orb:discovery`
