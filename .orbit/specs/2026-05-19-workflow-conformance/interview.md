---
date: 2026-05-19
interviewer: Claude Opus 4.7
card: .orbit/cards/0039-setup-conformance-check.yaml
mode: discovery
---

# Discovery: Workflow conformance

## Context

Card 0039 was distilled from a 2026-05-16 memo as "setup conformance check — `/orb:setup` verifies the full canonical surface." Its scenarios framed the work as **byte-compare of plugin-shipped canonical files** (STYLE.md, METHOD.md, `.orbit/.gitignore`) with per-file operator prompts. This session opened as `/orb:design` against that framing.

The author's first answers reframed the design space: conformance is about **evidence that the workflow is running as intended**, not file-byte equality. The consumer of conformance output is an **agent**, not a human operator. The framing pivot was large enough that `/orb:design` halted and `/orb:discovery` was invoked to re-explore the topic from intent.

Adjacent prior art:
- Card 0040's `/orb:design` interview (2026-05-18) named card 0039 as "the conformance check that handles the topology doc via the config pointer" — pre-establishing the topology-pointer link.
- `orbit audit drift` (schema-field permissive scan) and `orbit audit topology` (subsystem-pointer drift) already emit structured findings via the verb envelope. Pattern is established.
- `/orb:setup §6b` byte-compare-and-prompt mechanism (METHOD.md only) and `§6d` topology scaffolding establish the seed voice for plugin-canonical handling.

## Q&A

### Q1: Trigger
**Q:** When does the conformance check run — who triggers it, on what occasion?
**A:** On-demand by the agent. (Not baked into session prime; not operator-triggered; not on every substrate write.)

### Q2: Evidence surface
**Q:** What counts as 'workflow evidence' — which classes of finding does the check produce?
**A:** Substrate-state drift + plugin-canonical-file drift. Note from the author: "orbit methodology will continue to update — e.g. BLUF may drop out of STYLE.md. Conformance is about keeping this up to date." Reframes the file-byte case as plugin-canonical-version tracking, not style enforcement. Pipeline anti-patterns and convention adherence are **out** for v1.

### Q3: Audience
**Q:** Who reads conformance output and acts on it?
**A:** Agent acts; operator reads when asked. The agent's job is to triage findings and remediate (e.g. propose `/orb:distill` on an undistilled memo). The operator only sees findings when the agent escalates or surfaces them as part of a decision brief.

### Q4: Audit layering
**Q:** Relationship to existing audits — subsume, sibling, or aggregate?
**A:** Aggregate. Recommendation accepted: `orbit audit conformance` calls `audit_drift` + `audit_topology` internally plus new substrate-state checks, emits one envelope. Existing audit verbs stay as primitives. Matches `orbit verify`'s composition pattern.

### Q5: Plugin-version reference
**Q:** Conformance compares the local repo against which version of the plugin?
**A:** Pinnable per-repo, defaults to current. Each repo can pin a plugin version in `.orbit/config.yaml`; conformance compares to the pin. Unpinned repos compare to the currently-installed plugin.

### Q6: Findings shape
**Q:** What does the agent receive per finding?
**A:** Description + remediation verb. Each finding carries an explicit `remediation` field naming the next-action handle (e.g. `orbit setup`, `/orb:design 15`, `/orb:distill`). Agent acts without translation.

### Q7: Thresholds
**Q:** How is 'stuck' or 'stale' defined for substrate-state findings?
**A:** Recommendation accepted: hybrid, lean state-based.
- **Cards:** state-based. `maturity:planned + specs:[]` always fires.
- **Specs:** state-based. (Deferred from v1 — see Q9.)
- **Drives:** state-based. (Deferred from v1 — see Q9.)
- **Memos:** time-based. Undistilled > 7 days. Memos are inherently transient; state-based would defeat the lifecycle.
- No config override in v1. Defaults ship in the plugin; revisit when friction surfaces.

