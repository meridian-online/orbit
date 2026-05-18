# Spec Review

**Date:** 2026-05-18
**Reviewer:** Context-separated agent (fresh session)
**Spec:** 2026-05-18-topology-substrate-migration
**Verdict:** REQUEST_CHANGES

---

## Review Depth

| Pass | Triggered by | Findings |
|------|-------------|----------|
| 1 — Structural scan | always | 2 |
| 2 — Assumption & failure | content signals (schema rewire, cross-system parity, deprecation surface, idempotent setup verb) | 2 |
| 3 — Adversarial | not triggered (no cascading dependency or rollback shape — every finding is patchable in spec text) | — |

## Findings

### [MEDIUM] ac-01 references a non-existent helper `Layout::choice_path_for(id)`
**Category:** missing-requirement
**Pass:** 1
**Description:** ac-01's description and the matching dispatch rule in ac-02 name `Layout::choice_path_for(id)` as the resolver that turns a `decision_record` entry (typically a choice id like `0025`) into a filesystem path the drift checker can stat. That symbol does not exist in `orbit-state/crates/core/src/layout.rs`. The shipped surface is two-step: `resolve_numeric_slug(VERB, &layout.choices_dir(), id)` (see `verbs.rs:3371`) followed by `layout.choice_file(&resolved)`. A name that the implementer cannot grep maps to in the codebase makes the AC unverifiable as written and leaves the resolution semantics underspecified — does the migration AC also ship a new `Layout::choice_path_for` helper (and if so, where does it live and what does it return on a missing id), or does the drift checker inline the two-step pattern?
**Evidence:**
- ac-01 description: "decision_record entries first try Layout::choice_path_for(id) and fall back to direct path check."
- `orbit-state/crates/core/src/layout.rs:114` — `pub fn choices_dir(&self) -> PathBuf` (exists); no `choice_path_for` anywhere.
- `orbit-state/crates/core/src/verbs.rs:3371-3373` — the canonical "resolve choice id to filesystem path" pattern is `resolve_numeric_slug(...) → layout.choice_file(...)`.
**Recommendation:** Pick one and rewrite the dispatch rule:
- **Option A — name the existing two-step pattern.** Replace `Layout::choice_path_for(id)` with "resolve via `resolve_numeric_slug(VERB, &layout.choices_dir(), id)` then `layout.choice_file(&resolved)`; on resolution failure, fall through to direct path check". This keeps the helper surface unchanged.
- **Option B — ship a new helper.** Add an explicit ac-01 sub-item: introduce `Layout::choice_path_for(&self, id: &str) -> Option<PathBuf>` that wraps the resolve-then-file pair and returns `None` on resolution failure. Add a unit test covering bare-NNNN / padded / full-slug inputs against a layout with the choice present and absent.

Option A is the cheaper landing; B is worth it only if a second call site is already in flight.

### [MEDIUM] ac-02 names a parser function `load_topology_entries` that does not exist
**Category:** test-gap
**Pass:** 1
**Description:** ac-02's grep target asserts the migration removes `fn parse_topology_doc` and `fn load_topology_entries` from `verbs.rs`. `parse_topology_doc` exists at `verbs.rs:2910` and matches the spec's description. The second function — `load_topology_entries` — does not exist anywhere in `verbs.rs`. The actually-shipped helper at the line cited by the previous review-spec (verbs.rs:2975) is **`load_topology_subsystem_names`**, not `load_topology_entries`. An implementer running the grep assertion as written will pass it trivially (because the function never existed); a tidier implementer who notices `load_topology_subsystem_names` is the real legacy helper and removes that one too will pass ac-02's intent but fail its letter. The cycle-1 review introduced this name error and ac-02 inherited it.
**Evidence:**
- `verbs.rs:2910` — `fn parse_topology_doc(text: &str) -> Vec<TopologyEntry>` (the markdown parser, correctly named in the spec).
- `verbs.rs:2985` — `fn load_topology_subsystem_names(layout: &OrbitLayout) -> Vec<String>` (the actual legacy helper, called from `verbs.rs:1652` inside session-prime drift detection).
- `grep -n "fn load_topology_entries"` against `verbs.rs` returns zero matches.
**Recommendation:** Rewrite the ac-02 description and verification to grep for `fn parse_topology_doc` and `fn load_topology_subsystem_names` (not `load_topology_entries`). Confirm the call site at `verbs.rs:1652` (`let subsystems = load_topology_subsystem_names(layout);`) is rewired to the new per-file scanner — otherwise session-prime keeps calling a removed function.

