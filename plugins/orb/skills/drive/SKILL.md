---
name: drive
description: Drive a card or spec through the orbit pipeline — promote → review-spec → implement → review-pr — at a declared autonomy level
---

# /orb:drive

Take a card and an autonomy level. Drive the pipeline (promote →
review-spec → implement → review-pr) as a single session. Drive state
lives in `.orbit/specs/<spec-folder>/drive.yaml`; resumption reads that
file. AC state lives in the spec's `acceptance_criteria` field.

## Usage

```
/orb:drive <card_path> [full|guided|supervised]   # fresh drive from card
/orb:drive <spec-id>                              # resume an in-flight drive
/orb:drive                                        # resume the unique in-progress drive, if any
```

### Autonomy Levels

| Level | Behaviour |
|-------|-----------|
| **full** | Agent self-answers in promote / review gates. All stages run without human interaction. Pauses only for PR merge. Requires ≥3 card scenarios. |
| **guided** | Promote runs autonomously. All stages run without intermediate pauses — the reviews ARE the quality gates. Author approves the final review-pr verdict before PR creation. Default. |
| **supervised** | Author greenlights after each stage before proceeding. |

## Input contract

The skill operates on exactly one drive per session. Resolution proceeds
in three branches:

1. **Card path provided** (`/orb:drive <card_path> [autonomy]`). Run
   the pre-flight thin-card refusal (below), then promote (§Promote).

2. **Spec-id provided** (`/orb:drive <spec-id>`). Validate that the
   spec has a `drive.yaml` sidecar; if not, halt and instruct the
   agent to re-invoke with a card path. Otherwise, resume from the
   stage named in `drive.yaml: stage`.

3. **No argument** — query for open specs that have a `drive.yaml`:

   ```bash
   orbit --json spec list --status open \
     | jq -r '.data.result.specs[].id' \
     | while read -r sid; do [[ -f ".orbit/specs/$sid/drive.yaml" ]] && echo "$sid"; done
   ```

   - **Single match** → resume it.
   - **Zero matches** → halt with usage.
   - **Multiple matches** → halt and instruct the agent to pass the
     spec id explicitly, listing the candidates.

## Pre-flight (card path branch only)

**Thin cards block full autonomy.** Read the card and count
`scenarios`. If fewer than 3:

- If autonomy is `full`: **REFUSE.** Output:
  ```
  BLOCKED: Card has N scenario(s) — full autonomy requires ≥3.
  Missing coverage areas to consider:
  - <suggest what scenarios are absent based on the card's goal>
  Add scenarios with /orb:card and retry.
  ```
  Do not proceed. Do not silently downgrade to guided.

- If autonomy is `guided` or `supervised`: proceed (the human is in
  the loop to compensate for thin requirements).

## Promote stage

Promote replaces the old Design + Spec stages.

```bash
SPEC_ID=$(plugins/orb/scripts/promote.sh "<card_path>")
```

`promote.sh` materialises a spec under `.orbit/specs/<spec-id>/spec.yaml`
(folder layout) seeded from the card's scenarios as ACs. The returned
`SPEC_ID` is the spec's id (matching the folder name).

Then write the drive sidecar:

```bash
cat > ".orbit/specs/$SPEC_ID/drive.yaml" <<EOF
spec_id: $SPEC_ID
card_path: <absolute card_path>
autonomy: <full|guided|supervised>
iteration: 1
stage: review-spec
review_spec_cycle: 0
review_spec_date: null
review_pr_cycle: 0
review_pr_date: null
iteration_history: []
EOF
```

After promote, schedule the heartbeat (full autonomy only — see below)
and proceed to Stage 1.

## Heartbeat (full autonomy only)

Skip this section entirely when `autonomy != full`.

Drive uses CronList-first idempotent reconciliation: never
delete-then-recreate, so Claude Code's `--resume` task restoration is
preserved.

1. `CronList` — enumerate active cron tasks.
2. If `drive-checkin-<spec-id>` already exists, no-op.
3. Otherwise, `CronCreate` recurring with ID `drive-checkin-<spec-id>`,
   interval 5 minutes (hardcoded), prompt body verbatim:

