---
name: rally
description: Coordinate multiple independent cards through the orbit pipeline as a single multi-card delivery — proposal → queued design decisions → consolidated design review → implementation → stacked or batched review. State lives in rally.yaml; the children graph IS the rally.
---

# /orb:rally

Drive **multiple** independent cards through the orbit pipeline as a coordinated rally. A rally packs the human work into two high-signal gates (ideation and assurance) and lets the agent work between them with maximum clarity from the best available evidence — instead of paying a per-card touchpoint tax for design, spec, review, and PR. State lives in `rally.yaml` at proposal-approval time; the `children` array encodes the implementation order via `dep_predecessors`. Individual cards run `/orb:drive <spec-id>` in full autonomy (serially in the main checkout, or in parallel inside isolated worktrees).

## Usage

```
/orb:rally <goal_string> [guided|supervised]   # fresh rally from a goal
/orb:rally <rally-folder>                      # resume an existing rally
/orb:rally                                     # resume the unique in-progress rally, if any
```

- `goal_string` — a short description of the subsystem, theme, or
  objective binding the cards together (e.g. `"pipeline runtime
  readiness"`, `"review workflow hardening"`)
- Autonomy defaults to **guided** if omitted

### Autonomy Levels

| Level | Behaviour |
|-------|-----------|
| **guided** | Proposal and consolidated decision gate are interactive. Reviews serve as quality gates — no intermediate supervision between design and implementation. Default. |
| **supervised** | Same as guided plus explicit pauses after each rally phase (design, review, implementation, PR) for author greenlight. |

`full` autonomy is **not offered** — rally's value comes from sharper
human gates, not fewer.

**Rally-level vs drive-level autonomy.** Rally-level autonomy
(guided | supervised) governs pauses between rally phases — proposal,
consolidated decision gate, consolidated design review, batched diff
review. Drive-level autonomy inside a rally is **always full**, both
for parallel sub-agents running in worktrees and for serial cards
running in the main checkout.

## Input contract

The skill operates on exactly one rally per session. Resolution
proceeds in three branches:

1. **Goal string provided** (`/orb:rally <goal_string> [autonomy]`).
   Run §pre-flight (scan-for-active-rally + thin-card guard), then
   §Stage 1 (Proposal).

2. **Rally folder provided** (`/orb:rally <rally-folder>`). Validate
   that the folder contains `rally.yaml` and its `phase` is not
   `complete`; if not, halt and instruct the agent to start a fresh
   rally. Otherwise, resume from the phase named in `rally.yaml.phase`.

3. **No argument** — query for in-progress rallies:

   ```bash
   for f in .orbit/specs/*-rally/rally.yaml; do
     [[ -f "$f" ]] || continue
     phase=$(yq -r '.phase // ""' "$f")
     [[ "$phase" != "complete" && -n "$phase" ]] && dirname "$f"
   done
   ```

   - **Single match** → resume it.
   - **Zero matches** → halt with usage (a goal string is required to
     start a fresh rally).
   - **Multiple matches** → halt and instruct the agent to pass the
     rally folder explicitly, listing the candidates.

## Pre-flight

### 1. Scan for an active rally

Before launching a fresh rally, the input-contract resolution above
already ensures no in-progress rally exists. The skill never launches
a second rally over a first. If a stale rally is the problem, close it
explicitly by setting `phase: complete` in its `rally.yaml` before
starting a fresh rally.

**One active rally at a time** is non-negotiable. Children-graph
orchestration loses its meaning if two rallies overlap on cards.

### 2. Thin-card guard (refuse at proposal)

Before the proposal is presented to the author, check the scenario
count on every candidate card. If any candidate has fewer than
**3 scenarios**, the proposal refuses to proceed:

```
Rally cannot proceed — the following candidate card is too thin:

  .orbit/cards/0017-<slug>.yaml — 2 scenarios

Thicken this card via `/orb:card .orbit/cards/0017-<slug>.yaml` or remove it from
the rally list before continuing.
```

