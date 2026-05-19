# Spec Review

**Date:** 2026-05-19
**Reviewer:** Context-separated agent (fresh session)
**Spec:** 2026-05-19-workflow-conformance
**Verdict:** APPROVE

---

## Review Depth

| Pass | Triggered by | Findings |
|------|--------------|----------|
| 1 — Structural scan | always | 3 |
| 2 — Assumption & failure | content signals (cross-system boundary: plugin manifest version vs orbit-state binary; new `.orbit/config.yaml` field; cross-skill prose update on setup SKILL.md/METHOD.md) | 1 |
| 3 — Adversarial | not triggered (Pass 2 issues are localised — each is a small implementation-time choice with a clear default, none undermines plan structure) | — |

## Findings

### [LOW] chrono is not yet a workspace dependency
**Category:** missing-requirement
**Pass:** 1
**Description:** ac-01 and ac-03 require `chrono::NaiveDate` (`chrono::NaiveDate::parse_from_str("%Y-%m-%d")` in ac-03, `chrono::Local::today().naive_local()` in ac-01). The orbit-state workspace currently uses the `time` crate (`time::OffsetDateTime`, see `orbit-state/crates/core/src/verbs.rs:37-38`); `chrono` is not declared in `orbit-state/Cargo.toml`. Pulling in a second date crate alongside `time` is mild bloat and easy to miss in review — and `time::Date::parse_from_str` with a format description can do everything ac-03 needs without the extra dependency.
**Evidence:** `grep -n "chrono" orbit-state/Cargo.toml` returns zero matches. `grep -n "time::" verbs.rs` shows the existing OffsetDateTime/Rfc3339 usage. ac-01 description: "the public verb calls this with `chrono::Local::today().naive_local()`". ac-03 description: "parse the leading 10-char date from the filename via chrono::NaiveDate::parse_from_str(\"%Y-%m-%d\")".
**Recommendation:** Either (a) keep chrono and add it explicitly to `orbit-state/Cargo.toml` (workspace deps) as part of ac-01's plumbing — the spec should call this out so the implementer doesn't view it as scope creep; or (b) reuse the existing `time` crate (`time::Date::current_date()` from `time::OffsetDateTime::now_local().date()`; `time::Date::parse` with a `format_description::parse("[year]-[month]-[day]")` format) and rewrite the chrono references in ac-01/ac-03 accordingly. Either is fine; pick one and name the chosen library on both ACs. Not a verdict-blocker — the implementer can pick at land time — but flagging now avoids a second-pass review when it surfaces in PR.

### [LOW] `Layout::list_memo_files()` does not exist
**Category:** missing-requirement
**Pass:** 1
**Description:** ac-03 says "walks `.orbit/memos/*.md`". The implementation will need either a new `Layout::list_memo_files()` helper or inline filesystem iteration over `layout.memos_dir()`. The spec doesn't name which. Layout already exposes `list_card_files()` and `list_memory_files()` — adding `list_memo_files()` for parity is the cleanest call, and there's no other call site in core that needs it, so it's a small addition rather than a refactor.
**Evidence:** `grep -n "fn list_memo\|fn list_card\|fn list_memory" orbit-state/crates/core/src/layout.rs` shows `list_card_files`, `list_memory_files`, no `list_memo_files`. `layout.memos_dir()` exists.
**Recommendation:** Either explicitly name "add `Layout::list_memo_files()` for parity with the existing scanners" as a sub-step of ac-03, or say "iterate `layout.memos_dir()` directly with `std::fs::read_dir`". One-line spec clarification; not a blocker for the implementer who will encounter the gap immediately on first compile.