### [LOW] ac-04's RETAINED-field deprecation contract leaves write-time behaviour unstated
**Category:** test-gap
**Pass:** 2
**Description:** ac-04 lands Option A from the cycle-1 review: `DocsConfig::topology` is retained as a parse-only deprecated field, so brownfield `Config::from_str` keeps succeeding on session-prime. The verification line says "the field round-trips (or is silently dropped on write — implementer's call)". That parenthetical leaves a real semantic gap: if the field is silently dropped on canonical-write, then any operator who runs `orbit verify` against a brownfield config carrying `docs.topology` will see verify mutate their on-disk config (round-trip drift), which is normally a hard failure of `verify_all`'s "round-trips through serde → canonical → serde" contract. If the field round-trips intact, then canonical-write preserves a key the spec says is deprecated and unused, which contradicts the design intent and leaks the deprecated key forward indefinitely. ac-05's brownfield-cleanup arm partially resolves this (the setup verb strips the key), but `orbit verify` runs independently of `orbit topology setup` and can fire first.
**Evidence:**
- ac-04 verification: "Round-trip test fixture `.orbit/config.yaml` whose docs.topology key points at docs/topology.md and assert Config::from_str succeeds and the field round-trips (or is silently dropped on write — implementer's call)."
- ac-01 verification names `verify_all` as the round-trip enforcer; if write canonicalisation drops a key that read preserves, `verify_all` flags it as drift.
- ac-05 cleans the field via `orbit topology setup` but does not pre-empt `orbit verify` runs before setup is invoked.
**Recommendation:** Pin one of:
- **Round-trip intact, write preserves.** State explicitly that the deprecated field is preserved on canonical write so `verify_all` sees no drift. The follow-on deletion spec (named in ac-04) does the actual removal. Verification: a fixture config with `docs.topology` set round-trips through canonical writer with the key still present.
- **Round-trip drops, verify skipped on this field.** Add a `#[serde(skip_serializing)]` (or equivalent) so the field reads-only-never-writes, AND extend `verify_all` to special-case this field so round-trip drift on `docs.topology` is not a failure. Verification: a fixture's read returns the deprecated field; canonical write omits it; `orbit verify` returns clean (not drift) on a config that previously carried the field.

Either is defensible; "implementer's call" is not, because the two paths have observably different operator surfaces.

### [LOW] ac-05's "elide empty docs block" rule sets up a config-rewrite race against §6d's idempotency check
**Category:** failure-mode
**Pass:** 2
**Description:** ac-05 says the setup verb, on a brownfield config carrying *only* `docs.topology`, removes the key AND elides the now-empty `docs:` block. ac-04 keeps `DocsConfig::topology` parseable so existing brownfield configs do not hard-fail. The combination produces a subtle ordering hazard: on a brownfield repo, `/orb:setup` §6d's "idempotent check" (per the existing skill) reads `.orbit/config.yaml`; if the verb rewrites the config underneath, a second §6d invocation in the same session reads the rewritten config and may take a different code path (e.g. fire the "wire topology now?" prompt because `docs.topology` is now absent). This is benign in interactive use (the operator sees a prompt and answers `y` to re-scaffold) but degrades the §6d idempotency claim ("idempotent on a wired repo"). The §6d prose explicitly carries forward the byte-compare-and-prompt voice, so the prompt firing twice in one session is a UX wart, not a correctness break.
**Evidence:**
- ac-05 description: "The §6d byte-compare-and-prompt voice still applies: prompt fires when .orbit/topology/ is absent or empty OR when legacy docs.topology cleanup is pending; idempotent on a populated repo with a clean config."
- ac-05 explicitly lists "idempotent" as a closing condition but the cleanup arm mutates the config on first run, so the "populated repo with a clean config" idempotency only holds *after* the first invocation.
**Recommendation:** Tighten the ac-05 idempotency claim. State that idempotency holds in two stages: (i) first invocation on a brownfield substrate mutates the config and seeds the directory; (ii) every subsequent invocation is a no-op on both surfaces. Add a verification line that runs `orbit topology setup` twice in succession against a brownfield fixture and asserts the second invocation produces no on-disk diff. This is a one-line addition that closes the gap without changing the cleanup behaviour.

---

## Honest Assessment

The cycle-2 spec materially improves on cycle-1. Both MEDIUM findings from cycle-1 — the missed `docs.topology` call sites in distill/release skills and the brownfield config-load failure — are landed: ac-04 explicitly names all three plugin skills and adopts Option A (deprecated-but-parseable field) with a follow-on deletion spec; ac-05 adds the brownfield config cleanup arm. The supersession story remains clean and the substrate-engagement parity contract holds.

The two MEDIUM findings here are precision defects, not design defects: ac-01 names a non-existent helper (`Layout::choice_path_for`) and ac-02 names a non-existent function (`load_topology_entries`) carried forward from the cycle-1 review. Both are five-minute fixes in the spec text — pick the canonical name, regrep, update the verification line. The risk of leaving them is that the implementer either (a) cannot run the grep assertion as written, or (b) runs it and gets a false-pass against the unrenamed surface.

The two LOWs are tighten-the-contract items: ac-04's "implementer's call" parenthetical and ac-05's two-stage idempotency. Neither blocks implementation; both are worth pinning before the drive enters the implement stage so the close-time evidence has a single canonical shape.

Biggest residual risk: ac-04's write-time round-trip behaviour for the deprecated field. If left unspecified, the implementer may pick either Option A (preserve on write) or Option B (drop on write with verify allowance) and the close-time evidence will reflect that choice — which is fine for ac-04 closing, but the follow-on deletion spec inherits a baseline whose shape was not deliberately chosen.
