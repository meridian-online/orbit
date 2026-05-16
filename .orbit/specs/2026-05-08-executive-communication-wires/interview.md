# Design: Executive-communication wires — STYLE.md and skill citations

**Date:** 2026-05-08
**Interviewer:** Claude Opus 4.7 (orbit /orb:design)
**Card:** .orbit/cards/0026-executive-communication.yaml

---

## Context

Card *Executive communication — BLUF-led Decision Brief as agent output contract* — 9 scenarios (4 gates), goal: agent-to-Hugh prose is always BLUF-led, recommendation-driven, anti-pattern-free.

Prior specs: 0. The card is fully specified but no work has shipped to wire it; `specs: []` is the canonical aspirational-card signal under choice 0019 (amended 2026-05-08). This design session scopes the first live wire.

Gap: contract lives in card 0026; nothing in the substrate enforces it. Card scenario 9 names three expected wires — CLAUDE.md preamble, orb-skill SKILL.md citations, /orb:audit anti-pattern column. The first spec covers the first two.

Constraint surfaced during evidence review: Claude Code plugins do not auto-inject CLAUDE.md content. Load-bearing surfaces are project CLAUDE.md, user CLAUDE.md, and skill SKILL.md prompts.

## Q&A

### Q1: Spec scope
**Q:** What does the first spec cover?
**A:** Preamble + orb-skill citations, structured around a new `.orbit/STYLE.md` file imported via `@` reference from CLAUDE.md (per Anthropic's rich-content best practice — https://code.claude.com/docs/en/best-practices#provide-rich-content). The audit anti-pattern column is deferred to a later spec.

### Q2: Preamble home
**Q:** Where does the BLUF preamble live?
**A:** Resolved by Q1's structure — STYLE.md at `.orbit/STYLE.md` in this repo, imported from project CLAUDE.md. Cross-project reach (user CLAUDE.md, every project) is deferred to a future spec.

### Q3: Enforcement strength
**Q:** How strict is enforcement in the first spec?
**A:** Directive only — preamble + citations tell agents the contract; no hooks, no audit. Pillar 3 working at the soft, context-loaded end.

### Q4: Skill citation pattern
**Q:** How should the orb skills cite STYLE.md?
**A:** Both — `@` import plus a one-line prose marker in each named skill body. Belt-and-braces against `@` resolution failures.

### Q5: STYLE.md density
**Q:** How dense is STYLE.md?
**A:** Distilled checklist — tight prose, one-liner per anti-pattern, short variant table, BLUF / Decision Brief skeleton, tone contract. Token cost low, signal density high.

---

## Summary

### Goal

Land the first live wires for card 0026 so the BLUF / Decision Brief contract is loaded into every orbit-repo session by default, not dependent on the agent happening to read the card. Closes the canonical aspirational-card example named in choice 0019.

### Constraints

- Plugins cannot inject CLAUDE.md content; the wire must live in files the harness loads (project CLAUDE.md and skill SKILL.md prompts).
- The card's full prose is too large to load into every session verbatim — STYLE.md is a tight distillation, not a copy.
- `@` import resolution from plugin SKILL.md files is unverified — the spec must check at implementation; if it doesn't resolve, fall back to explicit-prose citation in skill prompts (the one-line marker remains either way).
- Universal-contract intent (every project Hugh opens, not just orbit) is *not* delivered by this spec — only orbit-repo sessions will load STYLE.md. Recorded as open question for a follow-up spec.

### Success Criteria

- `.orbit/STYLE.md` exists and contains a distilled BLUF / Decision Brief checklist: TL;DR contract, recommendation discipline, the seven anti-patterns by name with one-liner each, response-variant table (decision / status / factual / research), tone contract.
- Project `CLAUDE.md` imports STYLE.md via `@.orbit/STYLE.md`.
- `/orb:design`, `/orb:review-spec`, `/orb:review-pr` SKILL.md files each carry an `@` import and a one-line prose marker citing card 0026 + STYLE.md.
- Card 0026's `specs:` array references this spec — the audit signal flips from aspirational to wired.
- A spot-check session in this repo confirms STYLE.md is loaded into context (e.g., `cat CLAUDE.md` shows the import; an opening session reflects the contract).

### Decisions Surfaced

- **STYLE.md as canonical contract source.** File at `.orbit/STYLE.md`, imported via `@` from CLAUDE.md and the named orb skill prompts. Alternative considered: lift card prose into the preamble inline. Rationale: single source of truth, lower drift risk, follows Anthropic rich-content guidance. Likely warrants a `/orb:choice` MADR record at spec time.
- **Belt-and-braces citation pattern.** `@` import for the substantive content + one-line prose marker for robustness against `@` resolution failures across surfaces.
- **Directive-only enforcement.** No hooks or audit checks in this spec. Substrate enforcement (anti-pattern detection on response output) is a separate, later spec under card 0026.

### Implementation Notes

- The card's `references:` list cites `.orbit/memos/2026-05-06-executive-communication-framework.md`, which has been deleted (normal post-distill state). The implementing spec should either remove this stale reference or annotate it as historical.
- The card also references `ops decision 0029 (recommendation discipline)` — a pointer into Hugh's private ops repo. orbit is a public repo; the bare reference is unlikely to expose anything sensitive but the implementing agent should sanity-check before keeping it as-is.
- Verify `@` import semantics from plugin SKILL.md files (not just CLAUDE.md). If unsupported, fall back to explicit prose citation in skill prompts; STYLE.md remains the canonical source via project CLAUDE.md.
- Skills named in card scenario 9: `/orb:design` and `/orb:review-*`. Treat that as the starting set; broader skill coverage (e.g., `/orb:discovery`, `/orb:card`) is a follow-up.
- STYLE.md compression target: aim for a single screen of dense prose. The card has nine scenarios; STYLE.md should distil their gate behaviours, not transcribe them.
- After this spec ships, card 0026's `specs:` array gains its first entry — when the `/orb:audit` aspirational-card column lands (separate spec), the audit will see card 0026 as wired.

### Open Questions

- **Cross-project reach.** STYLE.md at `.orbit/STYLE.md` only loads in this repo. Card 0026's goal says "*always* BLUF-led", not orbit-only. Future spec to consider: promote STYLE.md to a user-CLAUDE-importable location, distribute via the orbit plugin's installation, or both.
- **Substrate enforcement.** Anti-pattern detection (lede-burying, hedge-stacking, etc.) is a separate spec when/if Claude Code's hook surface supports post-response scanning. Out of scope for this spec; flagged for the audit-column spec to coordinate with.

---

**Next step:** `/orb:spec` to generate a structured specification from this design session.