The author may then:

- Run `/orb:card` on the thin card to thicken it, re-invoke rally
- Remove the thin card from the list and re-invoke rally
- Run the thin card individually via `/orb:drive <card> guided` or
  `supervised` (rally is not the venue for thin cards)

The thin-card refusal is **unconditional on the eventual serial-or-
parallel outcome**. The guard runs before the proposal is shown and
before the post-design disjointness check (§Stage 4) — it is a
pre-qualification gate, not a runtime decision. **No silent
downgrade.**

## Stage 1: Proposal

Parse the goal string from `$ARGUMENTS[0]` and autonomy from
`$ARGUMENTS[1]` (default `guided`).

**Scan `.orbit/cards/` for candidate cards:**

1. Read every `.orbit/cards/*.yaml` (ignore `.orbit/cards/memos/`)
2. For each card, score relevance to the goal string using the card's
   `feature`, `goal`, `scenarios`, and `references`
3. Surface the top candidates (usually 3–6) with a one-line rationale
   per card

Run the §thin-card guard against the candidate list before showing
anything to the author. If any candidate is thin, halt per the guard.

### Present the proposal using AskUserQuestion

The proposal surface has two strict halves: a **markdown preview
block** that carries the evidence (per-card rationale), and an
**AskUserQuestion** that carries the decision (three canonical, terse
options). They are not collapsed — the preview block scales with N
cards while the AskUserQuestion stays short and action-focused.

**Preview block (markdown, above the AskUserQuestion) — owns per-card
rationale:**

```
## Rally Proposal — <goal string>

Candidate cards:
  1. .orbit/cards/<id>-<slug>.yaml — <feature line>
     Rationale: <why this card fits the goal>
  2. .orbit/cards/<id>-<slug>.yaml — <feature line>
     Rationale: <why this card fits the goal>
  ...

Autonomy: <guided|supervised>
```

**AskUserQuestion — owns the decision.** Exactly three canonical
options in this order. The `description` field for each option is a
one-line **action summary** — it describes the action, not the cards.
Per-card rationale must not appear in these descriptions (the preview
block already owns it).

- **`approve-all`** — `Proceed with all N candidates`
- **`modify-list`** — `Add or remove cards before proceeding`
- **`decline`** — `Abort the rally; offer individual drive as alternative`

**On `approve-all`:** proceed to §Create the rally (below).

**On `decline`:** abort the rally and offer individual `/orb:drive`
invocations as the alternative.

**On `modify-list`:** the lead issues exactly one follow-up
AskUserQuestion with **no pre-populated options** (free-form only).
The prompt text reads:

> *Name cards to add (by path, e.g. `.orbit/cards/0019-foo.yaml`) or
> remove (by number, e.g. `2`). Empty response cancels the modification
> and returns to the approval prompt.*

An empty response cancels the modification and re-presents the
unchanged candidate list with the same three canonical options. A
non-empty response is interpreted as modification instructions.

**Modify loop — sequence per iteration:**

1. **Apply** the requested additions and removals to the candidate
   list.
2. **Re-run the §thin-card guard** against the revised list. The
   guard's rules live in §pre-flight and are not restated here; only
   the re-run behaviour is named.
3. **Re-present** the revised preview block plus the AskUserQuestion
   with the same three canonical labels.

**Invariant:** no candidate list is shown to the author unless it has
passed the thin-card guard in the current loop iteration. The author
never decides against a list that cannot fly.

Guard re-runs inside the modify loop are **pre-qualification retries**
— they are not rally-level strikes and do not count against any
escalation budget.

The loop continues — verdict → (modify instructions → apply →
re-guard → re-present) → verdict → … — until the author returns
`approve-all` or `decline`.

**The proposal gate is the only pre-design independence check.** The
agent's scan proposes; the author's approval qualifies. Do not attempt
a lightweight heuristic disjointness check — the definitive check
happens after designs exist (§Stage 4).

