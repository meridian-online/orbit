---
name: topology
description: Architecture-level investigation discipline made cheap — scaffolds, reads, and audits a subsystem-keyed pointer-only doc that points at each subsystem's canonical code, decision record, operational doc, and test surface. Three modes — write, read, audit.
user-invocable: true
created_by: human
created_at: 2026-05-18
pinned: true
---

# /orb:topology

Architecture-level investigation discipline made cheap. Sibling to `/orb:code-investigate` — that skill makes file-level investigation cheap; this one makes the same investment at the architecture level. The agent owns the codebase at both granularities.

Three modes:

- **Write mode** — agent passes a subsystem name and the skill scaffolds a new entry (interview-style) or updates an existing one (targeted edit). Each entry is five lines.
- **Read mode** — agent passes a subsystem name (or a question like *how does X work?*) and the skill returns the entry, citing the canonical sources by path.
- **Audit mode** — agent invokes without a subsystem name and the skill reports drift: stale pointers, missing entries for subsystems detected in the codebase, entries that don't follow the five-line convention.

## Usage

```
/orb:topology [subsystem-name | question | --audit]
```

- A subsystem name (or a question naming a subsystem) → write mode if no entry exists, read mode if it does.
- `--audit` (or no argument) → audit mode.

## The five-line entry shape

Pointer-only. Each entry references the canonical sources but carries no content of its own — when the authoritative file's docstring updates, the entry stays correct because there's nothing to update.

```
## <subsystem>

- code: <path to authoritative implementation>
- decision: <path to MADR / architectural decision record>
- operational: <path to runbook, deployment doc, or operational notes>
- tests: <path to test surface>
- what: <one sentence — what this subsystem gives you>
```

The line labels (`code`, `decision`, `operational`, `tests`, `what`) are normative — they're the five anchors the audit checks for. Pointers can be relative to the repo root or absolute paths; the audit resolves them either way.

## Write mode

When invoked with a subsystem name and no existing entry:

1. **Scaffold the five lines.** Ask the agent (or the author, if interactive) for each anchor in turn — keep the prompt tight: subsystem name → code path → decision path → operational path → test path → one-sentence what.
2. **Refuse hand-waved pointers.** Each path must resolve on disk at write time. If `decision` is `none` or `operational` is `n/a`, take it but note the gap — these become drift candidates the audit will surface.
3. **Append the entry to the topology doc** at the path named by `.orbit/config.yaml`'s `docs.topology` key. If the doc doesn't exist yet, create it with the heading + a one-paragraph explainer (the `/orb:setup` scaffold writes this on greenfield).
4. **Quote the entry back** so the author can confirm before the next invocation.

When invoked with a subsystem name that already has an entry, this is an update — show the current entry, ask which anchor changed, edit in place. Don't re-interview the whole entry.

## Read mode

When invoked with a subsystem name (or a question that resolves to one):

1. **Resolve the subsystem.** Substring-match the name against the topology doc's entries — case-insensitive. If multiple match, list them as a disambiguation prompt.
2. **Return the entry verbatim** with the five lines, then **load the cited sources** — read the `code` file, the `decision` MADR, and the `tests` file at minimum. Cite each by path + line number when surfacing facts back.
3. **Don't extrapolate.** If the answer to the agent's question isn't visible in the cited sources, say so. Substrate beats extrapolation — the skill's job is to route to the canonical sources, not to manufacture answers from one of them.

## Audit mode

When invoked without a subsystem name (or with `--audit`):

```bash
orbit audit topology
```

The verb walks the topology doc and reports drift in three categories:

| Category | Detected when |
|----------|---------------|
| `stale_pointer` | An entry's pointer (any of code/decision/operational/tests) names a path that no longer exists |
| `missing_entry` | A subsystem detected in the codebase (initial heuristic: top-level directory under `src/` or `crates/`) has no entry in the topology doc |
| `shape_drift` | An entry doesn't follow the five-line convention (missing anchor, wrong label, extra lines) |

Exit code is 0 for all outcomes. Discrimination is via the envelope's `topology_drift` array — never `$?`. This is symmetric with `orbit audit drift`'s exit-0-even-on-drift contract.

## Discipline

- **Pointer-only.** Entries carry no content of their own. If you find yourself writing prose into the topology doc, extract it to the canonical source and link to it.
- **Substrate beats extrapolation.** Before reasoning about how a subsystem works, read its topology entry and load the cited sources. Don't extrapolate from a single file when the topology entry names four others.
- **Update at learning moments, not edit moments.** The trigger surface is `/orb:distill` completion, `orbit memory remember --label topology`, and `orbit session prime`'s drift surface — not every code edit. The doc accretes as the codebase does.
- **Quality-gate the writes.** Not every code change warrants a topology update. Subsystem-level changes do — a new module, a moved authoritative file, a shifted boundary. Single-file fixes do not.

## After using this skill

If something non-obvious surfaced — a subsystem boundary that was unclear, an investigation pattern worth keeping, or an entry-shape question worth surfacing — write a short memory:

```bash
orbit memory remember <key> "<body>" --label topology
```

The label `topology` is the substrate seam the learning loop pivots on. When a memory carries this label, `orbit memory remember` emits a nudge prompting the agent to consider whether the topology doc itself should be updated — closing the loop between insight and substrate.

## Architecture-level analogue framing

`/orb:code-investigate` makes file-level investigation cheap; `/orb:topology` makes architecture-level investigation cheap. The two share a design contract — token-frugal default, pointer-only over duplication, reach-as-default rather than reach-on-demand. The agent owns the code at both granularities.
