---
date: 2026-05-20
interviewer: Claude Opus 4.7 (rally lead)
card: .orbit/cards/0038-skills-infer-or-prompt-before-halt.yaml
rally: 2026-05-19-agent-side-substrate-engagement-rally
mode: rally-design (decision-pack distillation)
---

# Design: Skills infer or prompt before halting

## Context

Card 0038 was carried into the `agent-side-substrate-engagement` rally alongside siblings 0037 (memory-gates-decisions) and 0042 (act-when-authorised). The shared axis is *the agent's relationship to persistent substrate at the moment it matters*. Card 0038 covers the "consult substrate at skill-entry" surface — every `/orb:*` skill that requires a contextual argument follows the same infer → prompt → halt recovery, never silently stopping on a recoverable missing arg.

The decision pack at `.orbit/specs/2026-05-19-skills-infer-or-prompt-before-halt/decisions.md` framed five decisions. The consolidated decision gate (Stage 3 of the rally) approved all five recommendations verbatim.

## Decisions (approved)

### D1 — Where infer → prompt → halt logic lives

**Decision:** First-class `orbit spec resolve` CLI verb (NOT prose, NOT shell helper).

**Mechanism:** New verb in `orbit-state/crates/core/src/verbs.rs` alongside `spec_list` / `spec_show`. Returns JSON:
- `{resolved: "<id>"}` on success
- `{prompt_with: [<open-spec-ids>]}` when nothing bound and multiple open specs exist
- `Error::unavailable` when both fallbacks fail

Skill still owns the `AskUserQuestion` call (the prompt is an agent action; CLI can't issue it); resolver returns the menu data structurally.

**Rationale:** AC-04's "implementation lives where enforcement is strongest" reads as "in the CLI, not in prose." Matches existing `spec.list` / `spec.show` / `spec.close` pattern. One unit-testable surface in Rust; one-line invocation per skill.

### D2 — How a card-binding resolves to a spec-id

**Decision:** Single-open-spec-per-card rule. If the bound card has exactly one open spec, use it. If zero or multiple, fall through to the prompt branch (lists open specs scoped to the card; falls back to project-wide if card has none).

**Rationale:** Matches `implement/SKILL.md`'s existing single/zero/multi triage. Deterministic semantics that AC-03's halt-only-when-both-fallbacks-fail can sit on. Avoids the "skill auto-picked wrong spec" failure mode AC-03 warns against. When card has multiple opens, the menu is card-narrowed — strictly better than today's project-wide menu. Option 3 (bind a spec, not a card) deferred to a future card if prompt-branch traffic proves painful.

### D3 — Which skills are "affected"

**Decision:** Five spec-id consumer skills: `implement`, `review-pr`, `review-spec`, `audit`, `drive`.

**Rationale:** These share input shape (spec-id-or-equivalent) and substrate (`spec list --status open` + `.session-card` → card → specs). Other arg-taking skills (memo, card, distill, design) have different inference sources (slug, topic, scope) — collapse to AskUserQuestion only, not the three-step recovery. Card's goal text ("typically a spec-id") signals spec-id consumers are the intended set; generalisation is a follow-up card.

### D4 — AskUserQuestion shape

**Decision:** Spec-id + goal one-liner per choice. Extend `spec.list`'s response schema to include `goal_first_line` (or truncated `goal`) so the resolver returns prompt-ready labels.

**Rationale:** Bare spec-id list (D4 option 1) is uninterpretable in a many-spec project; status+age (option 3) over-specifies (status is implicit when listing open-only; age is noisy in a fast-moving codebase). Goal-one-liner is the high-leverage middle. One AskUserQuestion call per skill. Cancel branch maps to AC-03 halt path (terminal "no spec to act on" message).

### D5 — Halt message contract

**Decision:** Two-tier templates: terminal vs recoverable.

- **Terminal** — `.session-card` unbound AND no open specs exist:
  `no spec to act on for /orb:<skill> — both fallbacks failed (.session-card is unbound and no open specs exist). Create one with /orb:spec.`
- **Recoverable** — `.session-card` bound but the card has no open specs:
  `no open spec under the bound card <card-slug>. Create one with /orb:spec, or rebind with orbit session set-card <id>.`

**Rationale:** AC-03's "clear" plus AC-01's "uses the bound spec" together imply the user cares about the bound-card-with-no-open-specs case. Two templates land in the resolver; reused verbatim across all affected skills. Verification: grep that no skill emits a halt message other than these two canonical templates.

## Disjointness map (for rally Stage 4)

**Substrate (Rust):**
- `orbit-state/crates/core/src/verbs.rs` — new `spec_resolve` verb alongside `spec_list` / `spec_show`; `SpecResolveArgs` / `SpecResolveResult` types.
- `orbit-state/crates/core/src/schema.rs` — possible new `goal_first_line` field on `SpecListItem` (if D4 ergonomics require).
- `orbit-state/crates/cli/src/main.rs` — wire `spec resolve` subcommand.

**Skill prose:**
- `plugins/orb/skills/implement/SKILL.md` — replace §"Input contract" branches with `orbit spec resolve` call.
- `plugins/orb/skills/review-pr/SKILL.md` — replace §1 spec-id hard-stop with `orbit spec resolve`.
- `plugins/orb/skills/review-spec/SKILL.md` — same.
- `plugins/orb/skills/audit/SKILL.md` — same.
- `plugins/orb/skills/drive/SKILL.md` — replace §"Input contract" branches with `orbit spec resolve` (preserving the card-path branch and drive-sidecar filter as a wrapper).

**Rally siblings:**
- 0037 — disjoint at file level. `verbs.rs` overlap is at the file level only; the new symbols (`spec_resolve` vs `memory_match`) don't collide. `schema.rs` overlap similar — different new types and fields.
- 0042 — possible light overlap in `drive/SKILL.md`: 0038 edits §"Input contract", 0042 edits §1.6 autonomy prose. Different sections; merge cost is light. Sequence 0038 before 0042 if needed (input-contract is upstream of the autonomy contract).

## Open items

- The exact prose of the two halt-message templates is fixed in this spec; if a literal-text drift test is added, the strings live in a single constant in `verbs.rs`.
- `goal_first_line` shape decision (full goal vs truncated): implementation detail, picked at coding time. The contract is "the menu describes itself."