### Create the rally folder, rally.yaml, and child specs

On `approve-all`:

```bash
# 1. Derive a rally slug from the goal string
SLUG=$(echo "<goal string>" | tr '[:upper:]' '[:lower:]' | sed 's/[^a-z0-9]\+/-/g; s/^-//; s/-$//' | cut -c1-40)

# 2. Create the rally folder
RALLY_DIR=".orbit/specs/$(date -I)-${SLUG}-rally"
mkdir -p "$RALLY_DIR"

# 3. Promote each card to a spec
declare -a CHILDREN_YAML
for CARD in "${APPROVED_CARDS[@]}"; do
  CARD_SLUG=$(basename "$CARD" .yaml)
  SPEC_ID=$(plugins/orb/scripts/promote.sh "$CARD")
  CHILDREN_YAML+=("- card_path: $CARD")
  CHILDREN_YAML+=("  spec_id: $SPEC_ID")
  CHILDREN_YAML+=("  branch: rally/${CARD_SLUG#*-}")
  CHILDREN_YAML+=("  spec_dir: .orbit/specs/$SPEC_ID")
  CHILDREN_YAML+=("  card_phase: proposed")
  CHILDREN_YAML+=("  dep_predecessors: []")
  CHILDREN_YAML+=("  worktree: null")
  CHILDREN_YAML+=("  rally_dir: $RALLY_DIR")
done

# 4. Write rally.yaml
cat > "$RALLY_DIR/rally.yaml" <<EOF
goal: <goal string>
autonomy: <guided|supervised>
phase: approved
started: $(date -Iseconds)
completed: null
folder: $RALLY_DIR
children:
$(printf '%s\n' "${CHILDREN_YAML[@]}")
EOF
```

The `phase` field advances through: `approved` → `designing` →
`design-review` → `implementing` → `complete`. Each transition is a
single `rally.yaml.phase` rewrite (the lead reads, mutates, writes).

`rally.yaml.children` lists every card-spec in the rally. The
**claimable set** is derived from this array:

> A child is **claimable** when its `card_phase` is `proposed` or
> `designed` (depending on the rally phase) AND every spec id in
> `dep_predecessors` belongs to a child whose `card_phase` is
> `complete` or `parked`.

The lead computes this set on every queue tick — there is no separate
`bd ready` query.

## Stage 2: Decision packs — queued design

This is the rally's central innovation. The goal: present the author
with executive-ready decisions in a single consolidated gate, with
options + trade-offs + recommendations drawn from the best available
evidence — not raw questions they lack context to answer.

> **Principle:** The goal of rally is to have the highest quality
> interactions at ideation and assertion. This means maximum clarity
> based on the best available evidence. Agent work between gates
> exists to make the next gate sharper — not just faster.

### 2a. Set the phase

```bash
# Update rally.yaml: phase: designing
# Update each child: card_phase: designing
```

### 2b. Launch N design sub-agents in parallel

Design sub-agents write to the main checkout. Using the Agent tool,
one call per card, all in the same message for parallelism. Each
sub-agent receives a self-contained brief:

```
You are a design analyst for card <card_path> (spec <child-spec-id>).
Produce a decision pack.

Read your spec via `orbit --json spec show <child-spec-id>` to confirm
the card linkage, then read your spec_dir from rally.yaml children
entry. (Convention: <spec_dir> = .orbit/specs/<child-spec-id>.)

Your job:
1. Read the card (<card_path>) and its references
2. Read prior specs in the card's `specs` array (if any)
3. Run a keyword scan on the codebase using terms from the card's goal and scenarios
4. Identify the 4–6 design decisions that this card's implementation requires

For each decision, produce:
  - Title (one line, describes the choice)
  - Context (1–2 sentences — why this decision exists)
  - Options (2–3 concrete alternatives)
  - Trade-offs (what each option gains, what it loses — grounded in evidence from the card, prior specs, or codebase)
  - Recommendation (which option and why, citing the evidence)

Do NOT run interactive Q&A. Do NOT call AskUserQuestion. You produce a written decision pack that the lead agent will present to the author.

Write your decision pack to: <spec_dir>/decisions.md
Do NOT write outside <spec_dir>.

Do NOT read or write any rally-coordination state. The lead owns rally.yaml exclusively.

When done, return a JSON object with this shape (and nothing else):
  { "files": ["<spec_dir>/decisions.md", ...any other paths you wrote...] }
```