### Q8: Out of scope
**Q:** What is explicitly NOT the conformance verb's concern?
**A:** All four candidates: code drift (lint/tests/types), commit/git hygiene, throughput/velocity metrics, plugin-side checks (SKILL.md drift, hooks.json sanity). Conformance is exclusively about the operator's repo substrate vs the plugin contract.

### Q9: v1 scope
**Q:** Which finding families ship in v1?
**A:** Three families:
1. **Plugin-canonical-file drift** — STYLE.md / METHOD.md / `.gitignore` byte-compare against pinned plugin version. Remediation: `orbit setup`.
2. **Card-state findings** — cards at `maturity:planned` with empty `specs` array. Remediation: `/orb:design <id>`.
3. **Memo staleness** — memos undistilled > 7d. Remediation: `/orb:distill <memo-path>`.

Spec-state findings (open without ACs, post-claim without progress.md) deferred — open specs already have their own substrate hygiene paths via `orbit verify`.

### Q10: Pin lifecycle
**Q:** When does the plugin-version pin advance?
**A:** Operator via `/orb:setup` OR agent via remediation. Agent can offer to bump the pin as a remediation step when drift is widespread; operator confirms. Agent never silently advances.

### Q11: New-canonical-file semantics under older pin
**Q:** When the current plugin ships a new canonical file (e.g. v0.5.0 adds `.orbit/PRINCIPLES.md`) and the repo is pinned to v0.4.x, how does conformance behave?
**A:** Implementation-level — author flagged this as needing means-level context to answer. Routed to implementation notes (see below).

---

## Summary

### Goal

Provide an on-demand verb the agent invokes to check whether the operator's repo is **operating against the current (or pinned) plugin's contract**. Output is a structured findings envelope (description + remediation verb per finding) that the agent triages without operator involvement, escalating only when warranted.

### Constraints

- **Aggregate over existing audits** (`audit_drift`, `audit_topology`) — don't subsume; compose.
- **Plugin-canonical files** are sourced from the installed plugin at `plugins/orb/skills/setup/*.md` (or pinned version).
- **Findings carry `remediation.verb`** — every finding is action-shaped.
- **Audience is agent-first** — operator only sees output on agent escalation; zero-finding case is silent.
- **Per-repo pin** in `.orbit/config.yaml` (key TBD by implementing agent); unpinned defaults to current.
- **Out of scope:** code drift, git hygiene, velocity, plugin-side checks.

### Success Criteria

- v1 produces findings across three families: plugin-canonical-file drift, card-state (planned+empty), memo-stale (>7d).
- Each finding has `subject`, `state`, `severity`, `evidence`, `remediation.verb` (machine-parseable).
- Clean repo: zero findings, exit 0. (Mirrors `audit drift` / `audit topology` patterns.)
- Pinning works: a repo pinned to v0.4.18 doesn't surface v0.4.20-introduced drift.
- Agent can ingest the envelope, decide which findings to remediate, and act without operator translation.

### Decisions Surfaced

- **Aggregate over existing audits** — chose aggregate over subsume/sibling. Reuses `orbit verify`'s composition pattern. Existing CLIs stay as primitives.
- **State-based thresholds for cards; time-based for memos** — chose hybrid over fixed-uniform or all-state. Aligns each family with its lifecycle.
- **Pin-and-current model** — chose pinnable-defaults-to-current over rolling-only or latest-released. Operator owns upgrade cadence; agent can offer bumps as remediation.
- **Card 0039 is the receiver** — discovery rewrites the card's scope from "setup verifies canonical surface" to "workflow conformance — evidence the pipeline is operating against the plugin contract." Card slug stays (`0039-setup-conformance-check`); body is rewritten. (May want to revisit the slug, but the id is the stable handle.)

These decisions are candidates for a MADR choice file at `.orbit/choices/NNNN-workflow-conformance-shape.yaml` if the spec lands.

### Implementation Notes

