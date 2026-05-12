# Design: reconcile mode — adjusting legacy YAML to the canonical schema

**Date:** 2026-05-12
**Interviewer:** Claude (Opus 4.7) via /orb:design
**Card:** .orbit/cards/0032-brownfield-spec-migration.yaml
**Mode:** open

---

## What good looks like

When I bring orbit into a research project I've already started, I shouldn't have to hand-rewrite five spec files to get past ac-01. One verb invocation looks at every legacy `spec.yaml` in the tree, tells me up front what it's going to do — map this, drop that, quarantine the rest — and only rewrites when I say go. Fields the registry doesn't recognise land in a sidecar next to the spec rather than getting silently destroyed; I can come back and map them later. Once the run finishes, `orbit verify` is clean and nothing in day-to-day work has changed — the verb is a one-time on-ramp, not a permissive escape hatch baked into routine paths.

---

## Context

Card: *Brownfield spec migration* — 7 scenarios (4 gating), goal: a project with N legacy spec.yaml files reaches `orbit verify` clean in one verb invocation.
Prior specs: 0 — this is the first spec against card 0032.
Gap: the detection half shipped in 0.4.11 (`orbit audit drift` reports unknown fields across Card / Spec / Choice / Memory via `FIELDS` constants in `schema.rs`). The fix half — applying dispositions to those unknown fields — is what this spec closes.

## Q&A

### Q1: Verb scope — one-shot or reusable?
**Q:** Should `orbit spec migrate-fields` be a one-shot onboarding verb (run once when a project adopts orbit), or a reusable brownfield verb invokable any time strict parse blocks legacy content?
**A:** Reusable. Author reframed: "migration" isn't the right way of thinking — it's more like *adjusting to the spec*. The verb is continuous reconciliation, not a one-time bridge that ages out.

### Q2: Surface — new verb or mode of canonicalise?
**Q:** Given the "adjust to the canonical schema" framing, should this land as a new top-level verb, or as a mode of the existing `orbit canonicalise`?
**A:** Mode on canonicalise. One unified "make this file conform to canonical" hammer. The verb name becomes `orbit canonicalise --reconcile` (exact flag name an implementation detail).

### Q3: Entity scope — spec only or all four?
**Q:** `canonicalise` already operates across Card / Spec / Choice / Memory. Should `--reconcile` mirror that full reach from day one, or start spec-only since that's where the downstream pain was?
**A:** Mirror canonicalise. Registry handles unknown-field disposition for all four entity types from v1. Symmetric with `audit drift`'s existing reach via `FIELDS` constants.

### Q4: Sidecar lifecycle — write-only or re-merge?
**Q:** When `--reconcile` quarantines an unknown field to a sidecar, what's the verb's responsibility for that sidecar afterwards?
**A:** Write-only safety net. Verb writes the sidecar on first encounter and leaves it alone. Contributors resolve unknown fields by teaching the registry a mapping rule; subsequent runs drain the sidecar by applying the new rule. No re-merge verb.

### Q5: Registry extensibility — day-one or follow-up?
**Q:** Should the mapping registry be project-extensible from day one, or default-only first with project-local extension deferred to a follow-up spec?
**A:** Default-only first. Ship a built-in registry of known mappings. Project-local override file (card scenario 6, non-gate) is deferred to a follow-up spec once there's a second-project demand signal.

---

## Summary

### Goal

A project with N legacy YAML files (across Card / Spec / Choice / Memory) reaches `orbit verify` clean by running `orbit canonicalise --reconcile` once. Unknown fields are mapped onto canonical equivalents, dropped per the registry, or quarantined into a per-file sidecar (`<name>.legacy.yaml`) so semantic content is never silently lost. The ac-01 strict-parse invariant in `orbit-state/crates/core/src/schema.rs` is unchanged — the permissive read happens only inside `--reconcile`'s code path, never in routine parsing.

### Constraints

- **ac-01 preserved**: `deny_unknown_fields` stays on every canonical schema struct. Permissive reads live only inside the reconcile code path.
- **Not part of routine verify or hooks**: `--reconcile` is invoked deliberately. `orbit verify`, pre-commit hooks, and `/orb:setup`'s greenfield path do not invoke it.
- **Reusable, idempotent**: a clean tree → no-op. A tree with quarantined sidecars and no new registry rules → no-op. No "already-migrated" completion marker.
- **Cross-entity from day one**: registry covers Card / Spec / Choice / Memory unknown-field dispositions.
- **Sidecar is safety, not workflow**: contributors close the loop by teaching the registry a mapping rule, not by hand-editing the sidecar.

