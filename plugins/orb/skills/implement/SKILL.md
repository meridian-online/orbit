---
name: implement
description: Pre-flight check + work loop for an orbit-state spec — surface ACs, enforce gates, escalate detours as sub-tasks, close on completion
---

# /orb:implement

Drive a single spec from claim to close. This skill is the agent's working
contract for the duration of an in-flight spec: pre-flight context, AC
tracking, gate enforcement, detour escalation, and close-out are all
expressed as concrete `orbit` and `orbit-acceptance.sh` commands.

## Why This Exists

Without a pre-flight check, implementing agents treat the codebase as
ground truth and miss spec-prescribed patterns. This skill forces the
spec's acceptance criteria into working memory before implementation
begins, and keeps the spec's `acceptance_criteria` field as the single
source of truth throughout.

> **Reference incident:** an agent treated the codebase as ground truth
> and missed a spec-prescribed entrypoint pattern.

## Migration Note

Earlier `/orb:implement` revisions mirrored ACs into a `progress.md`
file, emitted Claude Code tasks via `TaskCreate`, computed a `Spec hash`
for drift detection, and ran a cancel-then-recreate "resume reconcile"
on session resume. All of that is now subsumed by the orbit-state
substrate:

- AC list and status — spec `acceptance_criteria` field (structured
  records with `id`, `description`, `gate`, `checked`).
- Live AC visibility — `orbit spec show <id>` and `orbit spec list`.
- Drift detection — removed; the acceptance field IS the contract.
- Resume — `orbit spec show <id>` is the canonical refresh.
- Detours — sub-tasks via `orbit task open --spec-id <spec> --body
  "<detour title>"` with the body capturing the discovered-from
  provenance.

The mechanisms motivated by cards 0003 (implement-session-visibility)
and 0009 (mission-resilience) are now provided by the substrate; those
cards are referenced as historical pointers, not as live contracts in
this skill.

## Usage

```
/orb:implement [spec-id]
```

If `spec-id` is omitted, the skill resolves the active spec
automatically (see "Input contract" below).

## Input contract

The skill operates on exactly one open spec per session. Resolution
proceeds in three branches:

1. **Argument provided** — `/orb:implement <spec-id>`. Use it directly.
   The skill calls `orbit spec show <spec-id>` to validate; if the spec
   does not exist, the call returns `spec.show: not-found: ...` and the
   skill surfaces that error.

2. **No argument** — query for open specs:

   ```bash
   orbit --json spec list --status open
   ```

   - **Single match** → use it.
   - **Zero matches** → halt and instruct the agent: "No open spec.
     Create one with `/orb:spec` or pick an existing spec with `orbit
     spec list`."
   - **Multiple matches** → halt and instruct the agent to pass the
     spec ID explicitly, listing the candidates.

This skill does not attempt to manage concurrent in-progress specs —
multi-claim is out of scope. If the agent is juggling multiple specs,
it must invoke this skill once per spec with the explicit ID.

## Pre-flight

Before any code is written, the agent runs the following sequence:

1. **Read the spec.**

   ```bash
   orbit spec show <spec-id>
   ```

   The agent reads the `goal` verbatim (this carries the goal and any
   constraints written into the spec). Constraints are prose embedded
   in the goal or in linked card files — the agent applies them; there
   is no parser.

2. **Enumerate ACs.**

   ```bash
   plugins/orb/scripts/orbit-acceptance.sh acs <spec-id>
   ```

   This emits one tab-separated tuple per AC:
   `<ac-id>\t<status>\t<description>\t<is_gate>`. The agent surfaces
   this list in its response so the human-visible context contains the
   full AC roster.

3. **Identify the next AC.**

   ```bash
   plugins/orb/scripts/orbit-acceptance.sh next-ac <spec-id>
   ```

   This emits `<ac-id>\t<is_gate>` — the first unchecked AC that is not
   blocked by an unchecked gate. If output is empty, all ACs are
   checked and the agent jumps to "Completion" below.

4. **Run a keyword scan.** Search the project source for keywords drawn
   from the spec's `goal` and AC descriptions before writing code (see
   `/orb:keyword-scan`). This surfaces existing patterns the work
   should build on rather than reinvent.

No checklist file is written. The spec's `acceptance_criteria` field is
authoritative and remains so for the rest of the session.

## Implement loop

For each AC, in `next-ac` order:

1. **Confirm the next AC** with `orbit-acceptance.sh next-ac <spec-id>`.
   This is the authoritative gate — if a gate AC is unchecked,
   `next-ac` will return that gate; the agent must complete it first.

2. **Implement the AC.** Write code, edit deliverables, run tests as
   needed.

3. **Mark the AC done.**

   ```bash
   plugins/orb/scripts/orbit-acceptance.sh check <spec-id> <ac-id>
   ```

   This calls `orbit spec update --ac-check <ac-id>` internally,
   flipping the AC's `checked` flag from false to true through the
   canonical writer. The spec's `acceptance_criteria` field is the only
   place AC status lives.

