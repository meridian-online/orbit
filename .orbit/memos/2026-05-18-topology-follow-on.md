# Memo — topology follow-on spec scope

**Date:** 2026-05-18
**Context:** Spec 2026-05-18-documentation-topology closed at 8/13 ACs per Hugh's "continue through ac-06 only" decision. This memo captures the remaining 4 work-ACs + 1 observation-AC for distillation into a follow-on spec.

## What shipped in the parent spec

- **ac-01** — `/orb:topology` skill at `plugins/orb/skills/topology/SKILL.md` with three modes (write / read / audit). Front-matter + prose follow the `/orb:code-investigate` convention.
- **ac-02 / ac-03 / ac-04** — `Config` + `DocsConfig` structs in `orbit-state/crates/core/src/schema.rs` with `FIELDS` consts, schema-drift tests, `deny_unknown_fields`. `layout.config_file()` for `.orbit/config.yaml`. `verify_all` runs `check_round_trip::<Config>` when the file exists; absence tolerated. 11 new tests.
- **ac-05** — METHOD.md posture line (*"Substrate beats extrapolation"*) + `--label topology` convention in both `.orbit/METHOD.md` and the canonical `plugins/orb/skills/setup/METHOD.md`.
- **ac-06** — `orbit audit topology` verb on CLI + MCP. Topology doc parser + drift detection across `stale_pointer` / `missing_entry` / `shape_drift`. 8 core tests + 3 CLI integration tests. Symmetric exit-0 contract with `orbit audit drift`.
- **ac-10** — `/orb:release` SKILL.md §1 step 5: topology-drift surface step with N=10 truncation rule.
- **ac-11** — `/orb:distill` SKILL.md §5 step 5: quality-gated topology nudge fired only when a distillation produces a subsystem-level capability.

Test suite: **306 / 306 passing** at the parent spec close.

## What's left for the follow-on

### ac-07 — /orb:setup integration

Greenfield path: scaffold `.orbit/config.yaml` with `docs.topology: docs/topology.md` and a stub `docs/topology.md` (heading + one-paragraph explainer + empty entry list).

Brownfield path: detect whether `.orbit/config.yaml` exists; prompt to add the `docs.topology` key if missing (operator can decline). Brownfield-accept: if the target path named by `docs.topology` does not exist on disk, ALSO create the stub (suppresses first-prime drift noise). If the target file exists, wire the pointer but do NOT overwrite.

Surfaces to edit:
- `plugins/orb/skills/setup/SKILL.md` — add §6e (or equivalent) documenting the new scaffolding step in the byte-compare-and-prompt voice
- Setup shell script(s) — write the actual scaffolding logic

### ac-08 — orbit session prime envelope extension

Extend the `session_prime` verb's result struct with `topology_drift: Vec<TopologyDriftEntry>`. Additive to the existing envelope (`handover` / `item_bound` / `memories` / `next_step` / `open_specs`). Skip-on-default when `.orbit/config.yaml` is absent or `docs.topology` not set (key omitted entirely from envelope).

Surfaces to edit:
- `SessionPrimeResult` in `orbit-state/crates/core/src/verbs.rs` — add `#[serde(default, skip_serializing_if = "Vec::is_empty")] topology_drift: Vec<TopologyDriftEntry>` field (or similar; might need Option<Vec> to distinguish empty-array from key-absent)
- `session_prime` function — call `audit_topology` internally when configured, populate the field
- Parity test on CLI + MCP for all three states (configured + clean / configured + drift / not configured)

Implementation note: the field is shared with `audit.topology` output, so the type already exists.

### ac-09 — spec.close topology_warnings + word-boundary heuristic

Extend the `spec.close` verb's ok envelope with `topology_warnings: Vec<TopologyDriftEntry>`. Detection heuristic: read the spec's `spec.yaml` and (when present) `interview.md`; for each topology entry whose subsystem name is ≥ 5 characters long, test for case-insensitive word-boundary match (regex `\b<subsystem>\b`) in the concatenated spec text.

Surfaces to edit:
- `SpecCloseResult` in verbs.rs — add `topology_warnings` field
- `spec_close` function — load topology entries (via the existing audit_topology code path), apply the heuristic against spec text, populate field
- Parity test on CLI + MCP

Cycle-2 reviewer note (cycle-2 LOW finding #3): the `\b<subsystem>\b` regex must call `regex::escape` on the subsystem name before interpolation — careful implementer will get this right by default.

### ac-12 — orbit memory remember --label topology nudge

Post-store hook on `memory.remember` that checks the supplied `--label` list for `"topology"` and emits a nudge (*"consider /orb:topology"* or near-equivalent — exact wording locked at implementation). Nudge is non-blocking (memory stores successfully) and quiet (suppressible via `--no-nudge` flag or equivalent — final flag name decided in implementation).

Surfaces to edit:
- `MemoryRememberResult` in verbs.rs — add `nudge: Option<String>` field (or emit to stderr in the CLI; envelope-side is cleaner for MCP)
- `memory_remember` function — detect the label, populate the nudge field
- CLI — render the nudge to stderr (or stdout in human mode); add `--no-nudge` flag
- Parity test on CLI + MCP

### ac-13 — Observation-band (deferred until 2026-06-15+)

4-week usage audit. Earliest fire date: 4 weeks after the parent spec ships (anchor: 2026-05-18 → audit window opens 2026-06-15). Audit reads (a) topology doc entry count + update frequency by subsystem, (b) memories labelled `topology` over the window, (c) `topology_warnings` counts from `spec.close`, (d) `topology_drift` counts from `session prime`. Produces memo at `.orbit/memos/2026-06-15-topology-4-week-audit.md` with data tables + recommendation.

This AC inherits its observation-band semantics — does not block close.

## Recommended follow-on spec shape

Slug: `2026-05-19-topology-substrate-wires` (or `topology-envelope-extensions`).

5 ACs — ac-07 setup, ac-08 session prime, ac-09 spec.close, ac-12 memory remember, ac-13 observation. Cleaner unit than the parent: all of these consume the substrate already shipped (Config schema + audit verb), so the follow-on is pure envelope/UX surface.

Cycle-2 review LOWs to carry into the follow-on spec:
1. `regex::escape` on subsystem name before `\b<subsystem>\b` interpolation in ac-09
2. Normalise `topology_drift` entry shape reference across ac-08/09 (DRY against the type already defined in verbs.rs)
3. Consider whether ac-02's bundled (struct + FIELDS const + drift test) needs splitting — reviewer accepted the bundling for the parent spec but flagged it

## Cluster note

Card 0040 (documentation-topology) joins 0025-codebase-mastery + 0037 + 0038 + the autonomy-too-ready-to-halt memo in the agent-side substrate-engagement cluster. The cluster synthesis card hasn't been opened yet — memo flagged it as "worth watching, not opening preemptively". This memo is one more concrete instance in the cluster; still no synthesis trigger.
