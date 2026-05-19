# Spec Review

**Date:** 2026-05-19
**Reviewer:** Context-separated agent (fresh session)
**Spec:** 2026-05-19-workflow-conformance
**Verdict:** REQUEST_CHANGES

---

## Review Depth

| Pass | Triggered by | Findings |
|------|--------------|----------|
| 1 — Structural scan | always | 4 |
| 2 — Assumption & failure | content signals (cross-system boundaries: plugin marketplace, `.orbit/config.yaml` schema, shared verb dispatch) + Pass 1 HIGH findings | 3 |
| 3 — Adversarial | not triggered (Pass 2 issues are localised — fixable in spec text, no structural rework of plan) | — |

## Findings

### [HIGH] ac-04 names a canonical file that does not exist
**Category:** missing-requirement
**Pass:** 1
**Description:** ac-04 requires byte-compare of `.orbit/STYLE.md` against `plugins/orb/skills/setup/STYLE.md`. The plugin does **not** ship a `STYLE.md` under `plugins/orb/skills/setup/` — only `SKILL.md` and `METHOD.md` exist there. `.orbit/STYLE.md` is canonical inside *this* repo (orbit dogfooding itself, surfaced via `@.orbit/STYLE.md` in CLAUDE.md), but it is not part of the plugin-shipped surface that `/orb:setup` copies into downstream consumers. Setup §6b only copies METHOD.md.
**Evidence:** `ls plugins/orb/skills/setup/` returns `SKILL.md  METHOD.md` only. `grep STYLE.md plugins/orb/skills/setup/SKILL.md` returns zero matches. The interview's "Implementation Notes" line 115 says `plugins/orb/skills/setup/STYLE.md` "verify path on implementation" — that verification was deferred, and the path is wrong.
**Recommendation:** Drop STYLE.md from ac-04's canonical-file set, OR add a prior step (likely belongs in card 0026 or a sibling card) that promotes STYLE.md into the plugin-shipped setup surface (parallel mechanism to §6b for METHOD.md). The latter widens scope; the former contracts ac-04 to two files (METHOD.md + .gitignore). Decide before implementation — implementing against a non-existent canonical path will fail.