4. **Loop** — re-run `next-ac` and continue.

### Gate enforcement

Gate enforcement is delegated entirely to `orbit-acceptance.sh next-ac`.
By convention (see `.orbit/conventions/acceptance-field.md`), an
unchecked AC with `gate: true` blocks all subsequent ACs by declaration
order. The parser implements this — the agent does not re-check gates
inline.

If the agent suspects a gate is blocking, it can confirm with:

```bash
plugins/orb/scripts/orbit-acceptance.sh blocking-gate <spec-id>
```

This emits the first unchecked gate's `<ac-id>\t<description>` (or
nothing if no gate is blocking).

### Working rules during implementation

- **Spec over codebase.** If the spec's goal or an AC prescribes a
  pattern the codebase doesn't have, implement what the spec says. Do
  not work around missing structure — create what the spec requires.
- **Surface unspecced decisions.** When you encounter a choice not
  covered by the spec with meaningful consequences, **stop and ask**
  via `AskUserQuestion`. Present 2–3 options with trade-offs.
- **Constraints in the spec are non-negotiable.** If you find yourself
  about to violate one, stop and flag it. Either the constraint needs
  updating (re-scope the spec) or the approach needs changing.
- **Assumption reversals require escalation.** When implementation
  evidence (tests, benchmarks, real data) contradicts a stated
  assumption, stop. Document the finding (use `orbit memory remember`
  for durable persistence), name the invalidated assumption, and
  checkpoint before proceeding.
- **Derive from evidence.** When prior research, phase results, or
  benchmarks answer a parameter question, use that data. Only escalate
  when evidence is genuinely silent or contradictory.

## Detour escalation

When unplanned work blocks the current AC — a missing dependency, a bug
in a foreign module, a malformed dataset — the agent escalates the
detour as a **sub-task** under the current spec. The task body captures
the discovered-from provenance:

```bash
orbit task open \
  --spec-id <current-spec-id> \
  --body "detour: <short title> (discovered while implementing <current-ac-id>; blocks the parent AC because <reason>)"
```

`orbit task open` returns the task id. Then claim and work it:

```bash
orbit task claim <task-id>
# ... resolve the detour ...
orbit task done <task-id>
```

Resume the parent spec:

```bash
orbit spec show <current-spec-id>
plugins/orb/scripts/orbit-acceptance.sh next-ac <current-spec-id>
```

`orbit spec show <spec>` reloads the spec context; `next-ac` returns
the AC the agent was working on. AC state lives in the spec; task state
lives in the spec's task event stream — the substrate is the audit
trail.

## Forward findings — three channels

During implementation the agent will discover work that does not belong
in the current spec. The skill routes findings into one of three
channels:

| Finding kind                                  | Channel                                                                    |
|-----------------------------------------------|----------------------------------------------------------------------------|
| Blocking detour (must resolve before this AC) | sub-task under current spec via `orbit task open --spec-id <current>` (above) |
| Follow-up work that does NOT block this AC    | new spec via `/orb:spec`, or a memory note via `orbit memory remember <key> "<finding>"` |
| Product-direction question (capability-level) | memo at `.orbit/memos/YYYY-MM-DD-<slug>.md` for `/orb:distill` later |

**Never suggest "open a follow-up card."** Cards describe capabilities,
not work items. The agent doesn't know whether a finding warrants a new
capability or feeds an existing card's next spec — that's the author's
call during distill. The memo channel preserves the finding without
forcing a structural decision.

## Test execution

Long-running test invocations — expected duration over **60 seconds**, or
full-suite runs (e.g. `cargo test` without a filter, `pytest` at the
repo root, `npm test`) — **MUST** be launched via the `Monitor` tool
with the command piped through a line-buffered failure-marker filter.
The canonical filter is:

```
grep --line-buffered -E 'FAIL|ERROR|AssertionError|Traceback'
```

Short targeted tests (under 60 seconds, a named subset or single test)
continue to use the `Bash` tool as before. **Unfiltered Monitor on a
test suite is forbidden** — every stdout line becoming a notification
swamps the agent. The `grep --line-buffered` wrapper ensures only
failure markers surface as streamed events while the suite runs to
completion.

### First-failure checkpoint

On the **first streamed** line from `Monitor` that matches the
failure-marker regex `FAIL|ERROR|AssertionError|Traceback`, the agent's
behaviour branches by interactivity.

**Interactive path** — stdin is a TTY AND `ORBIT_NONINTERACTIVE` is unset
or not equal to `1`:

The agent MUST pause mid-run, acknowledge the failure inline, and call
`AskUserQuestion` with exactly two options:

- `Fix the failure now (I will investigate and re-run)`
- `Let the suite finish, then triage`

Subsequent failure lines in the same `Monitor` run are surfaced but do
NOT re-prompt. The "first" semantics is per-Monitor-invocation: a new
test run resets the gate.

