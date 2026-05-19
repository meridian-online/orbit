# Conformance audit conflates three card shapes

**Date:** 2026-05-19
**Source:** session triage of 14 planned+empty cards

## Observation

The conformance audit fires on a strict mechanical rule — "card says planned and has zero specs." That single rule catches three different shapes of card:

1. **Already shipped — field is stale.** Cards whose maturity hasn't been updated and whose `specs:[]` array hasn't been populated as specs landed. Example this session: 0020-orbit-state (≥2 specs exist; card says `planned`, specs=[]), 0021-tasks (`orbit task` is documented as the in-session protocol), 0018-two-artefact-contract (largely shipped via substrate hiding agent state).

2. **Deliberately parked.** Cards waiting on an upstream decision, an N=2 evidence threshold, or a cluster synthesis. The `notes:` field carries the rationale but conformance can't read it. Examples this session: 0041-reference-integrity (N=1 hold), 0029-fan-out (pattern works organically, awaits third-use-case forcing), 0037/0038 (substrate-engagement cluster awaiting synthesis), 0010/0011/0012/0015 (external-execution cluster awaiting investment decision).

3. **Genuinely undesigned.** Cards that are warm, unblocked, and ready to take through `/orb:design`. Example this session: 0019-tabletop, 0013-playbook-fast-path, 0014-default-merge-after-review.

All three shapes look identical to the audit. The operator has to do the triage by hand every pass.

## Why this matters

The conformance verb shipped this session (0.4.22) to make workflow drift mechanically visible. It works — it surfaced 14 stale items. But the *signal-to-noise on stale-card findings* is poor because two of the three shapes are not actually drift:

- **Shape 1** (stale field) is genuine drift but the remediation isn't `/orb:design` — it's a one-line maturity bump.
- **Shape 2** (parked) isn't drift at all — it's a deliberate hold. The agent reading the finding doesn't know that without reading the card.
- **Shape 3** (undesigned) is the only one where the `/orb:design <id>` verb the audit prescribes is the correct next action.

Until conformance can distinguish the three, the operator absorbs the cost on every pass.

## What would close it

A "park signal" the conformance verb respects. Candidates:

- **Schema extension:** add an optional `park:` field to the card schema with `reason:` and `until:` (e.g., `until: "N=2 evidence"` or `until: "cluster synthesis"`). Conformance reads it and demotes severity from medium to info, or skips the finding entirely.
- **Maturity vocabulary extension:** add `parked` as a maturity value between `planned` and `emerging`. Cards in `parked` state don't trigger the planned+empty-specs finding.
- **Notes parsing convention:** require a structured first line in `notes:` (e.g., `PARKED: <reason>`) that conformance reads. Cheaper but more fragile.
- **Specs-array auto-population:** for shape 1, fix the upstream — the verbs that close specs should write to the card's `specs:[]` array. Removes the need for manual hygiene. Doesn't address shape 2.

## What we did this session

Triaged 14 cards by hand into the three shapes (plus a fourth: design-now candidate). Recommendation captured in session-close. Not opening a card yet — this is a workflow refinement that benefits from one more conformance pass to confirm the pattern repeats.

## Status

Memo only. Surface again after the next conformance pass — if shape 1 keeps producing the same false-positive shape, this becomes card-shaped.