**Path discipline is trust + post-verify.** The brief names the target
directory as a convention the sub-agent is expected to honour. The
lead verifies on return via three primitives (§2c). Claude Code does
not provide a tool-level path prefix guard, so the lead takes
responsibility for the check; the brief takes responsibility for the
contract.

### 2c. Verify on return — three primitives (snapshot-diff discipline)

Before launching each design sub-agent, the lead captures a
**pre-snapshot** of the main checkout:

```bash
git status --porcelain > /tmp/rally-pre-<card-slug>.snap
```

After the sub-agent returns, the lead runs all three checks:

1. **Self-report (contract):** parse the sub-agent's returned JSON
   `files` list. If the JSON is missing or malformed, reject.
2. **Artefact assertion (completeness):** assert
   `<spec_dir>/decisions.md` exists; assert every path in the returned
   list is under `<spec_dir>`.
3. **Snapshot diff (independent verification):** capture a
   post-snapshot (`git status --porcelain`) and compute the set
   difference `post \ pre`. Any entry in that difference that is not
   under `<spec_dir>` rejects the sub-agent's output. There is no
   lead-owned allowlist beyond the spec dir — rally state lives in
   `rally.yaml`, which the lead alone touches. Entries present in both
   pre and post are pre-existing lead-side state and are ignored.

On the **first** violation: re-brief the same sub-agent with an
explicit path warning naming the offending entry (e.g. `your previous
return created 'plugins/orb/scratch.md' outside <spec_dir>; do not
write outside <spec_dir>`). This re-brief is a **pre-qualification
retry** — NOT a rally-level strike.

On the **second** violation for the same card: park via the
`rally.yaml.children[].card_phase=parked` + `orbit spec note + orbit
spec close` flow with `reason_label: tool_surface_incomplete` (see
§NO-GO Handling). The rally continues with the remainder.

### 2d. Wait for all sub-agents to return

For each non-parked child, update `rally.yaml`:

```yaml
# children[i]: card_phase: designed
```

Once all packs are in (or parked), advance the rally:

```yaml
# rally.yaml: phase: design-review
```

## Stage 3: Consolidated decision gate

Read all decision packs. Present them to the author **grouped by
card**, in a single consolidated response:

```
## Consolidated Decision Gate — <N> cards

### Card: <card feature> (<card_path>)
Spec: <child-spec-id>

#### Decision 1: <title>
Context: <context>
Options:
  A. <option>
  B. <option>
  C. <option>
Trade-offs:
  A: gains <x>, loses <y>
  B: gains <x>, loses <y>
  C: gains <x>, loses <y>
Recommendation: B — <rationale>

#### Decision 2: <title>
...

### Card: <next card>
...
```