```
This is a drive heartbeat. Run `orbit --json spec show <spec-id>` and
read its `status`. Read `.orbit/specs/<spec-id>/drive.yaml` and read
its `stage`.

If status is `closed` (drive stage `complete` or `escalated`), call
CronDelete with ID `drive-checkin-<spec-id>` and emit:

  drive: heartbeat stopped (stage=<drive_stage>)

Then stop. Do not emit a heartbeat line.

Otherwise: run `plugins/orb/scripts/orbit-acceptance.sh next-ac
<spec-id>` to find the current AC (if it returns nothing or stage is
not `implement`, use `-`). Compute elapsed as mm:ss since drive.yaml's
`started_at` field (set on the first heartbeat tick if absent). Emit
exactly one line in the format:

  drive: spec=<spec-id> stage=<stage> ac=<id|-> elapsed=<mm:ss>

Do not modify the spec. Do not launch any Agent. Emit the single
heartbeat line and stop.
```

The heartbeat self-terminates when `spec.status == closed`
(defence-in-depth; primary cleanup is in §Completion / §Escalation).
If `CronCreate` fails, log `heartbeat unavailable: <reason>` and
continue — the heartbeat is observability, not a gate.

## Stage 1: Review-Spec

Review-spec runs as a **forked Agent** via the Agent tool — fresh
context, no shared conversation history.

### 1.1 Compute the cycle-specific verdict path

Read `review_spec_cycle` from `drive.yaml`. Let N = that value + 1
(the cycle ordinal for this fork — 1-indexed).

Capture or reuse the date token:

- If `review_spec_date` is null, set it to today's ISO date and write
  it back to `drive.yaml`. This is cycle 1's date.
- Otherwise, reuse the stored value. The date is fixed at cycle 1 for
  the whole stage so long-running drives don't split cycle files
  across date boundaries.

Compute the output path:

- **Cycle 1:** `.orbit/specs/<spec-id>/review-spec-<date>.md`
- **Cycle 2:** `.orbit/specs/<spec-id>/review-spec-<date>-v2.md`
- **Cycle 3:** `.orbit/specs/<spec-id>/review-spec-<date>-v3.md`

### 1.2 Idempotent resumption check

Before launching any fork, check whether a valid review already exists
at the cycle-specific path. If the file exists AND contains a line
matching the canonical verdict regex (§1.4), parse that verdict and
proceed to §1.5 verdict handling **without launching any Agent**.
Otherwise, continue to §1.3.

### 1.3 Launch the forked review

**Pre-flight: verify Agent tool availability.** Run `ToolSearch
select:Agent` to load the Agent schema. If ToolSearch returns no
result, do NOT fall back to inline review — escalate immediately:

- Set `stage: escalated` in `drive.yaml`.
- Output: `Agent tool unavailable — cannot launch cold-fork review for review-spec`
- Stop. Inline review violates the cold-fork separation contract.

Invoke the Agent tool with:

- `subagent_type: general-purpose`
- A brief containing **only**:
  - The spec-id whose acceptance the reviewer must read
  - The absolute path where the review must be written (§1.1)
  - The instruction to read the spec via `orbit --json spec show
    <spec-id>`, parse ACs via
    `plugins/orb/scripts/orbit-acceptance.sh acs <spec-id>`, follow
    the `/orb:review-spec` skill, and write the verdict to the
    specified path using the canonical verdict line format

Example brief:

```
Run /orb:review-spec on spec <spec-id>. Read the spec via `orbit --json
spec show <spec-id>` and parse ACs via
`plugins/orb/scripts/orbit-acceptance.sh acs <spec-id>` — the spec's
acceptance_criteria field is the authoritative spec for this review.
Write the review to exactly <absolute output path> (this path takes
precedence over the default path in the skill). Use the canonical
verdict line format `**Verdict:** APPROVE | REQUEST_CHANGES | BLOCK`.
```

The brief must NOT include any conversation context, iteration counter,
cycle number, or pointers to prior review files.

### 1.4 Parse the verdict from the file

After the fork returns, read the file at the cycle-specific output
path. Locate the first line matching:

```
^\*\*Verdict:\*\* (APPROVE|REQUEST_CHANGES|BLOCK)\s*$
```

Match is case-sensitive on the verdict token. No fuzzy matching. If
zero matches or the file is missing, fall through to the retry.

**Retry on missing verdict (budget: 1).** Launch one retry fork with a
fresh brief identical to the original. The retry overwrites the same
path. Retry does NOT increment `review_spec_cycle`. If the retry
also produces no parseable verdict, drive escalates with
`stage: escalated` in `drive.yaml` and the message `review could not be
completed after 2 forked attempts at review-spec`.

### 1.5 Verdict handling

- **APPROVE:** Set `stage: implement` in `drive.yaml`. Proceed to
  Stage 2.