- **Verb name & location.** `orbit audit conformance` is the natural CLI shape. Aggregates `audit_drift` + `audit_topology` results into one envelope alongside new substrate-state findings. Lives in `orbit-state/crates/core/src/verbs.rs` as `audit_conformance(layout, args)` per existing audit-verb patterns.
- **Findings envelope.** Match `AuditDriftResult` / `AuditTopologyResult` shape. Single `findings: Vec<ConformanceFinding>`. Each entry: `{severity, subsystem, subject, state, evidence, remediation: {verb, rationale}}`. JSON output is byte-identical to MCP `tools/call` payload — preserves existing parity test pattern.
- **Plugin-canonical source.** STYLE.md / METHOD.md / `.gitignore` canonical bytes live at `plugins/orb/skills/setup/STYLE.md`, `plugins/orb/skills/setup/METHOD.md`, and `plugins/orb/scripts/orbit.gitignore` (verify path on implementation). Conformance reads these from the installed plugin's marketplace cache when pinned == current; from a stored snapshot when pinned to older version.
- **Pin storage.** Add `plugin_version: "0.4.20"` (or similar) to `.orbit/config.yaml`. Unpinned = `None` = current. `Config` struct in orbit-state already has `DocsConfig`; add a sibling field.
- **New-canonical-file semantics (Q11 default).** When the installed plugin is ahead of the pinned version, emit ONE finding: `{state: "pin_behind", subject: ".orbit/config.yaml", evidence: {pinned: "0.4.18", current: "0.4.20"}, remediation: {verb: "orbit setup --bump-pin"}}`. Do NOT enumerate per-new-file findings under that condition — the pin-bump prompt absorbs them. Implementing agent may revise after one usage cycle.
- **Card-state finding.** Walk `.orbit/cards/*.yaml`. Fire for each card where `maturity == "planned" && specs.is_empty()`. `remediation.verb`: `/orb:design <card-id>`. Severity: `medium` (these aren't blockers; they're "ready to advance").
- **Memo-stale finding.** Walk `.orbit/memos/*.md`. For each memo, fire if `(now - file_mtime).days > 7`. `remediation.verb`: `/orb:distill <memo-path>` (or whatever the canonical distill invocation is). Severity: `medium`. Note: filename includes a date (e.g. `2026-05-16-*.md`) — consider using filename date as the staleness anchor rather than mtime (more semantic; survives git operations that mangle mtime).
- **Severity model.** Borrow review-pr's HIGH/MEDIUM/LOW. v1 likely uses `medium` for everything (no blockers); reserve `high` for actual contract violations (e.g. STYLE.md byte-drift on a repo that imports STYLE.md as load-bearing prose).
- **AC types.** Most v1 ACs will be `code` (Rust verb + tests). The "pin bump via /orb:setup" AC may be `code` (verb supports `--bump-pin`) plus `doc` (SKILL.md §6e or wherever the operator surface lives).
- **Test surface.** Unit tests in `audit_conformance` covering each finding family. CLI parity test (one) covering the human-mode rendering. MCP parity test (one) covering the JSON envelope shape.
- **First-cut UI.** `orbit audit conformance` with `--json` flag. Human mode prints findings as a numbered list with severity + subject + remediation hint. JSON mode emits the structured envelope.

### Open Questions

- **Slug rewrite for card 0039?** Current slug `setup-conformance-check` no longer matches the reframed scope ("workflow conformance"). Renaming the slug breaks file path and external references (card 0040's interview names it, card 0017 has a depends-on relation). Keeping the slug-as-historical-artefact is the safer call but creates a mild future-friction signal. Author decision needed before `/orb:spec`.
- **Severity calibration.** v1 plan is `medium` for everything. A second design pass after one usage cycle may identify cases that warrant `high` (e.g. BLUF dropped from plugin STYLE.md but repo still imports the old text) vs `low` (e.g. memo just stale, no action urgency).
- **Pin format and migration.** New `.orbit/config.yaml` key `plugin_version`. Existing repos won't have it — silent default to "current" or one-time "first conformance run records pin"? Implementing agent's call, but worth surfacing.
