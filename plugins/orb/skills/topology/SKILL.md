---
name: topology
description: Architecture-level investigation discipline made cheap — scaffolds, reads, and audits subsystem-keyed pointer-only yaml entries that point at each subsystem's canonical code, decision record, operational doc, and test surface. Three modes — write, read, audit.
user-invocable: true
created_by: human
created_at: 2026-05-18
pinned: true
---

# /orb:topology

Architecture-level investigation discipline made cheap. Sibling to `/orb:code-investigate` — that skill makes file-level investigation cheap; this one makes the same investment at the architecture level. The agent owns the codebase at both granularities.

**Substrate shape** (per choice 0025 — `topology-substrate-folder`): each subsystem is one yaml file at `.orbit/topology/<subsystem>.yaml`, parsed against `schema::TopologyEntry` by the existing orbit-state machinery. Pruning a stale subsystem is `rm`. Drift detection is structural (serde + validate), not markdown-heuristic.

Three modes:

- **Write mode** — agent passes a subsystem name and the skill scaffolds a new entry (interview-style) or updates an existing one (targeted edit).
- **Read mode** — agent passes a subsystem name (or a question like *how does X work?*) and the skill returns the entry, citing the canonical sources by path.
- **Audit mode** — agent invokes without a subsystem name and the skill reports drift: dangling pointers, validate failures, missing entries for subsystems detected in the codebase.

## Usage

```
/orb:topology [subsystem-name | question | --audit]
```

- A subsystem name (or a question naming a subsystem) → write mode if no entry exists, read mode if it does.
- `--audit` (or no argument) → audit mode.

## The entry shape

Pointer-only. Each entry references the canonical sources but carries no content of its own — when the authoritative file's docstring updates, the entry stays correct because there's nothing to update.

```yaml
# .orbit/topology/<subsystem>.yaml
subsystem: <slug — kebab-case, lowercase, ≥ 5 chars>
canonical_code:
  - <path to authoritative implementation>
decision_record:
  - <choice id (e.g. 0025) or path to MADR>
operational_doc:
  - <path to runbook, SKILL.md, or operational notes>
test_surface:
  - <path to test surface>
```

- `subsystem` and `canonical_code` are required; the others are optional but expected for any well-documented subsystem.
- All four pointer fields are **lists** — most subsystems span multiple files. The schema accepts a single-entry list when only one path applies.
- `subsystem` must be slug-shaped (lowercase letters, digits, hyphens; ≥ 5 chars; no leading/trailing/double hyphens) and equal the file stem.
- `decision_record` entries are typically choice ids (`0025`) — the audit resolves them via `resolve_numeric_slug` then `layout.choice_file`. Direct paths also work and are checked as-is.

## Write mode

When invoked with a subsystem name and no existing entry:

1. **Scaffold the yaml.** Ask the agent (or the author, if interactive) for each field in turn — keep the prompt tight: subsystem slug → canonical_code paths → decision_record (choice id or path) → operational_doc path → test_surface path.
2. **Refuse hand-waved pointers.** Each path must resolve on disk at write time. If a field is genuinely empty, leave the list out — the schema treats absent optional fields as empty.
3. **Write the file** at `.orbit/topology/<subsystem>.yaml` via the Write tool. The canonical writer is invoked at substrate-side (`orbit canonicalise` after write) to normalise byte form.
4. **Quote the entry back** so the author can confirm before the next invocation.

When invoked with a subsystem name that already has an entry, this is an update — show the current entry, ask which field changed, edit in place. Don't re-interview the whole entry.

## Read mode

When invoked with a subsystem name (or a question that resolves to one):

1. **Resolve the subsystem.** The canonical lookup is `Read` the file at `.orbit/topology/<subsystem>.yaml`. If absent, try slug-prefix match (e.g. *"how does cards work?"* → look for `.orbit/topology/cards.yaml`).
2. **Return the entry verbatim** then **load the cited sources** — read the `canonical_code` files, the `decision_record` MADRs, and the `test_surface` files at minimum. Cite each by path + line number when surfacing facts back.
3. **Don't extrapolate.** If the answer to the agent's question isn't visible in the cited sources, say so. Substrate beats extrapolation — the skill's job is to route to the canonical sources, not to manufacture answers from one of them.

## Audit mode

When invoked without a subsystem name (or with `--audit`):

```bash
orbit audit topology
```

The verb walks the `.orbit/topology/` directory and reports drift in these categories (envelope shape stable for parity with spec `2026-05-18-topology-substrate-wires`):

| Category | Detected when |
|----------|---------------|
| `stale_pointer` | An entry's pointer (any of `canonical_code` / `decision_record` / `operational_doc` / `test_surface`) names a path that no longer exists |
| `missing_entry` | A subsystem detected in the codebase (top-level directory under `src/` or `crates/`) has no entry in `.orbit/topology/` |
| `invalid_field` | An entry parses as yaml but fails `TopologyEntry::validate` (subsystem slug too short, not slug-shaped, or empty `canonical_code`) |
| `parse_failed` | An entry can't be parsed as `TopologyEntry` yaml (unknown field, wrong type, missing required) |

`audit_topology(...).configured` is true iff `.orbit/topology/` exists AND contains ≥ 1 entry (populated == configured). Exit code is 0 for all outcomes. Discrimination is via the envelope's `topology_drift` array — never `$?`. Symmetric with `orbit audit drift`'s exit-0-even-on-drift contract.

## Discipline

- **Pointer-only.** Entries carry no content of their own. If you find yourself writing prose into a topology yaml, extract it to the canonical source and link to it.
- **Substrate beats extrapolation.** Before reasoning about how a subsystem works, read its topology entry and load the cited sources. Don't extrapolate from a single file when the topology entry names four others.
- **Update at learning moments, not edit moments.** The trigger surface is `/orb:distill` completion, `orbit memory remember --label topology`, and `orbit session prime`'s drift surface — not every code edit. The substrate accretes as the codebase does.
- **Quality-gate the writes.** Not every code change warrants a topology update. Subsystem-level changes do — a new module, a moved authoritative file, a shifted boundary. Single-file fixes do not.
- **Prune cheaply.** When a subsystem is genuinely gone, `rm .orbit/topology/<subsystem>.yaml`. The per-file shape makes the prune trivial; substrate hygiene depends on it actually happening.

## After using this skill

If something non-obvious surfaced — a subsystem boundary that was unclear, an investigation pattern worth keeping, or an entry-shape question worth surfacing — write a short memory:

```bash
orbit memory remember <key> "<body>" --label topology
```

The label `topology` is the substrate seam the learning loop pivots on. When a memory carries this label, `orbit memory remember` emits a nudge prompting the agent to consider whether the topology substrate itself should be updated — closing the loop between insight and substrate.

## Architecture-level analogue framing

`/orb:code-investigate` makes file-level investigation cheap; `/orb:topology` makes architecture-level investigation cheap. The two share a design contract — token-frugal default, pointer-only over duplication, reach-as-default rather than reach-on-demand. The agent owns the code at both granularities.