- **REQUEST_CHANGES:**
  - Increment `review_spec_cycle` in `drive.yaml`.
  - Check the budget (§1.6).
  - If the budget allows another cycle: address the findings (edit the
    spec via `orbit spec update <spec-id> --goal "..."` for goal
    revisions, or rewrite acceptance_criteria via
    `orbit spec update --ac-check / --ac-uncheck` for individual AC
    flips, or `orbit spec note <spec-id> "<context>"` for narrative
    edits), then return to §1.1 to recompute the cycle-specific output
    path and re-fork.

- **BLOCK:** Jump to §NO-GO Handling. The block reason becomes the
  NO-GO constraint.

### 1.6 REQUEST_CHANGES budget & synthetic BLOCK

Each stage (review-spec, review-pr) has an **independent budget of 3
REQUEST_CHANGES cycles per top-level iteration**. The counters live in
`drive.yaml` (`review_spec_cycle`, `review_pr_cycle`) and reset to 0
when a new iteration's spec is created (§NO-GO).

After incrementing the counter on a REQUEST_CHANGES verdict:

- If the new value is **< 3**: the stage has budget remaining. Address
  the findings and launch the next cycle.
- If the new value **== 3**: this was the 3rd real REQUEST_CHANGES on
  the stage in this iteration. The budget is exhausted. Do NOT launch
  a 4th fork. Synthesise a BLOCK with the canonical constraint string:

  > `review converged on REQUEST_CHANGES after 3 iterations; findings have not been addressable within budget`

  This string is fixed and **byte-identical** with the spec ac-05
  verification target. Do not paraphrase. The synthetic BLOCK consumes
  a top-level iteration the same way a real BLOCK does — jump to §NO-GO.

**Resumption case:** If drive resumes with `review_<stage>_cycle == 3`
and the synthetic BLOCK was not yet written (session died between the
counter increment and the NO-GO write), synthesise the BLOCK on resume
— do not launch a 4th fork.

### 1.7 Supervised mode gate (review-spec)

If autonomy is `supervised` AND the verdict was APPROVE, pause here.
**Severity dispatch (see §Four-option verdict prompt):**

- **No findings or LOW-only findings:** use the 2-option prompt:
  ```
  AskUserQuestion: "Spec review complete — verdict: APPROVE. <N> findings (<severities>). Review saved at <path>. Proceed to implementation?"
  Suggested answers: ["GO — proceed to implement", "NO-GO — re-enter at promote"]
  ```
- **At least one MEDIUM or HIGH finding:** use the four-option verdict
  prompt (§Four-option verdict prompt) with `approve / request changes
  / block / read full review first`.

If NO-GO or `block` → §NO-GO. If `request changes` → increment
`review_spec_cycle` and return to §1.1 (budget-gated).

## Stage 2: Implement

Drive sets `stage: implement` in `drive.yaml` and delegates entirely to
`/orb:implement`:

```bash
# Edit drive.yaml's stage field, then:
# (invoke /orb:implement with the spec id)
```

Drive does NOT inline AC tracking, detour escalation, or progress
emission — those are owned by `/orb:implement`. When implement returns
(the spec's acceptance_criteria field has no unchecked ACs — verifiable
via `orbit-acceptance.sh has-unchecked <spec-id>` exiting 1), drive
sets `stage: review-pr` in `drive.yaml` and proceeds to Stage 3.

**Supervised mode gate (implement):** If autonomy is `supervised`,
pause after implement returns:

```
AskUserQuestion: "Implementation complete. <N>/<total> ACs addressed. Review and greenlight to continue, or NO-GO to re-enter at promote."
Suggested answers: ["GO — proceed to review-pr", "NO-GO — re-enter at promote"]
```

If NO-GO → §NO-GO Handling.

## Stage 3: Review-PR

Mirrors Stage 1 mechanics with the diff brief. The forked reviewer
reads the post-implement spec state directly via `orbit --json spec
show <spec-id>` and `orbit-acceptance.sh acs <spec-id>` — the
acceptance_criteria field may have been edited during implement, and
the live `orbit` query gives the reviewer the up-to-date state with no
intermediate artefact.

### 3.1 Compute the cycle-specific verdict path

Using `review_pr_cycle` and `review_pr_date` from `drive.yaml`:

- Cycle 1: `.orbit/specs/<spec-id>/review-pr-<date>.md`
- Cycle 2: `.orbit/specs/<spec-id>/review-pr-<date>-v2.md`
- Cycle 3: `.orbit/specs/<spec-id>/review-pr-<date>-v3.md`