**Use AskUserQuestion per card** to capture approvals and overrides.
For each card, the author either accepts the recommendations
wholesale or names specific overrides (e.g. "card 2 decision 3:
option A instead of C"). Record every override explicitly — these
flow into the interview.

### Interview production

Re-launch each sub-agent (or have the lead agent write directly —
whichever is cheaper) with its decisions + the author's
approvals/overrides. The sub-agent writes `<spec_dir>/interview.md`
following the design skill's interview record format, reflecting the
approved decisions.

Re-launched sub-agents are subject to the same three-primitive
verification as §2c (first violation → re-brief retry; second
violation → park).

When all interviews exist, the rally moves on to Stage 4.

## Stage 4: Consolidated design review + disjointness wiring

Once all interviews exist, present them to the author in a single
session:

```
## Consolidated Design Review — <N> cards

1. Card: <name> — <spec_dir>/interview.md (spec: <child-spec-id>)
   Goal: <goal>
   Key decisions: <one-liners>

2. Card: <name> — <spec_dir>/interview.md (spec: <child-spec-id>)
   Goal: <goal>
   Key decisions: <one-liners>
```

### Run the definitive disjointness check

Extract from each interview:
- Files named in the design (e.g. `plugins/orb/skills/foo/SKILL.md`)
- Symbols named (types, traits, functions, schemas)
- Shared references (skills, scripts, hooks)

Compute the intersection. Any non-empty intersection is a hard input
to implementation ordering — **it gates, not advises.**

### If shared symbols are found — wire dep_predecessors

```
Shared symbols detected:
  - Engine trait — referenced by <card A> and <card C>
  - .orbit/specs/.../hook.sh — both <card B> and <card C> modify it

Proposed implementation order: <card A> → <card C> → <card B>
Rationale: Card A establishes the Engine trait; card C extends it; card B depends on the hook update card C ships.
```

Use AskUserQuestion to confirm or modify the order. On confirm, encode
the order via `rally.yaml.children[].dep_predecessors` — for each
ordered pair `(earlier, later)`:

```yaml
# children[<later-index>].dep_predecessors: [<earlier-spec-id>]
```

The claimable-set rule (§Stage 1 "Create the rally") will then surface
only the head of the chain at any moment, releasing the next card as
each predecessor closes.

### If no shared symbols are found — leave parallel

```
No shared symbols detected — parallel implementation is safe.
Rationale: Each design names disjoint files and types.
```

No `dep_predecessors` entries. Every child whose `card_phase` is
`designed` is claimable simultaneously, ready for parallel claim.

### Advance to implementation

```yaml
# rally.yaml: phase: implementing
```

**Supervised mode gate:** If autonomy is `supervised`, pause for
greenlight before proceeding to implementation.

## Stage 5: Implementation

The implementation queue is the **claimable set** computed from
`rally.yaml.children` (§Stage 1 rule). The shape of the queue
(single-head chain vs flat fan-out) is determined entirely by the
`dep_predecessors` arrays from §Stage 4. Rally does not maintain a
parallel queue structure — the children array IS the queue.

### 5a. Commit interviews to rally branches

For each non-parked child, commit `<spec_dir>/interview.md` to the
card's rally branch as a clean first commit. This is git hygiene —
the rally branch tells the card's story in chronological order — and
is independent of how `/orb:drive` resumes.

```bash
# on main
git checkout -b rally/<slug>            # or: git checkout rally/<slug>
git add <spec_dir>/interview.md
git commit -m "rally/<slug>: approved design"
git checkout main
```

### 5b. Serial implementation (chain wired by dep_predecessors)

When dep_predecessors arrays are non-empty, the claimable set returns
exactly one card at a time. The lead loops:

- Compute the claimable set per the §Stage 1 rule (children whose
  `card_phase` is `designed` or `implementing` AND every entry in
  `dep_predecessors` belongs to a child whose `card_phase` is
  `complete` or `parked`).
- Take the first claimable child as `NEXT`. If the set is empty, the
  serial pass is done.
- Update `rally.yaml`: `children[NEXT].card_phase=implementing,
  worktree=main`.
- Resolve the branch with
  `BRANCH=$(yq -r ".children[] | select(.spec_id == \"$NEXT\") | .branch" "$RALLY_DIR/rally.yaml")`.
- Check out the branch, run `/orb:drive <NEXT>` (which resumes from
  drive.yaml's stage and closes the spec on APPROVE), then return to
  main.
- Repeat. The next card is released by the claimable-set rule only
  when its predecessor closes (`card_phase` becomes `complete` or
  `parked`).

Each serial card runs **drive-full against the rally branch in the
main checkout** — rally-level autonomy does not reduce drive's
internal autonomy.

### 5c. Parallel implementation (no dep_predecessors)

When no `dep_predecessors` arrays are populated, the claimable set
returns all designed cards. Launch N implementation sub-agents
concurrently, each in its own git worktree.

For each card:

```bash
SLUG=$(yq -r ".children[] | select(.spec_id == \"$CHILD_SPEC\") | .branch" "$RALLY_DIR/rally.yaml" | awk -F/ '{print $NF}')
WORKTREE_PATH="$(realpath ..)/$(basename "$(pwd)")-rally-$SLUG"
git worktree add "$WORKTREE_PATH" "rally/$SLUG"
# Update rally.yaml: children[CHILD_SPEC].card_phase=implementing, worktree=$WORKTREE_PATH
```

Then, in a single message, spawn all N sub-agents via the Agent tool
with `run_in_background: true`:

```
# Sub-agent brief (parallel implementation)

You are an implementation agent for spec <child-spec-id>. Your working
directory is <worktree path>. Run `/orb:drive <child-spec-id>` inside
that worktree. Drive will:
  1. Resume from .orbit/specs/<child-spec-id>/drive.yaml stage (or initialise it if absent)
  2. Run review-spec → implement → review-pr internally as forked Agents
  3. Close the spec via orbit spec close on APPROVE
  4. Or escalate if iteration / review budgets exhaust

Do NOT read or write rally.yaml. The lead owns rally.yaml exclusively.
You may read your own spec via `orbit --json spec show <child-spec-id>`
and update your own .orbit/specs/<child-spec-id>/drive.yaml as drive
normally does.

When drive completes (APPROVE at review-pr), return a JSON object:
  { "verdict": "complete", "pr": "<pr-number-or-url>", "spec_dir": "<spec_dir>" }

If drive escalates, return:
  { "verdict": "parked", "reason_label": "<label>", "reason": "<one-line>",
    "spec_dir": "<spec_dir>" }

where `reason_label` is one of the six fixed tokens (see §NO-GO):
  budget | recurring_failure | contradicted_hypothesis | diminishing_signal | review_converged | tool_surface_incomplete

Do not attempt rally-level retries — your internal drive iterations are the
strike.
```

The Agent tool is invoked with `run_in_background: true` and
`subagent_type: "general-purpose"`; every call is in the same message
so the harness dispatches all N in parallel.

**Recursive context separation.** Each parallel sub-agent runs
`/orb:drive` inside its worktree. Drive's review-spec and review-pr
stages themselves run as nested forked Agents — the same context-
separation pattern drive uses at its top level. Rally does not invoke
reviewers directly; drive does, once per stage per cycle.

**Parallel completion handling — Agent-return await (no polling, no
sentinels).** The lead awaits each sub-agent's completion via the
Agent tool's built-in background-completion notification. The harness
surfaces the sub-agent's final message as the lead's next turn event
— no `sleep`, no polling loop, no `Monitor` call, no sentinel file.

On each completion:

1. Parse the sub-agent's JSON verdict.
2. On `complete`: the child spec is already closed by drive. Update
   `rally.yaml.children[CHILD_SPEC].card_phase=complete` (idempotent
   — no-op if already set).
3. On `parked`: handle per §NO-GO Handling.

### 5d. Mid-flight parallel→serial conversion

If parallel implementation surfaces a shared-symbol contention
mid-flight (e.g. two in-progress sub-agents about to touch the same
file), the lead serializes by adding a single dep_predecessors entry:

```yaml
# children[<later-index>].dep_predecessors: [<earlier-spec-id>]
```

The claimable-set rule will then withhold `<later>` from new claim
until `<earlier>` closes. **In-progress work continues.** The runtime
change is the queue, not the running cards. No sub-agent restart, no
rally-coord mutation beyond the single dep edge — the rally.yaml edit
is the operation.

If a sub-agent has not yet started (still queued by the harness),
adding the edge prevents it from starting. If the sub-agent has
already started, the lead may either (a) let both finish if the
contention is mild, or (b) ask the later sub-agent to halt and
re-claim once its predecessor closes (sub-agent honours the request
via its own halt path).

## NO-GO Handling — single-strike park

A NO-GO verdict at **any** stage (drive's spec review BLOCK,
supervised gate NO-GO, drive's PR review BLOCK, or drive-full
escalation from a parallel sub-agent) parks the card immediately.
**No iteration retries within the rally.** Rally is about throughput;
retrying one card while others wait defeats the purpose.

Drive escalations from a sub-agent surface as the parked-verdict JSON
defined in the §5c sub-agent brief (`{verdict, reason_label, reason,
spec_dir}`). Rally lead converts that into:

```bash
orbit spec note <child-spec-id> "PARKED: [<reason_label>] <reason>"
orbit spec close <child-spec-id>
# Update rally.yaml: children[CHILD_SPEC].card_phase=parked, park_reason="PARKED: [<reason_label>] <reason>"
```

The reason_label vocabulary is preserved (six fixed tokens):

```
Drive escalation trigger                     reason_label
---------------------------------------------+----------------------
Budget exhausted (3 NO-GO iterations)        budget
Recurring failure mode                       recurring_failure
Contradicted hypothesis                      contradicted_hypothesis
Diminishing signal                           diminishing_signal
Synthetic BLOCK after 3× REQUEST_CHANGES     review_converged
Agent tool unavailable for cold-fork         tool_surface_incomplete
```

An unrecognised or missing `reason_label` in the sub-agent's JSON
return parks the card with the literal string `[unknown]` prefixed —
the card is still parked, and the label drift becomes visible in the
spec note for later investigation.

Rally does not retry at its level; the sub-agent's internal iterations
(drive's 3-iteration NO-GO budget plus each stage's 3-cycle
REQUEST_CHANGES budget) are the strike.

The parked card can be driven individually later with `/orb:drive
<card_path>`, where its full 3-iteration budget applies (a fresh
drive starts a new spec chain). The rally continues with remaining
cards.

## Stage 6: Assurance — PR strategy

### Stacked PRs (serial — dep_predecessors exist)

Each card's PR targets the previous non-parked card's branch:

```
main
 └── rally/card-a         [PR #101 → main]
      └── rally/card-c    [PR #102 → rally/card-a]
           └── rally/card-b   [PR #103 → rally/card-c]
```

**If a middle card is parked**, subsequent PRs target the **last
non-parked** card's branch. E.g. if card C is parked in the stack
above, card B's PR targets `rally/card-a`, not `rally/card-c`.

The lead computes "last non-parked predecessor" by walking the
`dep_predecessors` chain and skipping any child whose `card_phase` is
`parked`.

Present the stack to the author bottom-up for review.

### Batched diff review (parallel — no dep_predecessors)

Each sub-agent creates an individual PR against main. The lead
presents them together:

```
## Rally PR Review — <N> PRs ready

PR #201 — <card A feature>
  Spec: <spec_dir>/spec.yaml (<N> ACs)
  Files changed: <count>
  Review verdict: APPROVE — <one-line honest assessment>

PR #202 — <card B feature>
  ...
```

Author reviews in a single session.

## Stage 7: Completion

When every entry in `rally.yaml.children` has `card_phase` either
`complete` or `parked`:

1. **Write completion summary:**

   ```
   ## Rally Complete — <goal string>

   Duration: <started> → <now>
   Autonomy: <autonomy>

   Completed: <N> card(s)
     - <card feature> — PR #<n>
     - <card feature> — PR #<n>

   Parked: <N> card(s)
     - <card feature> — park_reason: PARKED: [<label>] <reason>

   Implementation order: <serial chain order OR "parallel">
   Rationale: <derived from dep_predecessors graph, or "no shared symbols" for parallel>

   PRs:
     - #<n>: <title> (<target branch>)
     - #<n>: <title> (<target branch>)
   ```

2. **Mark rally complete:**

   ```yaml
   # rally.yaml: phase: complete
   # rally.yaml: completed: $(date -Iseconds)
   ```

3. **No archival step.** The rally folder (`<rally-folder>`) stays
   where it is — its `rally.yaml`, decisions.md, and interview.md
   files remain on disk as the design + orchestration record. When
   the next rally begins, it creates its own folder alongside this
   one.

## Critical Rules

These are invariants — not duplicates of the body. The body describes
what to do at each step; these rules describe what must always hold,
and what must never happen, regardless of where in the rally you are.

- **One active rally at a time.** Resolution refuses a fresh rally if
  an in-progress rally folder exists. Children-graph orchestration
  loses meaning if two rallies overlap on cards.
- **rally.yaml is the single source of orchestration state.** No
  rally-coordination data lives outside it. **Sub-agents never read
  or write rally.yaml** — they read their own spec and update their
  own drive.yaml. The lead alone touches rally.yaml.
- **Sub-agent path discipline is trust + post-verify.** Claude Code
  does not provide tool-level path enforcement; the brief names the
  target directory as a contract, the lead verifies on return via the
  three primitives (§2c). This skill does not claim path enforcement
  it does not have.
- **Drive autonomy inside a rally is always full.** Rally-level
  autonomy (guided | supervised) governs rally-phase pauses only —
  parallel sub-agents in worktrees and serial cards in main both run
  drive in full mode.
- **Single-strike NO-GO.** A card that fails any review is parked
  immediately. No retries within the rally. The sub-agent's internal
  drive iterations (3 NO-GO budget × 3 REQUEST_CHANGES per stage) are
  the strike. A parked card can be driven individually later with
  `/orb:drive <card_path>`.

## Resumption

When `/orb:rally` is invoked with a rally folder (or detects an
in-progress rally per §Input contract):

1. **Read rally.yaml:** `<rally-folder>/rally.yaml`. Extract:
   - `phase`
   - `autonomy`
   - `started`, `completed`
   - `folder`
   - `children[]`

2. **For each child:** the spec id, card_path, branch, spec_dir,
   card_phase, dep_predecessors, worktree are all in the children
   array. Cross-reference each child's spec status via `orbit spec
   show <child-spec-id>` if a freshness check is needed (the spec
   may have been closed since rally.yaml's last write).

3. **Resume at the named phase.** rally.yaml is the source of truth —
   there is no separate state to scan or reconcile.

   | phase           | Resume at                                           |
   |-----------------|-----------------------------------------------------|
   | `approved`      | §Stage 2 (decision packs not yet launched)          |
   | `designing`     | §Stage 2 (some decision packs may be returned)      |
   | `design-review` | §Stage 3 (decision gate / interview production)     |
   | `implementing`  | §Stage 5 (queue replay via claimable-set rule)      |
   | `complete`      | Already done — report status                        |

4. **For implementing-phase resume:** the lead does not reconstruct
   per-card sub-stage from any side file — each implementing child
   resumes via `/orb:drive <child-spec-id>` which itself reads
   `.orbit/specs/<child-spec-id>/drive.yaml`'s `stage` field. Rally's
   job at resume is only to (a) re-launch sub-agents for any child
   whose `card_phase=implementing` and whose spec is still open,
   (b) honour the claimable-set rule for serial flows, and (c) await
   completions for any sub-agent that was running before the session
   died.

5. **Announce the resumption** in one block: rally goal, folder,
   phase, child counts (proposed / complete / parked / in progress),
   and per-child resume points (spec id, worktree, drive stage if
   implementing).

---

**Next step:** After completion, review all PRs in the order
recommended by the assurance strategy (stacked bottom-up or batched
together).