### [HIGH] ac-01 and ac-06 reference type names and field names that do not exist
**Category:** missing-requirement
**Pass:** 1
**Description:** ac-01's description names `AuditDriftEntry` ("mirroring AuditDriftEntry / TopologyDriftEntry shape") — the existing type is `DriftEntry`, not `AuditDriftEntry`. ac-01 and ac-06 both name `AuditConformanceResult.aggregated.drift.entries` and `aggregated.topology.entries` (ac-06 verification: "assert `aggregated.drift.entries` contains the schema drift entry; `aggregated.topology.entries` contains the topology drift entry"). The actual existing field names are `AuditDriftResult.drift: Vec<DriftEntry>` and `AuditTopologyResult.topology_drift: Vec<TopologyDriftEntry>`. There is no `entries` field on either result type.
**Evidence:** `orbit-state/crates/core/src/verbs.rs:885-911` defines `AuditDriftResult { drift: Vec<DriftEntry> }` and `AuditTopologyResult { configured: bool, topology_drift: Vec<TopologyDriftEntry> }`. The entry type is `DriftEntry` (verbs.rs:889).
**Recommendation:** Edit ac-01 to say "mirroring DriftEntry / TopologyDriftEntry shape" (drop the `Audit` prefix on the first). Edit ac-06 verification to assert `aggregated.drift.drift` (contains schema-drift entries) and `aggregated.topology.topology_drift` (contains topology-drift entries), OR — better — require ac-06 to expose `aggregated.drift` and `aggregated.topology` as the existing `AuditDriftResult` / `AuditTopologyResult` types verbatim (byte-equal to the standalone sub-verbs' results), which makes the verification "the inner Result's existing fields appear unchanged". State the byte-equal contract explicitly; the current language reads as if a new `entries` field is being introduced.

### [HIGH] ac-05 plugin-version detection mechanism is unspecified and unverifiable
**Category:** test-gap
**Pass:** 1
**Description:** ac-05 says "read installed plugin version via cargo pkgid or the marketplace metadata (implementing agent picks the canonical source)". Neither candidate is correct for this codebase. `cargo pkgid` returns Cargo workspace metadata for `orbit-state` (the Rust binary), not the marketplace plugin (`plugins/orb/.claude-plugin/plugin.json`). The plugin and the binary version numbers can diverge — orbit-state-core version is unrelated to plugin manifest version. "Marketplace metadata" is hand-wavy: there is no API exposed from inside the orbit-state binary to read marketplace state. The actual canonical source for plugin version is `plugins/orb/.claude-plugin/plugin.json` (`"version": "0.4.20"`).
**Evidence:** `plugins/orb/.claude-plugin/plugin.json:4` carries `"version": "0.4.20"`. No code path in `orbit-state/crates/core/` reads this file today. `cargo pkgid` invoked from this workspace returns workspace-relative crate identifiers, not plugin manifest data.
**Recommendation:** Pin the version-detection mechanism to a concrete, testable shape before implementation: either (a) embed plugin version at compile time via `env!("CARGO_PKG_VERSION")` if the binary and plugin are released in lockstep (verify they are), or (b) read `plugins/orb/.claude-plugin/plugin.json` at runtime via a resolved plugin install path. Option (b) is portable across operator repos (downstream consumers don't have the plugin tree colocated — they have it under the marketplace cache). Implementing agent needs an answer before writing the PinState code path; "agent picks" is not testable.

### [HIGH] ac-04 canonical-source resolution is unspecified for downstream consumers
**Category:** missing-requirement
**Pass:** 1
**Description:** ac-04 says canonical bytes live at `plugins/orb/skills/setup/METHOD.md` etc. — fine for this repo, where the plugin tree is colocated. In a downstream consumer (the typical operator repo), the plugin is installed via the marketplace cache (e.g. `~/.claude/plugins/cache/orbit/orb/<version>/skills/setup/METHOD.md`). The spec does not name how `audit_conformance` resolves the installed-plugin path. Pinning to an older version (ac-05) magnifies this: the pinned plugin's canonical bytes may not be on disk at all if the operator only has the current version installed.
**Evidence:** Interview line 115 acknowledges the issue indirectly ("from the installed plugin's marketplace cache when pinned == current; from a stored snapshot when pinned to older version") — but ac-04 / ac-05 don't surface "stored snapshot" or canonical-path resolution as an AC. Without it, pin_behind state has no canonical bytes to compare against and the spec silently passes by suppressing per-file findings (ac-05) — but then "matches" state with a pinned-and-uninstalled older version also has no bytes to compare against, and the spec is silent on what happens.
**Recommendation:** Add an AC (or extend ac-04) naming the canonical-source resolution algorithm: (1) when pinned == current OR unpinned: resolve from the running plugin install path; (2) when pinned != current: define the behaviour explicitly — either "no per-file findings, just pin_behind/pin_ahead" (the current ac-05 contract covers pin_behind; pin_ahead is silent on this) or "fetch the pinned version's canonical from marketplace cache; if absent, emit a `canonical_unavailable` finding". Pick one; the silent-pass case is a bug-shaped omission.

### [MEDIUM] ac-08 mentions a `/orb:setup` SKILL.md section number that doesn't exist
**Category:** test-gap
**Pass:** 2
**Description:** ac-08 says "Update plugins/orb/skills/setup/SKILL.md with a new section (likely §6e or a top-level discovery note)". Grepping the file shows §6a through §6d but no §6e — and the section structure suggests §6e is a reasonable next slot. This is minor (the AC says "likely §6e", so it's hedged), but ac-08's verification is "Grep for `orbit audit conformance` and assert at least one mention" — that's a weak test (passes trivially if the verb name appears in any comment or aside). The AC should require the prose to name what the verb does, what it returns, and how to act on findings (which the description already lists), and the verification should check those three concrete claims appear in the prose.
**Evidence:** `plugins/orb/skills/setup/SKILL.md` has §6a–§6d in current structure. ac-08 description names the three required prose claims but verification only greps for the verb name.
**Recommendation:** Strengthen ac-08 verification: grep for the verb name AND for one phrase per documented behaviour (e.g. "structured findings envelope", "remediation"). Keeps the AC machine-verifiable while preventing a single-mention pass.

### [MEDIUM] ac-05 pin-ahead semantics are anomalous but their finding-list shape is ambiguous
**Category:** failure-mode
**Pass:** 2
**Description:** ac-05 says `pin_ahead` (operator pinned to a version newer than installed) is "anomalous, surface as a high-severity finding" with "no per-file suppression — pin_ahead is anomalous and agent should escalate". Concretely: if `plugin_version: "0.5.0"` is pinned but only `0.4.20` is installed, the audit emits a high-severity pin_ahead finding AND walks the operator files for byte-drift against… what? The installed 0.4.20 canonical, the pinned-but-absent 0.5.0 canonical, or nothing? The current AC says "no per-file suppression" but doesn't say what bytes the per-file comparison uses. If it uses installed bytes, drift findings will be noisy and misleading (operator pinned ahead deliberately — the installed bytes are not their truth source). If it uses pinned bytes (which aren't available), there are no findings to emit.
**Evidence:** ac-05 description language: "(no per-file suppression — pin_ahead is anomalous and agent should escalate)". Verification: "(c) pin_ahead fires one high-severity pin_ahead finding (no per-file suppression — pin_ahead is anomalous and agent should escalate)" — silent on what byte source is used.
**Recommendation:** Either (a) suppress per-file findings on pin_ahead the same way pin_behind does (single-finding model, pin issue dominates), OR (b) pick the byte source explicitly. (a) is simpler and consistent — pin_ahead is "the pin is wrong; everything else is downstream of that".

### [MEDIUM] ac-03 controllable-`today` injection is required but no arg is specified
**Category:** test-gap
**Pass:** 2
**Description:** ac-03 verification says "Test uses a controllable `today` (inject via args or test helper) — do NOT rely on system clock in tests". This is correct hygiene but no `today` parameter appears in `AuditConformanceArgs` (ac-01 names `AuditConformanceArgs` but doesn't enumerate its fields). The implementer has to either (a) add `today: Option<NaiveDate>` to the args struct (visible on CLI/MCP, potentially confusing), or (b) inject via test helper (private API, doesn't affect public envelope). Decide before implementation — the choice determines whether the public CLI surface gains a debug-only flag.
**Evidence:** ac-01 description names `AuditConformanceArgs` but does not enumerate fields. ac-03 verification mandates injection without specifying the surface.
**Recommendation:** Specify in ac-01 that `AuditConformanceArgs` has fields `{}` (no public fields) and that the test surface injects `today` via a sibling private helper signature like `audit_conformance_at(layout, today)`. Keeps the CLI/MCP surface clean. Add this to ac-01's description so the implementer doesn't have to guess.

---

## Honest Assessment

The plan is well-shaped and the design is internally coherent: aggregate-over-existing-audits, structured findings with explicit remediation verbs, pin-and-current model. The four HIGH findings are not architectural — they are factual mismatches between the spec text and the codebase: a canonical file that doesn't exist (STYLE.md), type names that don't exist (`AuditDriftEntry`, `aggregated.{drift,topology}.entries`), and an unspecified version-detection mechanism that "implementing agent picks" can't actually pick correctly. Fix those four in the spec text (one editing pass), tighten ac-05 pin-ahead semantics and ac-08 verification, and the plan is ready to implement.

Biggest risk if implemented as-written: the implementer starts work, discovers the canonical-file mismatch and the field-name mismatch on first compile, and either silently substitutes correct names (drifting the spec from reality) or halts for clarification (wasting an implementation cycle). Better to fix in spec now — five-minute edit vs an interrupted drive.