### [LOW] Config canonical writer + deny_unknown_fields interaction with new `plugin_version` field
**Category:** failure-mode
**Pass:** 1
**Description:** ac-05 adds `plugin_version: Option<String>` to `Config`. Config currently uses `#[serde(deny_unknown_fields)]` and `Option<DocsConfig>` with `skip_serializing_if = "Option::is_none"` (see `schema.rs:543-549`). Adding a sibling `plugin_version: Option<String>` field with the same serde attributes is mechanical, but ac-05 says "PinState round-trips through Config::from_str and the canonical writer (orbit verify clean after edit)" without spelling out the FIELDS / canonicalisation parity contract. The Config::FIELDS constant pattern (used elsewhere in the codebase for deny-unknown-fields drift detection) is part of the substrate-binary contract — adding a field without also updating FIELDS will silently fail `orbit verify` on every repo with a populated config the moment the binary lands. The implementer needs to update both.
**Evidence:** `schema.rs:556` Config struct currently has only `docs: Option<DocsConfig>`. The pattern across other struct definitions in the file shows FIELDS consts mirroring serde shape (e.g. Card `#[serde(deny_unknown_fields)]`). No FIELDS const is visible for Config itself in the offset I read — but downstream `verify_all` does scan unknown-field drift.
**Recommendation:** Add a one-line note to ac-05 verification: "Update `Config::FIELDS` (or the equivalent deny-unknown-fields registry) to include `plugin_version`, and assert canonicalisation produces byte-identical YAML for a fixture that includes the new field — round-trip via `Config::from_str` → canonical writer → byte-equal." This is implementation hygiene, not a structural concern, but the spec is explicit elsewhere about deny_unknown_fields invariants (ac-01 calls them out for ConformanceFinding) and is silent on this one symmetric case.

### [LOW] ac-08 verification three-phrase check has one residual ambiguity
**Category:** test-gap
**Pass:** 2
**Description:** ac-08 verification says "grep for `orbit audit conformance` AND assert the surrounding prose contains all three documented claims: (a) what the verb does — phrase matching \"workflow conformance\" or \"audit … substrate\"; (b) what it returns — phrase matching \"structured findings\" or \"findings envelope\"; (c) what the agent should do — phrase matching \"remediation\"". "Surrounding prose" is undefined — within the same paragraph, the same section, anywhere in the file? Three separate-paragraph mentions of the verb name with the three phrases scattered across the SKILL would pass a naive grep. The previous review's recommendation to "name what the verb does, what it returns, and how to act on findings" implicitly assumed contiguous prose.
**Evidence:** ac-08 verification text quoted above. No definition of "surrounding".
**Recommendation:** Tighten "surrounding prose" to "within a single section, contiguous with the `orbit audit conformance` mention", or simpler: drop the proximity requirement and just require all four greps (verb name + three phrases) pass against the full file. The latter is mechanical and matches how other doc-type ACs verify in this repo. Either is a small edit. Not a verdict-blocker.

---

## Honest Assessment

The v2 spec resolves every HIGH and MEDIUM finding from v1 and the structural plan is now ready to implement. Concretely: ac-04 contracts to the actually-shipped canonical set (METHOD.md only, with explicit forward-extension via a `CANONICAL_FILES` const); ac-01/ac-06 use the real existing type names (`DriftEntry`, `TopologyDriftEntry`, `AuditDriftResult.drift`, `AuditTopologyResult.topology_drift`) and pin the byte-equal aggregation contract; ac-05 names a concrete version-detection mechanism (`env!("CARGO_PKG_VERSION")` riding the lockstep release contract verified in /orb:release §1.4), makes pin_ahead symmetric with pin_behind on suppression, and adds the round-trip + bump-pin contract; ac-03 specifies the private-helper test-injection seam so AuditConformanceArgs stays empty in v1; ac-08 verification is no longer a single-grep pass. All four v2 LOW findings are implementation-time choices with clear defaults, not structural concerns — chronologically the implementer will hit them on first compile (chrono dep, list_memo_files helper) or first verify run (Config::FIELDS), and the choices are small enough that escalation back to spec edit isn't warranted.

Biggest residual risk: the chrono-vs-time crate choice. If the implementer picks chrono without reading the existing time-crate usage in verbs.rs, the build will compile but the codebase carries two date libraries unnecessarily. Either pre-pick `time` in a 30-second spec edit, or trust the implementer to notice. Acceptable to ship as-is.