### 3.2 Idempotent resumption check, fork launch, verdict parse

As §1.2 / §1.3 / §1.4, with these differences:

- The Agent brief includes the diff reference (`git diff main...HEAD`
  on the current branch) PLUS the spec-id for AC cross-reference (the
  reviewer reads the live acceptance_criteria field via `orbit spec
  show` and `orbit-acceptance.sh`).
- Output path uses `review-pr` in place of `review-spec`.
- Counter / date fields use `review_pr_*`.
- Retry escalation message: `review could not be completed after 2
  forked attempts at review-pr`.

Example brief:

```
Run /orb:review-pr against the current branch. Implementation diff is
`git diff main...HEAD` on <branch_name>. Spec acceptance is on spec-id
<spec-id>; read via `orbit --json spec show <spec-id>` and
`plugins/orb/scripts/orbit-acceptance.sh acs <spec-id>`. Write the
review to exactly <absolute output path> (this path takes precedence
over the default path in the skill). Use the canonical verdict line
format `**Verdict:** APPROVE | REQUEST_CHANGES | BLOCK`.
```

### 3.3 Verdict handling

- **REQUEST_CHANGES:** Increment `review_pr_cycle`. Check budget
  (§1.6). If budget remains, address findings (edit the implementation),
  return to §3.1 for the next cycle. If budget exhausted, synthesise
  BLOCK.

- **BLOCK (real or synthetic):** Jump to §NO-GO Handling.

- **APPROVE:**
  - **In full mode:** Proceed directly to §Completion.
  - **In guided mode:** This is the **only gate in guided mode**.
    Severity dispatch:
    - **No findings or LOW-only findings:** three-option rich summary:
      ```
      AskUserQuestion: "Drive summary for <card name>:

      Spec: <spec-id> — <goal>
      Spec review: <verdict>, <N> findings
      Implementation: <N>/<total> ACs addressed
      PR review: APPROVE — <one-liner>

      Review saved at <path>. Proceed to PR creation?"
      Suggested answers: ["GO — create PR", "NO-GO — re-enter at promote", "Let me read the reviews first"]
      ```
      `Let me read the reviews first` defers — wait for the author's
      next turn, then re-present the gate.
    - **At least one MEDIUM or HIGH finding:** four-option verdict
      prompt (§Four-option verdict prompt), prefaced by the same
      drive-summary block.
  - **In supervised mode:** Same gate as guided.

## Four-option verdict prompt

When a review-spec supervised-APPROVE gate or a review-pr
guided/supervised APPROVE gate dispatches to the four-option prompt,
the following rules apply uniformly.

**When the four-option prompt fires.** Only on APPROVE verdicts where
the review file reports at least one finding at MEDIUM or HIGH
severity. REQUEST_CHANGES and BLOCK verdicts route via the existing
branch-to-next-cycle and NO-GO paths — the four-option prompt never
replaces those.

**Severity-read contract.** Drive reads severity labels (LOW / MEDIUM
/ HIGH) directly from the review file's findings table. Drive does
NOT re-classify findings, and does NOT invent severities.

**The four options (exact labels).** Use these labels verbatim as
AskUserQuestion suggested answers — lower-case, single spaces, no
hyphens, no punctuation:

```
approve
request changes
block
read full review first
```

**Interpretation.**

- `approve` — terminal verdict. Drive advances to the next stage
  (implement after a spec gate; §Completion after a PR gate).
- `request changes` — treated as a post-APPROVE REQUEST_CHANGES:
  drive increments `review_<stage>_cycle`, checks the §1.6 budget,
  and re-enters the review cycle.
- `block` — drive jumps to §NO-GO Handling. The constraint is
  `author blocked post-APPROVE at MEDIUM+ <review-spec | PR> review`.
- `read full review first` — **deferral, not a verdict.** Drive waits
  for the author's next turn; on their next turn drive re-presents
  the **same four-option prompt verbatim**.

## Completion

On APPROVE at review-pr (interactive gates per autonomy mode passed):

1. **Stage and commit the implementation** (commit 1):
   - All code changes and the review files
   - Commit message: `feat: <spec goal>`

2. **Propose card updates** (commit 2):
   - Update the card's `maturity` if appropriate
   - Refine the card's `goal` if implementation revealed more precise
     success criteria
   - Commit message: `docs: update <card> — maturity and goal after drive`

