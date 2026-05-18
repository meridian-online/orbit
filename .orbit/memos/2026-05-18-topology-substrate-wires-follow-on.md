# Memo — topology-substrate-wires follow-on tightening

**Date:** 2026-05-18
**Context:** Spec 2026-05-18-topology-substrate-wires shipped APPROVE on review-pr. Two non-blocking findings were captured at close. This memo records them as candidates for a future tightening pass.

## Findings carried forward

### MEDIUM — MCP parity tests for ac-02 / ac-03 / ac-04

Each AC's verification line explicitly calls for *"Parity test on CLI + MCP"*. The shipped implementation has:

- CLI parity tests in `orbit-state/crates/cli/tests/parity.rs` covering all four envelope states for session.prime, the populated case for spec.close, and the human/--json render channels + suppression flag for memory.remember.
- MCP-side: only the canonical-envelope shims in `orbit-state/crates/mcp/tests/common/mod.rs` (the `topology_warnings: vec![]` additions to keep existing spec.close fixtures green).

Recommendation: 5 mirror tests in the MCP parity surface — 4 for session.prime states (configured / not / docs.topology-absent / drift-present), 1 for spec.close topology_warnings populated, 3 for memory.remember nudge (present / absent / suppressed). The CLI parity tests are the template — mirror the JSON-RPC dispatch pattern from `mcp/tests/parity.rs` and assert the same envelope shape.

Reviewer's note: "APPROVE rather than BLOCK because the core unit-test coverage is solid (13 tests on verbs.rs) and the MCP transport is a thin wrapper." Implementation-level risk is low; the gap is a verification-language adherence issue, not a behavioural one.

### LOW — ac-01 has no automated test bearing the AC identifier

`plugins/orb/scripts/setup-topology.sh` was smoke-tested manually (greenfield, brownfield-decline, brownfield-accept with no existing file, brownfield-accept with existing file, nested target path, idempotent re-run — all 6 pass during implementation). No automated test exists with the `ac01_` or `setup_topology_` prefix.

Recommendation: add a bats-style shell test or Rust integration test that drives the script across the 6 fixture states. The smoke script in the cycle-1 implementation transcript is a usable template.

## What was shipped (for traceability)

- ac-01 — `/orb:setup` integration (script + skill §6d)
- ac-02 — `SessionPrimeResult.topology_drift: Option<Vec<TopologyDriftEntry>>` + audit_topology call in session_prime
- ac-03 — `SpecCloseResult.topology_warnings: Vec<TopologyDriftEntry>` + word-boundary heuristic with regex::escape on subsystem names
- ac-04 — `MemoryRememberArgs.no_nudge: bool` + `MemoryRememberResult.nudge: Option<String>` + CLI `--no-nudge` flag + stderr render

Test counts at close: 359 passing in the workspace (regex crate added at workspace + core, version 1.10). Build clean. No new clippy warnings.

ac-05 is `ac_type: observation` (4-week audit window, anchor 2026-06-15) — not addressed here; deferred per ac-taxonomy.

## Cluster context

This is the second concrete instance in the agent-side substrate-engagement cluster (parent spec 2026-05-18-documentation-topology being the first). No synthesis card opened yet — the cluster sibling memo flagged this as "worth watching, not opening preemptively".
