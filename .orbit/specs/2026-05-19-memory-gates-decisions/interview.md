---
date: 2026-05-20
interviewer: Claude Opus 4.7 (rally lead)
card: .orbit/cards/0037-memory-gates-decisions.yaml
rally: 2026-05-19-agent-side-substrate-engagement-rally
mode: rally-design (decision-pack distillation)
---

# Design: Memory gates decisions

## Context

Card 0037 was carried into the `agent-side-substrate-engagement` rally alongside siblings 0038 (skills-infer-or-prompt-before-halt) and 0042 (act-when-authorised). The shared axis is *the agent's relationship to persistent substrate at the moment it matters*. Card 0037 covers the "consult memory at decision-time" surface.

The decision pack at `.orbit/specs/2026-05-19-memory-gates-decisions/decisions.md` framed five decisions. The consolidated decision gate (Stage 3 of the rally) approved all five recommendations verbatim.

## Decisions (approved)

### D1 — Matching surface

**Decision:** Add a new `memory.match` verb (NOT extend `memory.search`).

**Shape:**
```rust
pub struct MemoryMatchArgs {
    pub topic: String,
    #[serde(default)] pub labels: Vec<String>,
    #[serde(default = "default_match_limit")] pub limit: usize, // default 10
}
pub struct MemoryMatchResult { pub matches: Vec<MemoryMatch>, }
pub struct MemoryMatch {
    pub memory: Memory,
    pub score: f32,
    pub reason: String,
}
```

**Rationale:** Distinct semantic from operator-keyword `memory.search`; absorbs future ranking work behind a stable name. v1 ranker is `token-overlap(body) + 2 * label-overlap(labels)`, normalised. Cheap, files-canonical, inspectable in `git diff`.

### D2 — Where design-time surfacing lives

**Decision:** SKILL.md prose call + close-time structural gate (D2a + D4 enforcement seam). NOT a new composite verb.

**Mechanism:** `/orb:design` SKILL.md §2 calls `orbit memory match <card-slug>` as a non-optional evidence-load step. The structural gate at D4 is the enforcement; the design-time call is the encouragement.

**Rationale:** Per the card's ac-06 ("skill-prompt-only enforcement is insufficient"), the substrate must hold the load. Pairing the design-time surfacing (encouragement) with the close-time block (enforcement) matches the card's stated intent directly. D2b (composite verb) is over-engineering for one caller; D2c (extending `card.show`) contaminates a read-only verb.

### D3 — Where `memories_considered` lives on Spec

**Decision:** Top-level `memories_considered: Vec<MemoryReconciliation>` on the `Spec` struct.

**Shape:**
```rust
pub struct Spec {
    // ...existing fields...
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub memories_considered: Vec<MemoryReconciliation>,  // NEW
}
pub struct MemoryReconciliation {
    pub key: String,
    pub disposition: ReconciliationDisposition,
    pub reason: String,
}
#[serde(rename_all = "kebab-case")]
pub enum ReconciliationDisposition {
    Adopted, PartiallyAdopted, NotApplicable,
}
```

**Rationale:** Spec-scoped (not AC-scoped) per card framing. `skip_serializing_if` keeps existing specs byte-identical when no memories matched. Extend `Spec::FIELDS` const and `spec_fields_matches_struct` drift test. Schema-version bumps minor (additive).

### D4 — How `spec.close` decides "unreconciled"

**Decision:** Match by `spec.goal + spec.cards` using the D1 ranker, score threshold default `0.3`, `--force` bypass mirroring the existing AC pre-flight.

**Mechanism:**
1. After AC pre-flight (`verbs.rs:1597-1642`), before unfinished-tasks check, invoke `memory.match(topic=spec.goal, labels=spec.cards)`.
2. Filter to `score >= MEMORY_MATCH_THRESHOLD` (constant `0.3`).
3. Build unreconciled set: matching keys absent from `spec.memories_considered`.
4. If unreconciled is non-empty AND `--force` not passed → `Error::conflict` naming keys.
5. `--force` bypasses; recorded as `forced_unreconciled: Vec<String>` in response (parallel to `forced_unchecked`).

**Rationale:** Wires into the same control surface as existing AC pre-flight — one code path, one test pattern. Project-wide threshold (not per-spec) keeps policy uniform; tuning is substrate change, not per-spec choice.

### D5 — Mechanism-over-state enforcement

**Decision:** Warn at `memory.remember` (NOT block); mirror the topology nudge pattern with `--no-warn` flag.

**Shape:**
- Extend `MemoryRememberResult` with `shape_warning: Option<String>`.
- Extend `MemoryRememberArgs` with `no_warn: bool`.
- Detection: small heuristic on body's first sentence (leading state-verb patterns: "X is …", "the problem is …", "Y proved difficult").
- Warning text: `memory body leads with state ('X is …'); decision-moment surfacing works better when the body leads with mechanism ('use X for Y', 'prefer X when Y'). Consider rephrasing — the memory is stored as written.`

**Rationale:** D5a (block) is too false-positive-prone (legitimate "FineType is uv-based" gets rejected); D5c (audit-only) disconnects the agent from the moment of authoring. D5b matches the proven topology-nudge precedent — already-shipped pattern, already-tested skip-on-default discipline.

## Disjointness map (for rally Stage 4)

**Substrate (Rust):**
- `orbit-state/crates/core/src/verbs.rs` — new `memory_match` + `MemoryMatch*` types; `spec_close` modification (memory-reconciliation block); `memory_remember` modification (shape_warning heuristic); new `MEMORY_MATCH_THRESHOLD` constant.
- `orbit-state/crates/core/src/schema.rs` — `Spec.memories_considered` field; `Spec::FIELDS` extension; drift-test fixture update; new `MemoryReconciliation` + `ReconciliationDisposition`; `MemoryRememberArgs.no_warn`; `MemoryRememberResult.shape_warning`.
- `orbit-state/crates/cli/src/main.rs` — wire `memory match` subcommand.
- `orbit-state/crates/mcp/src/main.rs` — register `memory.match` verb.
- `.orbit/schema-version` — minor bump (additive).

**Skill prose:**
- `plugins/orb/skills/design/SKILL.md` — §2 evidence-load: add `orbit memory match <card-slug>` call.
- `plugins/orb/skills/spec/SKILL.md` — add §"Record memories considered".

**Documentation:**
- `plugins/orb/PRIME.md` — add `orbit memory match` to Decisions section.

**Rally siblings:**
- 0038 — disjoint at file level (touches `spec_resolve` verb, different `verbs.rs` area; different SKILL.md set).
- 0042 — consumes `memory.match` (read-only); ordering: 0037 ships before 0042. No file overlap.

## Open items

- D1 score threshold `0.3` is a project-wide constant; reasonable default, tunable in follow-up if conformance audit shows the gate firing too noisily.
- D5 state-shape heuristic regex set is an implementation detail the implementing agent picks; the contract is fixed (warn, don't block; suggested rephrase; `--no-warn`).
