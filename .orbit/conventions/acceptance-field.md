# Acceptance criteria convention

Orbit specs store acceptance criteria as a structured field on the spec YAML — `acceptance_criteria`. This convention defines the field shape, the gate-enforcement semantics, and the helpers that read it.

## Field shape

`acceptance_criteria` is a list of records on the spec (`.orbit/specs/<spec-id>.yaml`):

```yaml
acceptance_criteria:
- id: ac-01
  description: Decide hash algorithm before implementing drift detection
  gate: true
  checked: false
- id: ac-02
  description: Implement sha256 drift check in pre-AC sequence
  gate: false
  checked: false
```

- **`id`** — sequential identifier starting at `ac-01`. Zero-padded to two digits. Stable for the lifetime of the spec; never renumbered.
- **`description`** — the AC text. Plain prose; no special syntax.
- **`gate`** — boolean. A gate AC blocks all subsequent ACs by declaration order until checked.
- **`checked`** — boolean. Flipped to `true` via `orbit spec update --ac-check <id>` (or the `orbit-acceptance.sh check` wrapper).

The structured field replaces the bd-era markdown-line format. Helpers read it via `orbit --json spec show <spec-id>` (full spec) or `plugins/orb/scripts/orbit-acceptance.sh acs <spec-id>` (tab-separated tuples per AC).

## Gate enforcement rules

- A gate AC blocks all subsequent ACs by declaration order, regardless of whether those subsequent ACs are themselves gates.
- Non-gate ACs do not block each other.
- An unchecked gate means: the agent must not start any AC declared after it.
- A checked gate releases all subsequent ACs until the next unchecked gate.
- Multiple consecutive gates are valid — each must be checked in order.

`orbit-acceptance.sh next-ac <spec-id>` returns the first unchecked AC that is not blocked by an unchecked gate. The implement skill (and `/orb:drive` Stage 2) defers to this helper rather than re-checking gates inline.

## Worked examples

### Example 1: Spec with gates

```yaml
acceptance_criteria:
- id: ac-01
  description: Decide hash algorithm before implementing drift detection
  gate: true
  checked: false
- id: ac-02
  description: Implement sha256 drift check in pre-AC sequence
  gate: false
  checked: false
- id: ac-03
  description: Write resume drift notice
  gate: false
  checked: false
- id: ac-04
  description: Confirm schema ownership before extending acceptance shape
  gate: true
  checked: false
- id: ac-05
  description: Add gate enforcement to implement skill
  gate: false
  checked: false
```

| AC    | Gate | Status    | Blocked by |
|-------|------|-----------|------------|
| ac-01 | yes  | unchecked | —          |
| ac-02 | no   | unchecked | ac-01      |
| ac-03 | no   | unchecked | ac-01      |
| ac-04 | yes  | unchecked | ac-01      |
| ac-05 | no   | unchecked | ac-01      |

**Next AC:** ac-01 (first unchecked; it's a gate so nothing after it can start).

After checking ac-01:

| AC    | Gate | Status    | Blocked by |
|-------|------|-----------|------------|
| ac-01 | yes  | checked   | —          |
| ac-02 | no   | unchecked | —          |
| ac-03 | no   | unchecked | —          |
| ac-04 | yes  | unchecked | —          |
| ac-05 | no   | unchecked | ac-04      |

**Next AC:** ac-02 (first unchecked, not blocked — ac-04 is also available but ac-02 comes first).

### Example 2: Spec without gates

```yaml
acceptance_criteria:
- id: ac-01
  description: Add heartbeat CronCreate at drive start
  gate: false
  checked: false
- id: ac-02
  description: Define heartbeat format string
  gate: false
  checked: false
- id: ac-03
  description: Document escalation ping one-shot
  gate: false
  checked: true
- id: ac-04
  description: Add CronDelete at drive completion
  gate: false
  checked: false
```

**Next AC:** ac-01 (first unchecked; no gates so nothing is blocked).

## Invariants

- The `acceptance_criteria` field is the single source of truth for AC status within a spec.
- AC IDs are stable — never renumbered after creation.
- "Next AC" is the first unchecked record whose declaration is not preceded by an unchecked gate.
- Helpers (`orbit-acceptance.sh`, `orbit spec update --ac-check / --ac-uncheck`) update the field through the canonical writer; ad-hoc YAML edits are discouraged because they risk drifting from canonical form (run `orbit canonicalise` after any direct edit).