### Success Criteria

- Running `orbit canonicalise --reconcile --dry-run` on a tree with unknown fields lists every unknown field by file path and proposed disposition (map / drop / quarantine). Exit code signals whether a non-dry-run would write.
- Running `orbit canonicalise --reconcile` (without `--dry-run`) applies the dispositions, writes any quarantine sidecars, and rewrites canonical files via the existing canonical writer.
- Post-run `orbit verify` is clean.
- Re-running `orbit canonicalise --reconcile` on the post-run tree is a no-op.
- A spec.yaml whose unknown field was quarantined has a sibling `spec.legacy.yaml` containing that field's content (yaml structure preserved).
- `orbit verify` continues to fail strictly on unknown fields when invoked without `--reconcile` — no permissive escape hatch persists in routine paths.

### Decisions Surfaced

- **Framing: "reconcile" / "adjust", not "migrate"** — verb is continuous reconciliation invokable any time the canonical schema gets ahead of legacy content; not a one-time bridge. (Q1)
- **Surface: mode on `canonicalise`, not a new verb** — extends the existing "make this file conform to canonical" hammer rather than introducing a parallel verb. Card scenarios that reference `orbit spec migrate-fields` will be reworded against `orbit canonicalise --reconcile` in the spec. (Q2) → candidate for a new MADR choice file (0023 next).
- **Entity scope: cross-entity from v1** — Card / Spec / Choice / Memory all covered. Registry is a Rust constant keyed by `(entity_type, field_name)`. (Q3)
- **Sidecar is write-only safety net** — no re-merge verb in v1. Closing the loop = teach the registry. (Q4)
- **Registry extensibility deferred** — card scenario 6 (non-gate) explicitly deferred to a follow-up spec once second-project demand surfaces. (Q5)

### Implementation Notes

- New module `orbit-state/crates/core/src/reconcile.rs` is the natural sibling to `migrate.rs` (layout migrations), `migrations.rs` (schema-version migrations), and `canonicalise.rs` (byte-drift fix). `--reconcile` short-circuits inside `canonicalise.rs`'s entry point and delegates to `reconcile.rs` for unknown-field disposition before handing the cleaned struct to the canonical writer.
- The permissive read uses `serde_yaml::Value` (matches `audit drift`'s pattern in `verbs.rs`). After classifying each top-level key against `Card::FIELDS` / `Spec::FIELDS` / `Choice::FIELDS` / `Memory::FIELDS`, the verb applies the rule: known → pass through; mapped → rewrite; dropped → record; unknown-without-rule → write to sidecar.
- The default mapping registry is a Rust constant inside `reconcile.rs`, e.g. `pub const FIELD_RULES: &[(EntityType, &str, Disposition)] = &[...]`. Per Q5, no file-shape registry yet. The known-mapping entries seed from the downstream research project's five fields (`version`, `date_opened`, `predecessor_evidence`, `constraints`, `exit_conditions`) and from any bd-era leftovers visible in `.orbit/archive/`.
- Sidecar shape: `<name>.legacy.yaml` adjacent to each entity. For specs that's `.orbit/specs/<id>/spec.legacy.yaml`; for cards `.orbit/cards/<slug>.legacy.yaml`. The naming convention generalises across entities.
- Idempotency: when the permissive read finds no unknown fields and no canonicalise-style byte drift, the verb is a no-op. When unknown fields are present but every one has a registry rule, the verb rewrites once and subsequent runs are no-ops. When a sidecar already exists with content matching what would be re-quarantined, the verb does not rewrite the sidecar.
- `--dry-run` mirrors `audit drift`'s existing dry-run shape: same JSON envelope, same per-file disposition listing, exit-code-signals-would-change semantics.
- Card scenario 7 (gating — "Wired into the framework") requires (a) `/orb:setup`'s brownfield path to invoke or instruct the agent to invoke `orbit canonicalise --reconcile`, and (b) card 0030 (canonical-schema-and-glossary) to name `--reconcile` as the on-ramp from legacy field shapes. Both are documentation/skill-prose edits.
- Card scenarios that mention `orbit spec migrate-fields` (scenarios 1, 5, 7) will be reworded against `orbit canonicalise --reconcile` in the spec; the card stays a user-facing capability statement and is not load-bearing for the verb name.
- Choice file 0023 (next available number) recording "field-shape reconciliation as a canonicalise mode, not a separate verb" — alternatives considered: separate verb, new verb wrapping canonicalise. To be authored as a MADR record once the spec is in.

### Open Questions

None — design space settled.