**Non-interactive path** — no TTY on stdin OR `ORBIT_NONINTERACTIVE=1`
(this is `/orb:drive`, rally, cron, CI):

The agent MUST NOT call `AskUserQuestion`. On the first matching failure
line, emit the canonical non-interactive marker string below to stderr,
stop consuming further `Monitor` output, and halt with **exit status 2**.
The upstream orchestrator (drive) uses the exit-2 convention to route to
a checkpoint distinct from a clean test-suite failure (exit 1).

**Canonical non-interactive first-failure marker (single source of truth):**

> **FIRST_FAILURE_NONINTERACTIVE_MARKER** = `orbit: first-failure checkpoint skipped (non-interactive); halting for upstream triage`

This exact string is emitted verbatim. Test fixtures grep this file for
the constant and assert the emitted marker matches byte-for-byte.

## Completion

When all ACs are checked the spec is ready to close.

```bash
plugins/orb/scripts/orbit-acceptance.sh has-unchecked <spec-id>
```

Exit status 1 means no ACs remain unchecked — the spec is done. The
agent then runs:

```bash
orbit spec close <spec-id>
```

`spec.close` transactionally appends the spec's path to every linked
card's `specs` array. It rejects if the spec has any open child tasks —
those must be done or cancelled first (see ac-06 contract).

After close, the agent suggests the next step:

> Run `/orb:review-pr` to verify the implementation.

## NO-GO outcome

Not every spec ships code. Some produce evidence that an approach
doesn't work — that's a valid outcome, not a failure. When results
invalidate the spec's hypothesis:

1. **Persist the finding.** Use `orbit memory remember` so the insight
   survives the spec close. The CLI takes a key and a body as separate
   positional args — the key is a short stable identifier (re-using it
   upserts):

   ```bash
   orbit memory remember <spec-id>-no-go "<evidence-backed insight>"
   ```

2. **Close with a NO-GO note.** orbit-state's `spec close` has no
   `--reason` flag (the close is the close — the audit trail lives in
   the spec note stream and memory). Append a final note before closing:

   ```bash
   orbit spec note <spec-id> "NO-GO: <one-line evidence summary>"
   orbit spec close <spec-id>
   ```

3. **Card direction-layer updates remain the author's call.** Cards
   describe capabilities and are never closed; if the NO-GO requires
   the card's `goal` to narrow or shift, that's a `/orb:distill` task
   for the author, not an automatic skill action.

## Worked example

This is a copy-pasteable command trace using a hypothetical spec
`2026-05-08-foo`.

```bash
# 1. Find the spec and confirm it exists
orbit spec list --status open                   # see what's open
orbit spec show 2026-05-08-foo                  # read goal + ACs

# 2. Pre-flight
plugins/orb/scripts/orbit-acceptance.sh acs 2026-05-08-foo
plugins/orb/scripts/orbit-acceptance.sh next-ac 2026-05-08-foo
# → ac-01    1   (gate, unchecked, must close first)

# 3. Implement loop — close each AC
# (work the AC, then mark it done)
plugins/orb/scripts/orbit-acceptance.sh check 2026-05-08-foo ac-01
plugins/orb/scripts/orbit-acceptance.sh next-ac 2026-05-08-foo
# → ac-02    0   (now startable — ac-01 gate cleared)

# ... repeat for ac-02, ac-03, ... ac-09 ...

# 4. (If a detour is discovered mid-AC)
orbit task open \
  --spec-id 2026-05-08-foo \
  --body "detour: fix flaky orbit-acceptance.sh test under bash 3.2 \
(surfaced while implementing ac-04; check fails silently under macOS default bash 3.2)"
# → orbit task open returns task id, e.g. 0001
orbit task claim 0001
# ... fix it ...
orbit task done 0001
orbit spec show 2026-05-08-foo
plugins/orb/scripts/orbit-acceptance.sh next-ac 2026-05-08-foo

# 5. Completion
plugins/orb/scripts/orbit-acceptance.sh has-unchecked 2026-05-08-foo
# (exit 1 → all done)
orbit spec close 2026-05-08-foo
```

## Integration with other skills

- **`/orb:review-pr`** reads the spec's `acceptance_criteria` field
  (via `orbit-acceptance.sh acs`) to cross-reference AC coverage
  against the implementation diff. The cold-fork review architecture
  (decision 0011 D2) is preserved — the reviewer reads the spec, not a
  legacy file.
- **`/orb:review-spec`** runs against the spec's structured ACs at
  design time; once a spec is open, this skill takes over.
- **`/orb:drive`** invokes `/orb:implement` as one stage of an
  end-to-end pipeline; in non-interactive contexts the
  `FIRST_FAILURE_NONINTERACTIVE_MARKER` exit-2 convention is the
  handoff.

---

**Next step:** after `orbit spec close`, run `/orb:review-pr` to verify
the implementation against the spec's acceptance criteria.