3. **Create the PR:**
   - Title: `drive: <spec goal>`
   - Body references the spec-id and review files

4. **Set drive.yaml stage and close the spec:**
   ```bash
   # Edit drive.yaml: stage: complete
   orbit spec note <spec-id> "drive completed: <one-line summary>"
   orbit spec close <spec-id>
   ```

   `spec.close` transactionally appends the spec's path to every linked
   card's `specs` array. It rejects if any open child tasks remain;
   resolve those first.

5. **Heartbeat cleanup (full autonomy only).** Attempt `CronDelete
   drive-checkin-<spec-id>`. **Failure is non-fatal** — log
   `heartbeat cleanup skipped: <reason>` and continue. The spec is
   already closed; the next heartbeat tick (if any) self-terminates
   on `spec.status == closed`.

## NO-GO Handling

A NO-GO means the current iteration failed a review (real or
synthetic BLOCK) or was rejected at a supervised gate.

1. **Note and close the current spec:**
   ```bash
   orbit spec note <spec-id> "NO-GO: <one-line constraint>"
   orbit spec close <spec-id>
   ```

   If `spec close` rejects due to open child tasks, mark them done
   first (`orbit task done <task-id>`) — the NO-GO captures their
   outcome via the spec note.

2. **Persist the constraint to memory:** the CLI takes the key and body
   as separate positional args.

   ```bash
   orbit memory remember drive-<card-slug>-iter<N> "<constraint>"
   ```

   The key format is stable so iteration ≥2 can list all prior
   constraints with `orbit memory search drive-<card-slug>`.

3. **Check budget:** Read `iteration` from `drive.yaml`. If
   `iteration == 3`, jump to §Escalation.

4. **Promote a new iteration spec:**
   ```bash
   NEW_SPEC=$(plugins/orb/scripts/promote.sh "<card_path>")
   ```

5. **Inject the cumulative constraint history into the new spec's
   goal (or as a leading note):**
   ```bash
   CONSTRAINTS=$(orbit --json memory search "drive-<card-slug>" \
     | jq -r '.data.result.memories[] | "- " + .body')
   orbit spec note "$NEW_SPEC" "Constraints carried from prior iterations:
   $CONSTRAINTS"
   ```

6. **Seed the new spec's drive.yaml (incremented iteration, fresh
   review cycles, prior history populated):**
   ```bash
   cat > ".orbit/specs/$NEW_SPEC/drive.yaml" <<EOF
   spec_id: $NEW_SPEC
   card_path: <card_path>
   autonomy: <level>
   iteration: $((<N>+1))
   stage: review-spec
   review_spec_cycle: 0
   review_spec_date: null
   review_pr_cycle: 0
   review_pr_date: null
   iteration_history:
     - spec_id: <closed-spec-id>
       iteration: <N>
       outcome: NO-GO
       constraint: <one-line>
   EOF
   ```

7. **Re-enter at Stage 1** with the new spec. The constraint history
   is now in its first spec.note; the cold-fork reviewer reads it as
   part of the spec's note stream.

## Escalation

The drive's job is to find the way through, not the evidence that
closes the card. Escalation is not giving up — it is the mechanism by
which difficult work gets human judgment at the right moment.

