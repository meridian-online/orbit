---
name: review-pr
description: Context-separated PR review — runs tests, checks AC coverage, verifies implementation
context: fork
agent: general-purpose
---

# /orb:review-pr

Verify an implementation before merge. This skill runs in a **forked context** — a fresh agent session with execution permissions that reads the diff cold.

Agent prose follows the BLUF / Decision Brief contract — see card 0026 (`.orbit/cards/0026-executive-communication.yaml`).

@.orbit/STYLE.md

## Usage

```
/orb:review-pr <spec-id> [branch_or_pr]
```

The skill takes an orbit-state spec id — the spec's `acceptance_criteria` are the implementation contract. The branch/PR argument is optional; if omitted the current branch is used.

## Instructions

### 1. Identify What to Review

- The spec-id is required via $ARGUMENTS. If not provided, report `no spec-id provided — review-pr requires a spec-id under the orbit-state substrate` and stop.
- If a branch name or PR number is provided alongside the spec-id: use it.
- If not: use the current branch or most recent PR.
- Gather the diff: `git diff main...HEAD`.

### 2. Phase 1: Read the Diff

1. Run `git diff main...HEAD` to see all changes.
2. Read the spec via `orbit --json spec show <spec-id>` to understand what was intended — the `goal` field carries the goal and the `acceptance_criteria` array enumerates the contract.
3. Run `plugins/orb/scripts/orbit-acceptance.sh acs <spec-id>` to enumerate the AC list with current check status. The spec's `acceptance_criteria` field replaces the earlier `progress.md` tracker — `[x]` marks are the implementer's self-reported AC completions, set by `/orb:implement` via `orbit-acceptance.sh check` (which calls `orbit spec update --ac-check`).
4. Identify which acceptance criteria this implementation claims to satisfy from the parsed `[x]` rows.
5. Run a keyword scan (see `/orb:keyword-scan`) against `.orbit/choices/` using terms from the spec's `goal` and any prose in the linked card files (`orbit card show <id>`). If relevant decisions exist, verify the implementation respects them. Flag violations as findings.

### 3. Phase 2: Run Tests + AC Coverage Check

1. Run the project's test suite. Record pass/fail with output.
2. **AC-to-test coverage check**: For every AC parsed in Phase 1, search the project's test sources for a test bearing the bare AC identifier (`ac<NN>` or `ac-NN`).

```
AC Coverage Report:
  ac-01:   ✓ ac01_creates_project_structure
  ac-02:   ✓ ac02_manifest_has_correct_fields
  ac-03:   ✗ NO TEST FOUND
  ac-04:   ✓ ac04_handles_edge_case
  Coverage: 3/4 ACs have tests (75%)
```

Cross-language patterns to search:
- Rust: `fn ac<NN>` or `fn test_ac<NN>`
- Python: `def test_ac<NN>` or `def ac<NN>`
- TypeScript: `test('ac<NN>` or `it('ac<NN>`
- Bash/general: grep for `ac<NN>` or `ac-<NN>` in test directories

In the honest-assessment paragraph, contextualise which uncovered ACs are doc/gate-style (judged from each AC's description text — e.g. an AC that names a documentation deliverable or a sequencing gate, not a code change) versus genuine test gaps. The orbit-state spec carries description text and a `gate` flag per AC; the reviewer reads the description and judges whether a missing test is a real gap or an exempt non-code AC.

### 4. Phase 3: Environment Simulation

For changes that touch deployment, infrastructure, scripts, or cron:
1. Identify the deployment context
2. Simulate it (run from $HOME, minimal PATH, etc.)
3. Record what you ran and what happened

### 5. Phase 4: Edge Case Probing

1. First run? (No prior state, empty databases, missing dirs)
2. Failure? (Network down, service unavailable)
3. Repeat? (Idempotency — running twice shouldn't break things)
4. Boundary conditions? (Empty input, max input, unicode)

### 6. Output

```markdown
# Pre-Merge Review

**Date:** <today>
**Reviewer:** Context-separated agent (fresh session)
**Branch:** <branch>
**Spec:** <spec-id>
**Verdict:** APPROVE / REQUEST_CHANGES / BLOCK

---

## Test Results

| Check | Result | Details |
|-------|--------|---------|
| Test suite | PASS/FAIL | N/M tests |
| AC coverage | X/Y | See report below |

## AC Coverage Report

| AC | Status | Test(s) |
|----|--------|---------|
| ac-01 | ✓ | ac01_description |
| ac-02 | ✗ | NO TEST FOUND |

## Findings

### [SEVERITY] <title>
**Category:** bug | test-gap | environment-mismatch | edge-case | security | performance
**Description:** What the problem is
**Evidence:** Command output or file:line reference
**Recommendation:** Specific fix

---

## Honest Assessment

<one paragraph>
```

### Verdict line contract (machine-parseable)

The header line `**Verdict:** APPROVE | REQUEST_CHANGES | BLOCK` is a **contract**, not formatting. Downstream consumers — notably `/orb:drive` — parse the verdict from this line with a strict regex (`^\*\*Verdict:\*\* (APPROVE|REQUEST_CHANGES|BLOCK)\s*$`). Write the line exactly as shown, with one of the three tokens unquoted, case-sensitive, and no trailing prose on the same line. Deviation (lowercase, inline prose, frontmatter, sidecar files) silently breaks the contract.

### Output path (invoked inline vs forked)

- **Inline invocation** (a human running `/orb:review-pr <spec-id>` directly): save to the default path `.orbit/specs/<spec-folder>/review-pr-<date>.md` if the spec is folder-shaped, otherwise `.orbit/reviews/<spec-id>/review-pr-<date>.md`.
- **Forked-Agent invocation** (e.g. launched by `/orb:drive`): the invoking agent's brief will supply an explicit output path — **use the brief's path verbatim**. It takes precedence over the default. Drive uses cycle-ordinal suffixes (`-v2.md`, `-v3.md`) to disambiguate REQUEST_CHANGES cycles; writing to the default path when the brief specified a cycle-specific path will cause drive to report the review as missing and trigger a retry.

## Critical Rules

- **Evidence over reasoning.** Every CRITICAL finding must include command output or file:line citations.
- The reviewer sees the diff and spec but has NO context from the implementing session.
- **Never suggest "open a follow-up card."** If you identify adjacent work or future improvements, note them in the Findings section. The implementing agent handles forwarding via memos — cards describe capabilities, not work items.