Escalation is triggered by **iteration budget exhaustion**
(`iteration == 3` and current iteration NO-GO'd) OR by a **semantic
trigger** — an honest agent may escalate before the budget is spent
when:

- **Recurring failure mode** — the same problem has appeared across 2+
  iterations despite varied approaches. The constraint may be
  structural, not configurational.
- **Contradicted hypothesis** — accumulated evidence points to the
  card's *underlying goal* being unreachable, not just the current
  approach falling short. The call to pivot a thesis belongs to the
  author.
- **Diminishing signal** — each iteration is producing less new
  information than the last. The drive is grinding, not learning.

### Steps

1. **Set drive.yaml stage and close:**
   ```bash
   # Edit drive.yaml: stage: escalated
   orbit spec note <spec-id> "ESCALATED: <reason>"
   orbit spec close <spec-id>
   ```

2. **Output the escalation summary.** Iteration history is read from
   the chain of `iteration_history` entries across each iteration's
   `drive.yaml`:

   ```bash
   # Walk back through iteration_history starting at the current spec.
   # Each entry names the prior iteration's spec_id and constraint.
   ```

   Format:

   ```
   DRIVE ESCALATED — <reason: budget exhausted | recurring failure | contradicted hypothesis | diminishing signal>

   Card: <card path>
   Goal: <card goal>

   Iteration history:
     1. <spec-id-iter1> — NO-GO: <constraint from orbit memory search>
     2. <spec-id-iter2> — NO-GO: <constraint>
     [3. <spec-id-iter3> — NO-GO: <constraint>]

   Accumulated constraints:
     - <all constraints from orbit memory search drive-<card-slug>>

   What would have to be true:
     <For a future attempt to succeed, what assumptions need revisiting?
      What constraints are structural vs configurational?
      What corner of the solution space was not explored?>

   Recommendation:
     <What the card needs before another drive attempt.>
   ```

3. **Heartbeat cleanup (full autonomy only).** Attempt `CronDelete
   drive-checkin-<spec-id>`. Non-fatal — failure logs `heartbeat
   cleanup skipped: <reason>` and continues. This step executes
   **before** the escalation ping so the recurring heartbeat can't
   fire between the summary output and the ping.

4. **One-shot escalation ping (full autonomy only).** Schedule
   `CronCreate` ~30 seconds out:
   - **Delay:** ~30 seconds (one-shot, not recurring).
   - **Task ID:** `drive-escalation-<spec-id>`.
   - **Prompt body (verbatim):**

     ```
     **DRIVE ESCALATED** on <card-slug> after <iterations> iterations. See prior output for findings and recommendation.
     ```

   If `CronCreate` for the ping fails, log `escalation ping skipped:
   <reason>` and continue. The escalation summary in step 2 is the
   authoritative channel; the ping is notification amplification.

5. **Stop.** The card needs human rethinking.

## Critical Rules

These are invariants — not duplicates of the body. The body describes
what to do at each step; these rules describe what must always hold.

- **drive.yaml is the single source of orchestration state.** Do not
  track drive state in any other file. The drive.yaml `stage` field
  is the source of truth for resumption.
- **Reviews run as forked Agents in cold context.** Every review is a
  fresh fork via the Agent tool — no shared conversation history, no
  iteration counter, no prior-finding pointers. Re-reviews after
  REQUEST_CHANGES are functionally identical to first-cycle reviews.
- **Verdicts are read from disk only.** The review file's canonical
  verdict line (regex in §1.4) is the single authoritative source.
  The fork's chat response is never parsed.
- **REQUEST_CHANGES is bounded per stage** (3-cycle budget per
  iteration). The 4th would-be cycle is converted to a synthetic
  BLOCK with the byte-identical canonical constraint string in §1.6.
- **Iteration is bounded by 3 specs in the iteration_history chain.**
  After three NO-GOs, drive escalates. Earlier escalation is permitted
  on semantic triggers (§Escalation).
- **Never silently downgrade autonomy.** If full mode is requested
  but the card is thin, refuse explicitly. The thin-card guard is a
  pre-qualification gate, not a runtime decision.

## Resumption

When `/orb:drive` is invoked with a spec-id (or detects an in-progress
drive per §Input contract):

1. **Read drive.yaml:** `.orbit/specs/<spec-id>/drive.yaml`. Extract:
   - `stage`
   - `iteration`
   - `review_spec_cycle`, `review_pr_cycle`
   - `review_spec_date`, `review_pr_date`
   - `card_path`, `autonomy`

2. **Resume at the named stage.** No file-presence detection. The
   `drive.yaml` is the source of truth.

   | stage         | Resume at                                         |
   |---------------|---------------------------------------------------|
   | `review-spec` | Stage 1 (idempotent §1.2 check skips fork if file already valid) |
   | `implement`   | Stage 2 (delegate to /orb:implement <spec-id>)    |
   | `review-pr`   | Stage 3                                           |
   | `complete`    | Already done — report status                      |
   | `escalated`   | Already escalated — report status                 |

3. **Synthetic-BLOCK resumption.** If `review_<stage>_cycle == 3` and
   the spec is still open (the synthetic BLOCK was not written before
   the session died), synthesise the BLOCK on resume per §1.6 — do
   not launch a 4th fork.

4. **Heartbeat reconciliation (full autonomy only).** Re-run the
   §Heartbeat CronList-first flow: if `drive-checkin-<spec-id>`
   exists, leave it; if absent, re-create it. Drive never
   delete-then-recreates, so a surviving task is preserved.

5. **Announce the resumption** in one line: spec id, stage, iteration,
   review-cycle counts, heartbeat status.

---

**Next step:** after `orbit spec close` at completion, the PR is ready
for human review and merge.
