# Changelog

All notable changes to orbit are documented here. Format follows [Keep a Changelog](https://keepachangelog.com/).

## [0.4.12] - 2026-05-13

Reconcile mode shipped — `orbit canonicalise --reconcile` is the on-ramp from legacy yaml field shapes to the canonical schema. A permissive read lives in a new `reconcile.rs` module gated behind the flag; every schema struct keeps `deny_unknown_fields`, so routine paths (`orbit verify`, `orbit canonicalise` without `--reconcile`, every other verb) stay strict. Closes spec `2026-05-12-reconcile-mode` (card 0032).

The change is forward-compatible for routine work — only invoking `--reconcile` itself requires the 0.4.12 binary. `/orb:setup`'s brownfield path is the only routine consumer (via the new §3g step, gated on `orbit audit drift` reporting drift).

### Added

- `orbit canonicalise --reconcile` — permissive pass that walks the substrate, applies dispositions from a built-in registry (`map` renames a field, `drop` removes it), and quarantines unknown content into a sibling `<name>.legacy.yaml` sidecar so semantic content is never silently destroyed. Combined with `--dry-run` it lists every disposition and exits non-zero when the tree is not clean — useful as a CI gate.
- `dispositions: [{path, kind, field, action}]` array on the canonicalise JSON envelope (only present in reconcile mode). Each entry names the file, entity kind, structural field path (e.g. `acceptance_criteria[2].ac_type`), and action (`map` / `drop` / `quarantine`).
- `AcceptanceCriterion::FIELDS`, `Scenario::FIELDS`, `Relation::FIELDS` — inner-shape field-name constants. Reconcile uses them to classify legacy fields inside lists-of-struct; lockstep unit tests keep each constant in sync with its struct.
- `/orb:setup` brownfield path gains §3g — after the layout migration completes, it runs `orbit audit drift` and offers `orbit canonicalise --reconcile --dry-run` → confirm → apply when drift is non-empty. Greenfield setup, `orbit verify`, and pre-commit hooks never invoke reconcile.
- Choice `0023-reconcile-as-canonicalise-mode` — MADR record of the surface decision (mode on `canonicalise` vs a separate verb).

### Changed

- Card 0030 (canonical-schema-and-glossary) names `orbit canonicalise --reconcile` as the on-ramp from legacy field shapes.
- Card 0032 (brownfield-spec-migration) reworded against the new mode; `specs[]` references this spec.

## [0.4.11] - 2026-05-12

Tree-views shipped — five new read-only navigation and synthesis verbs make the substrate's shape legible from the CLI and MCP without opening a single YAML file. Closes spec `2026-05-12-tree-views` (cards 0033, 0020). Surfacing wires land alongside the verbs so agents discover them at the right pipeline moments.

### Added

- `orbit card tree <id>` — local relations subgraph, depth-bounded, cycle-safe. Renders the cards/choices/specs/memories adjacent to a card so a session-start agent can see context without paging through files.
- `orbit card specs <id>` — bidirectional drift detection on `card.specs[]` against `spec.cards[]`. Surfaces orphaned refs in either direction.
- `orbit overview` — single-screen project synthesis: open specs, cards by maturity, recent memories, most-connected card, orphan cards. Bounded output regardless of project age.
- `orbit graph [--format mermaid|graphviz]` — renders the full cards-specs graph to stdout, pasteable into markdown or a renderer.
- `orbit audit drift` — permissive YAML scan against the canonical `Card` / `Spec` / `Choice` / `Memory` schemas. Surfaces unknown fields, missing required fields, and type mismatches that the canonical writer would silently rewrite.
- `Card::FIELDS`, `Spec::FIELDS`, `Choice::FIELDS`, `Memory::FIELDS` — public field-name constants on each schema type, the load-bearing surface that `orbit audit drift` checks against.
- `session.prime` gains a `next_step` field pointing at `orbit overview` so the very first verb after session start surfaces the substrate's shape.
- `/orb:card` SKILL §4 suggests `orbit card tree` after authoring; `/orb:distill` SKILL §2 directs to `overview` + `card tree` *before* drafting.

### Changed

- Wire envelope error coverage extended — every new verb's failure modes round-trip through the canonical `{ ok: false, error: { code, message } }` envelope shape with CLI ↔ MCP parity.

## [0.4.10] - 2026-05-11

Spec layout reverts to per-spec folders per choice 0021. `.orbit/specs/<id>.yaml + <id>.<sidecar>` becomes `.orbit/specs/<id>/spec.yaml + <id>/<sidecar>` across the substrate, the canonical writer, and every SKILL.md path string. Closes spec `2026-05-10-spec-folders-migration` (cards 0008).

The new `list_spec_files` walks immediate subdirectories of `.orbit/specs/` and returns every `<id>/spec.yaml`. As a side-effect it surfaced 19 bd-era specs that the previous flat scanner was silently skipping; those folders moved to `.orbit/archive/specs/` (no schema migration — the bd-era `constraints` / `values` fields are out of orbit-state v0.1's Spec schema). Card refs to those archived specs were rewritten to `.orbit/archive/specs/<id>/...`.

**Forward-incompatible layout change** — the parity gate fires. The 0.4.10 binary expects folder-shape; the 0.4.9 binary reads zero specs against the new layout.

### Added

- `OrbitLayout::spec_dir(id)` and `ensure_spec_dir(id)` helpers — callers writing per-spec files (spec.yaml, tasks.jsonl, notes.jsonl, sidecars) ensure the folder exists before invoking `write_atomic` / `append_jsonl_line`.
- `.orbit/archive/specs/` — quarantine destination for the 20 pre-orbit-state-v0.1 bd-era folders that don't parse against the current Spec schema.

### Changed

- `OrbitLayout::spec_file(id)` now returns `<root>/specs/<id>/spec.yaml`; `task_stream(id)` and `notes_stream(id)` now return `<id>/tasks.jsonl` and `<id>/notes.jsonl`. `list_spec_files` scans subdirectories.
- `spec.close` writes `.orbit/specs/<id>/spec.yaml` into linked-card `specs` arrays (was `<id>.yaml`). Existing card refs were updated for the post-migration specs and the archived bd-era specs in the same release.
- `.orbit/conventions/spec-layout.md` rewritten — folder shape canonical, flat sidecar layout named as the prior experiment with rationale (visual mess, prefix collision, non-atomic rename).
- `.orbit/METHOD.md` (and the byte-mirror at `plugins/orb/skills/setup/METHOD.md`) — vocabulary table Spec / Interview / Review / Drive state / Rally state rows updated to folder paths.
- SKILL.md sweep across `drive`, `rally`, `review-spec`, `review-pr`, `setup` — every cited sidecar path reverts from `<id>.<sidecar>` to `<id>/<sidecar>`.

## [0.4.9] - 2026-05-10

`Card` gains an explicit `id:` field; `orbit card show` and `orbit choice show` accept bare `NNNN` shorthand. The substrate's id conventions are documented as three families (enumerated for cards/choices, dated for specs, keyed for memories). Choices `0021-spec-folders` (per-spec folders revert) and `0022-entity-id-conventions` (id heterogeneity) are accepted; their migration specs open against cards 0008 and 0030.

### Added

- `Card.id: Option<String>` as the first field in the schema. Parsers accept legacy id-less yaml; the canonical writer fills `id` from the filename on the next canonicalise pass and rejects yaml whose `id` disagrees with its filename. One-shot pass over `.orbit/cards/` populated 31 existing cards.
- `resolve_numeric_slug` in `orbit-state/crates/core/src/verbs.rs` — `orbit card show 8` and `orbit choice show 21` resolve via filename prefix-match. Errors: zero matches → `not-found`; multiple matches → `ambiguous`. Six unit tests cover the resolver.
- `.orbit/conventions/id-conventions.md` — documents the three id-shape families, per-entity yaml field conventions, the type-qualifier prose contract, and CLI lookup forms.
- Choices `0021-spec-folders.yaml` (revert flat-sidecar specs to per-spec folders; supersedes the file-shape decision in the 2026-05-09 sidecar migration) and `0022-entity-id-conventions.yaml` (formalise the three id-shape families).
- Specs `2026-05-10-spec-folders-migration.yaml` (8 ACs, 6 gating) and `2026-05-10-card-id-field-and-conventions.yaml` (7 ACs, 5 gating) — open, ready for drive.
- README gains a `## Repository layout` section signposting the four top-level directories.

### Changed

- `.orbit/METHOD.md` and `plugins/orb/skills/setup/METHOD.md` — vocabulary table gains an Id-shape column; new Memory row; new Reference style section names the type-qualifier contract and bare-NNNN shorthand. Files stay byte-equal.
- `orbit-state` workspace version aligns with plugin (0.4.3 → 0.4.9). Substrate-binary parity gate now passes for terminals running the 0.4.9 binary against the new card schema.

### Fixed

- Spec `2026-05-10-repo-cruft-removal` shipped — `.beads-archive/` and the empty `.claude/worktrees/` removed from the working tree.

### Removed

- `.beads-archive/` (gitignored archived bd state, no longer needed) and `.claude/worktrees/` (empty stale runtime dir).

## [0.4.8] - 2026-05-10

`/orb:release` gains a substrate-binary parity gate. When `orbit-state/` changed in the release window but the on-PATH `orbit` binary predates the change, release refuses with a three-option resolution path (rebuild formula, set `ORBIT` env, or explicit `--accept-binary-lag` for forward-compatible changes). Closes the defect from 0.4.7 — sidecar-aware skill prose shipped against an older binary, which broke `orbit verify` for any terminal still on brew 0.4.3.

### Changed

- `plugins/orb/skills/release/SKILL.md` — pre-flight §1 gains step 4: substrate-binary parity gate. §7 confirm output now restates the binary state explicitly (resolved path, or "not gated" when orbit-state was untouched in this window).

## [0.4.7] - 2026-05-09

The bd-era folder layout for per-spec sidecars (drive.yaml, rally.yaml, review files) migrates to flat sidecar paths (`.orbit/specs/<id>.<file>`) — one substrate convention across drives, rallies, and reviews. The orbit-state scanner gains a dotless-stem filter so `<id>.drive.yaml` and `<id>.rally.yaml` are skipped during spec parsing; `orbit verify` and `orbit spec list` stay clean with sidecars on disk.

### Added

- `.orbit/conventions/spec-layout.md` — canonical sidecar inventory naming every per-spec sidecar shape (`<id>.yaml`, `<id>.tasks.jsonl`, `<id>.notes.jsonl`, `<id>.drive.yaml`, `<id>.rally.yaml`, `<id>.decisions.md`, `<id>.interview.md`, `<id>.review-{spec,pr}-<date>.md` with `-v2`/`-v3` cycle suffixes). The bd-era folder layout is named explicitly as deprecated.
- `plugins/orb/scripts/tests/test-sidecar-layout.sh` — five-step smoke test against a temp `--root`: promote produces flat spec, drive sidecar reachable via `[[ -f *.drive.yaml ]]`, rally sidecar reached via `*.rally.yaml` glob, `orbit verify` clean, `orbit spec list` excludes sidecar ids.
- Two unit tests in `orbit-state-core` pin the scanner-fix contract: `list_spec_files_skips_sidecar_shapes` (layout) and `verify_excludes_sidecar_yaml_shapes` (verify).

### Changed

- `orbit-state/crates/core/src/layout.rs` — `list_yaml_files` filters spec YAML loads to dotless-stem files only. Both `verify_all` and `Index::rebuild_from_files` consume the filtered list, so adding a new sidecar shape requires no scanner changes — the dotless-stem rule excludes it automatically.
- `/orb:drive` SKILL.md — every drive sidecar reference (path, code block, resumption-detection snippet, embedded CronCreate heartbeat prompt body) and every review-file path uses sidecar form. The promote-stage description corrected: `promote.sh` materialises a spec at the flat `.orbit/specs/<spec-id>.yaml` (no folder).
- `/orb:rally` SKILL.md — folder convention collapsed end-to-end. `RALLY_DIR` removed; CLI argument changes from `<rally-folder>` to `<rally-id>`; resumption scan iterates `.orbit/specs/*.rally.yaml`. Per-child decision packs and interviews migrate to sidecars (`<child-spec-id>.decisions.md`, `<child-spec-id>.interview.md`); the path-discipline contract names the two specific sidecars rather than a per-child folder.
- `/orb:review-spec` and `/orb:review-pr` SKILL.md — inline-invocation defaults default to sidecar paths; the `<spec-folder>`-shaped branch and the `.orbit/reviews/` fallback are removed.
- `.orbit/METHOD.md` and `plugins/orb/skills/setup/METHOD.md` — vocabulary table rewritten to sidecar form (Drive state, Rally state, Interview rows). The two files stay byte-equal so greenfield projects bootstrapped via `/orb:setup` get the same canonical statement.

### Fixed

- `orbit verify` and `orbit spec list` no longer break when a `<id>.drive.yaml` or `<id>.rally.yaml` sidecar is present in `.orbit/specs/` — previously the scanner attempted to parse them as `Spec` and surfaced an `unknown field, expected one of id, goal, cards, status, labels, acceptance_criteria` error. The dotless-stem filter excludes sidecar shapes from primary entity loads.

## [0.4.6] - 2026-05-09

`/orb:setup` now primes downstream projects with a canonical orbit method overview that CLAUDE.md `@-imports` — no more inline vocabulary blocks drifting across plugin versions. `/orb:card` and `/orb:distill` gain a card-vs-choice pre-flight so implementation-surface decisions ('should X be in bash or rust?') route to choice files, not aspirational cards.

### Added

- `plugins/orb/skills/setup/METHOD.md` — canonical orbit method overview (single screen, ~72 lines): pipeline, vocabulary, card-vs-choice-vs-spec-vs-memo decision tree, substrate rules, four pillars, BLUF / Decision Brief skeleton inlined directly so projects without `.orbit/STYLE.md` get the prose contract too.
- `plugins/orb/scripts/setup-method.sh` — atomic `/orb:setup` §6 implementation: legacy-CLAUDE.md detection BEFORE any file write (decline → atomic refuse, no orphan METHOD.md), byte-for-byte drift detection on re-run, idempotent `@-import`. Supports `--answer-legacy` / `--answer-drift` for scripted contexts.
- `plugins/orb/scripts/tests/test-setup-method.sh` — four scenarios (fresh / drift-prompt / legacy-accept / legacy-refuse), all green.
- `/orb:card` and `/orb:distill` SKILL.md gain a "Card or Choice?" pre-flight — implementation-surface decisions route out to MADR choice files at `.orbit/choices/`, not new cards.
- Choice `0020-shell-scripts-to-rust-verbs` — policy choice naming the migration path for `promote.sh`, `setup-method.sh`, and `orbit-acceptance.sh` to orbit Rust verbs, sequenced opportunistically per script.

### Changed

- `/orb:setup` SKILL.md §6 rewritten end-to-end. The old inline `## Workflow (orbit)` / `## Orbit vocabulary` / `## Current Sprint` snippet is removed; METHOD.md is the single source of truth. Existing downstream CLAUDE.md files containing the legacy blocks get an atomic migrate-or-refuse prompt — no path to dual-source drift.
- CLAUDE.md decision tree gains a fourth branch covering choices, placed before the card branch so agents discriminate before defaulting to a card. Worked example named: "should `orbit spec promote` live in rust" is a choice, not a card.
- CLAUDE.md vocabulary table's `Decision` row renamed to `Choice` (matches the `.orbit/choices/` directory), path corrected from `.md` to `.yaml`, and the row carries the implementation-surface framing.
- `/orb:distill` SKILL.md §2 Draft adds per-candidate capability-vs-choice classification — choice-shape distillations write MADR files instead of cards.
- Card 0017 amended: greenfield scenario then-clause updated to "writes `.orbit/METHOD.md` and ensures CLAUDE.md @-imports it"; two new scenarios cover drift detection and atomic legacy migration; pillar 2 (agent self-learning) attribution added via `relations:feeds → 0028`.
- orbit-repo CLAUDE.md dogfooded: 119 → 32 lines. Substrate sections (vocabulary, decision tree, pipeline, four pillars, key concepts, orbit-state quick reference) replaced by `@.orbit/METHOD.md`. The standalone "Session Completion / Mandatory Workflow" section is reshaped to a tight 4-line "Push discipline" block; substrate-shaped rules (orbit task verbs, hand-off via memory) deleted from CLAUDE.md, project-specific git discipline kept inline.

## [0.4.5] - 2026-05-09

The bd-era cleanup arc closes — `promote.sh` is ported to orbit-state, every /orb:drive promote stage runs against the substrate directly, no manual workaround. /orb:design also gains three modes (open / closed / partial), an implementation-question filter, and a user-voice prose paragraph promoted to a first-class output that downstream specs cite as the intent contract.

### Added

- /orb:design pre-flight design-space classification — open (no choice file), closed (architectural choice already pinned), partial (residual trade-offs). Closed mode emits a one-screen `design-note.md` instead of a full interview.
- Implementation-question filter at /orb:design — each candidate question must require codebase context, schema knowledge, metric vocabulary, or evaluation tooling to pass. Author-preference questions get routed to implementation-notes for the implementing agent rather than surfaced to the author.
- Top-of-file user-voice "What good looks like" paragraph slot in interview / design-note artefacts, drafted by the agent from the card and offered for editing rather than reconstructed via Q&A.
- /orb:spec and /orb:spec-architect cite the user-voice paragraph as the intent contract — quoted in the spec's `goal` or `notes`, alongside the Q&A.
- Mode-switch trigger at /orb:design — twice-rejected implementation-shaped questions trigger a switch to closed/partial mode rather than another reformulation.

### Changed

- `plugins/orb/scripts/promote.sh` rewritten against orbit-state — derives `<YYYY-MM-DD>-<card-slug>` from the card filename, calls `orbit spec create`, writes `acceptance_criteria` directly into the flat spec YAML, then runs `orbit canonicalise`. Stdout still emits just the spec id; new `--root` passthrough makes the script testable.
- `test-promote-gate-propagation.sh` now exercises the real promote → orbit-spec-create → orbit-spec-show round-trip end-to-end under a temp `--root`, not just the dry-run path.
- /orb:drive SKILL.md trimmed 853 → 688 lines (-19%); /orb:rally SKILL.md trimmed 1016 → 840 lines (-17%). Slim Critical Rules sections restored.
- `.orbit/conventions/acceptance-field.md` rewritten from the bd-era markdown-line format to orbit-state's structured `acceptance_criteria`.
- Project `CLAUDE.md` no longer inlines STYLE.md — the `@.orbit/STYLE.md` import resolves at session start, verified empirically against fresh subagent forks.
- Card 0028 amended to documentation-only pillar wiring; goal refined to reflect emergent pillar outcomes rather than schema fields.

### Removed

- Six bd-era files: `bd-init.sh`, `parse-progress.sh`, `session-context.sh`, `rally-coherence-scan.sh`, `AGENTS.md`, `plugins/orb/hooks/hooks.json`.

## [0.4.4] - 2026-05-08

First live wires under choice 0019 (cards declare framework wires in scenarios; aspirational cards don't pass review). Card 0026's BLUF / Decision Brief contract is now substrate-enforced — distilled into `.orbit/STYLE.md`, imported into project CLAUDE.md, and cited from the three prose-producing orb skills. Closes the canonical aspirational-card example the choice was written about.

### Added

- `.orbit/STYLE.md` — distilled BLUF / Decision Brief contract: TL;DR-led skeleton, recommendation discipline, seven anti-patterns by name, response-variant table, tone contract. Single-screen distillation, not a verbatim card transcription.
- Project `CLAUDE.md` imports STYLE.md via `@.orbit/STYLE.md` (with the contract inlined for cache-resilience) so the contract loads into every orbit-repo session.
- `/orb:design`, `/orb:review-spec`, `/orb:review-pr` SKILL.md files cite card 0026 + STYLE.md using the belt-and-braces pattern (one-line prose marker + `@` import).
- Choice 0019 — cards must declare framework wires in scenarios; aspirational cards (`maturity: planned` + empty `specs:`) don't pass review.
- Cards 0028 (four pillars), 0029 (fan-out), 0030 (canonical schema and glossary), 0031 (design-session user language) distilled from memos. Each carries the "Wired into the framework" gate scenario.

### Changed

- Project CLAUDE.md: four pillars (executive-level interaction, agent self-learning, agent state-persistence, long-running R&D) named explicitly as the load-bearing why-test for any work in this repo.
- Card 0026 (executive-communication) maturity bumped `planned` → `emerging` after the first wires drive shipped.

### Fixed

- `orbit memory remember` invocation syntax in skill prompts and PRIME.md — previously used a stale form that didn't match the current orbit-state CLI.

## [0.4.3] - 2026-05-08

`orbit canonicalise` is now a first-class subcommand of the main `orbit` binary. Hand-edited cards and choices that drift from the canonical writer's output (whitespace, field order, trailing newlines) used to fail `orbit verify` with `not_byte_identical` and no in-toolbox fixer — the brew binary shipped only `verify`, and the standalone `orbit-canonicalise` repair tool wasn't packaged. Surfaced when a downstream session got stuck adding a new MADR with no path forward short of building from source.

### Added

- **`orbit canonicalise [--dry-run] [--json]`** — walks `.orbit/{specs,cards,choices,memories}`, parses each file, reserialises through the canonical writer, and rewrites any drift in place. Mirrors `orbit verify`'s output shape; exits non-zero only on parse failures (drift fixed in place is success). The shared logic now lives in `orbit_state_core::canonicalise`, callable from both the main CLI subcommand and the standalone `orbit-canonicalise` binary.

### Changed

- **`orbit verify` error message** for `NotByteIdentical` now points at `orbit canonicalise` as the fixer, replacing the prior advice to "run a verb that touches the file" — a workflow that didn't exist for `Choice` (read-only verbs only).

## [0.4.2] - 2026-05-08

orbit now lives at `meridian-online/orbit` and shares the meridian release pipeline with `finetype` and `arcform`. End-users install the orbit binary via `brew install meridian-online/tap/orbit` (Homebrew on macOS, Linuxbrew on linux) instead of `cargo install --path orbit-state/crates/cli`. Plugin and binary versions are aligned from this release onward; both move in lockstep. See decision `0018-orbit-distribution-via-meridian` and spec `orbit-distro` for the migration plan; card `0027-brew-installable` is the capability being delivered.

### Migration notes for orb plugin users

Existing installations of `orb@orbit` against `hughcameron/orbit` need to re-add the marketplace from the new home:

```
/plugin marketplace remove orbit
/plugin marketplace add meridian-online/orbit
/plugin install orb@orbit
```

GitHub auto-redirects the old clone URL, so existing `git clone` of the substrate repo continues to work, but the Claude Code plugin marketplace metadata pins the original org/repo and needs to be refreshed manually.

### Added

- **`orbit` binary distribution** — pinned tar.gz archives for x86_64 and aarch64 on macOS and linux, sha256-stamped, published to GitHub Releases on every tag. The release pipeline auto-updates `meridian-online/homebrew-tap`'s `Formula/orbit.rb` so `brew upgrade orbit` is the upgrade path for end-users. Cargo-install remains supported for contributors building from source.

### Changed

- **Plugin and binary versions aligned at 0.4.2.** `plugins/orb/.claude-plugin/plugin.json` and `orbit-state/Cargo.toml` workspace version are now synchronised; releases bump both in lockstep. The orbit-state binary moves from its `0.1.0-dev` development version to the unified release line — this is an alignment jump, not a semver claim about the binary's API.

## [0.4.1] - 2026-05-08

orbit-state v0.1 substrate adoption — the six core skills now read and write the files-canonical orbit-state substrate (`.orbit/cards`, `.orbit/specs`, `.orbit/choices`, `.orbit/memories`) via the `orbit` CLI instead of `bd`. Verdict-line contracts, deterministic gate checks, and the cold-fork architecture are preserved verbatim; the underlying file format and tool surface have changed.

This is a substrate-shaped patch release. The skills assume the host repo has migrated to orbit-state per the playbook at `~/github/hughcameron/ops/playbooks/migration-orbit-state-v0.1.md`. Pre-migration repos should pin to 0.4.0 or migrate before upgrading.

### Added

- **`orbit-acceptance.sh`** — orbit-state-shaped sibling of `parse-acceptance.sh`. Same five subcommands (`acs`, `next-ac`, `blocking-gate`, `has-unchecked`, `check`) and same tab-separated tuple contract, but reads via `orbit spec show <id> --json` and writes via `orbit spec update --ac-check` instead of bd's `--acceptance` field.

### Changed

- **`/orb:implement`** rewritten against orbit-state. Spec-id input (was bead-id). AC list read from the spec's `acceptance_criteria` array (`{id, description, gate, checked}`). AC flips through `orbit-acceptance.sh check` → `orbit spec update --ac-check`. Detours become sub-tasks under the current spec via `orbit task open --spec-id <current>`; the bd `discovered-from` dep edge has no orbit-state v0.1 equivalent and is captured in the task body text. NO-GO close uses `orbit spec note` + `orbit spec close` (no `--reason` flag in orbit-state).
- **`/orb:drive`** rewritten against orbit-state. Drive state migrates from bd metadata fields (`drive_stage`, `drive_iteration`, `drive_review_*_cycle`) to `.orbit/specs/<spec>/drive.yaml` — the named slot in the orbit vocabulary. Iteration chains move from the bd dep tree to a `drive.yaml.iteration_history` array. Review output paths move from `orbit/reviews/<bead-id>/` to `.orbit/specs/<spec-id>/`. Verdict-line regex contract preserved verbatim.
- **`/orb:rally`** rewritten against orbit-state. Epic bead + child bead graph + dep edges all collapse into `.orbit/specs/<rally-folder>/rally.yaml`. The claimable-set rule (open + all `dep_predecessors` closed/parked) replaces `bd ready --type task --parent <epic>`. Six-token reason_label vocabulary preserved.
- **`/orb:review-spec`** rewritten against orbit-state. Spec-id input; reads via `orbit spec show <id> --json` + `orbit-acceptance.sh acs <id>`. Verdict-line contract preserved verbatim. Output paths support both flat (`.orbit/specs/<id>.yaml`) and folder-shaped (`.orbit/specs/<folder>/spec.yaml`) specs.
- **`/orb:review-pr`** rewritten against orbit-state. Same parser + verb shift as review-spec; AC coverage check now reads from the spec's `acceptance_criteria` array.
- **`/orb:audit`** rewritten against orbit-state. Locates specs via `orbit spec list` (was filesystem glob). Drops the deprecated `ac_type` field — orbit-state's strict schema stores ACs uniformly with `{id, description, gate, checked}`. Non-code classification is now made from description text plus gate flag at audit time.
- **Path-only updates** across the remaining skills (`card`, `design`, `discovery`, `distill`, `keyword-scan`, `memo`, `setup`, `spec`, `spec-architect`) and the gate-AC verification regression test — all `bd` references swapped for `orbit` verbs; `orbit/` → `.orbit/` paths.

### Removed

- **`parse-acceptance.sh`** — bd-era markdown AC parser. Its only live consumer (the gate-AC verification regression test) was ported to `orbit-acceptance.sh`'s JSON-array stdin shape.

### Notes

- Skills assume host-repo migration via the orbit-state v0.1 playbook. Mixing this plugin version with a bd-era host repo produces parse errors.
- The `orbit-state` Rust binary is a separate distribution (not bundled with this plugin). See the migration playbook for build instructions.

## [0.4.0] - 2026-05-01

Bead-native execution layer — orbit's four-card overhaul (orbit-6da.1–6da.4) makes beads the canonical substrate for AC tracking, drive orchestration, and rally state. The snapshot bridge between drive and the cold-fork reviewers is removed; reviewers read beads directly. `drive.yaml`, `progress.md`, and `rally.yaml` are gone. The bead graph IS the workflow.

### Added

- **Bead-native cold-fork reviews** (card 0016, orbit-6da.4). `/orb:review-spec` and `/orb:review-pr` read the bead directly via `bd show <bead-id> --json` and `parse-acceptance.sh acs <bead-id>` — the same parser `/orb:implement` uses, so AC interpretation cannot drift between implement and review. The snapshot bridge (`bead-snapshot-<date>.md`) is removed pipeline-wide. Verdict files land at `orbit/reviews/<bead-id>/review-{spec,pr}-<date>.md` for both forked and inline invocations.
- **End-to-end gate semantics.** Card scenario `gate: true` propagates through `promote.sh` to bead AC `[gate]` marker. `parse-acceptance.sh acs` exposes `is_gate=1` as a parsed column. `/orb:review-spec` Pass-1 deterministic check (non-empty / not-placeholder / ≥20 chars) fires against gate-AC description text — was silently no-op under the snapshot bridge.
- **Test fixtures for the bead-native review substrate.** `plugins/orb/scripts/tests/test-gate-ac-verification.sh` (parser + 3 deterministic rules) and `test-promote-gate-propagation.sh` (card scenario → promote.sh → bead AC `[gate]` marker).
- **MADR 0013** — `.orbit/choices/0013-bead-acceptance-field-as-cold-fork-substrate.md`. Documents five design decisions (skill-reads-bead vs drive-prerender; AC-shape mapping; ac_type mapping; gate propagation via promote.sh; hard cutover), the substrate-mapping table, and full consequences including accepted losses (ac_type exemption fidelity; AC commit-provenance; cycle-history `[x]` leak).
- **Card 0017** — `/orb:setup` is bead-aware (planned). Folds bd precondition check, orbit plugin version sanity, and `bd-init.sh` invocation into `/orb:setup` so the orbit/ layout and `.beads/` initialise atomically. Until this ships, bead-init runs as a manual operator step.
- **Beads foundation** (orbit-6da.0). Beads issue tracker initialised in orbit itself. Acceptance-field convention (`.orbit/conventions/acceptance-field.md`). Core scripts: `parse-acceptance.sh` (five subcommands for AC enumeration and check-off), `promote.sh` (card → bead with AC generation), `bd-init.sh` (project initialisation), `PRIME.md` (session-start context).
- **`/orb:implement` rewritten against beads** (orbit-6da.1). Bead acceptance field replaces `progress.md` as the AC source of truth. `TaskCreate`, drift detection (sha256), and resume reconcile removed. Detours escalate as sub-beads via `bd create --parent ... --deps "discovered-from:..."`. Gate enforcement delegated entirely to `parse-acceptance.sh next-ac`.
- **`/orb:drive` rewritten against beads** (orbit-6da.2). Design + Spec stages collapse into `promote.sh card→bead`. Drive state machine lives in bead metadata (`drive_stage`, `drive_iteration`, `drive_review_*_cycle`). Iteration history tracked via `discovered-from` dependency edges between iteration beads. NO-GO closes current bead and promotes a new iteration bead carrying constraint history in the description.
- **`/orb:rally` collapses onto the bead dependency graph** (orbit-6da.3). `rally.yaml` removed. Epic bead + child beads IS the rally. `bd ready --type task --parent <epic>` replaces TaskList for in-session card visibility. Rally phase tracking lives in epic bead metadata. Mid-flight parallel→serial conversion is a single `bd dep add` invocation.

### Changed

- **Drive cold-fork brief** — Stage 1 (review-spec) and Stage 3 (review-pr) briefs carry only `<bead-id>`, absolute verdict output path, and verdict-line contract. Snapshot paths gone.
- **Drive Completion** — commit-1 description and PR-body no longer reference bead snapshots (they no longer exist). Commit 1: `All code changes and the review files`.
- **Inline-mode verdict paths** in both review skills moved to `orbit/reviews/<bead-id>/review-{spec,pr}-<date>.md` (was `.orbit/specs/YYYY-MM-DD-<topic>/...`).
- **Drive SKILL.md section renumbering** — Stage 1: §1.1 is now "Compute the cycle-specific verdict path" (was §1.2; §1.1 "Write the bead snapshot" is gone). Stages 1 and 3 section numbers updated throughout; Resumption table cross-references corrected.
- **`/orb:review-spec` Step 1** renamed to "Gather the Bead"; takes a bead-id argument; reads `bd show <bead-id> --json` + `parse-acceptance.sh acs <bead-id>`. Spec.yaml lookup, interview_ref lookup removed.
- **`/orb:review-pr` Phase 1/2** reads bead via `bd show` + `parse-acceptance.sh`; `progress.md` cross-reference removed; `ac_type` / `test_prefix` field references removed; AC coverage check uses bare `ac<NN>` test-name pattern; reviewer contextualises exemptions in the honest-assessment paragraph.
- **Decision 0002 (`ac-test-prefix`)** status updated to `superseded by 0013 (review-pr scope only)` — `test_prefix` remains live in `/orb:spec`, `/orb:spec-architect`, `/orb:audit`, `/orb:implement`.
- **Decision numbering collision resolved** — `0011-design-intent-not-means.md` renamed to `0012-design-intent-not-means.md`. New substrate MADR is `0013`.
- **Drive heartbeat self-termination** — full-autonomy heartbeat calls `CronDelete` on itself when the bead transitions to `closed`, as a backstop alongside primary cleanup in §Completion and §Escalation.
- **Cold-fork review gate hardened** against nested Agent unavailability — drive escalates immediately rather than falling back to inline review, preserving the cold-fork separation contract.

### Removed

- `drive.yaml` per-iteration orchestration state — replaced by bead metadata fields.
- `progress.md` AC tracker — replaced by bead `acceptance_criteria` field via `parse-acceptance.sh`.
- `rally.yaml` rally state — replaced by epic bead + child bead graph.
- Bead snapshot bridge (`bead-snapshot-<date>.md`, `bead-snapshot-<date>-pr.md`) from drive's review pipeline.

## [0.3.3] - 2026-04-22

### Added
- `/orb:implement` §6a — out-of-scope findings during implementation are forwarded as memos (`.orbit/cards/memos/`) with data and provenance. Agents no longer suggest "open a follow-up card" — cards describe capabilities, not work items. Distill handles the structural decision later.

### Changed
- `/orb:review-pr` — explicit rule: never suggest follow-up cards in findings.

## [0.3.2] - 2026-04-21

### Changed
- **Design interviews capture intent, not means.** `/orb:design` reframed from "works out the how" to "captures what good looks like." Questions target outcomes, priorities, risk appetite, and scope — not implementation approach. Means-level observations (which function, what algorithm, test structure) are recorded as implementation notes for the implementing agent instead of being asked as interview questions.
- Interviewer persona gains a decision-level gate before the evidence hierarchy: "Would the author need codebase context to answer this?" If yes, it's a means question — record as an implementation note, don't ask.
- `/orb:discovery` aligned with the same intent-level questioning principle.

### Added
- `implementation_notes` field in spec YAML format — means-level leads from the design session. Not constraints; starting context the implementing agent can use or override with evidence. Consumed by `/orb:implement`.
- `.orbit/choices/0012-design-intent-not-means.md` (originally numbered 0011 at 0.3.2 release; renumbered in 0.4.0 to resolve a numbering collision with `0011-beads-execution-layer.md`)

## [0.3.1] - 2026-04-21

### Changed
- **Rally state moves into a spec-shaped folder.** `rally.yaml` now lives at `.orbit/specs/<date>-<slug>-rally/rally.yaml` instead of a flat `.orbit/specs/rally.yaml`. Completed rallies stay where they are — the folder itself is the history record. No sibling `archive/` directory, no archival prompt when the next rally begins.
- `/orb:rally` §1 scans `.orbit/specs/*/rally.yaml` for an active rally (phase != complete); §3 Initialise creates the rally folder before writing `rally.yaml` inside it; §10 Completion and §11 Resumption drop the "awaiting archival" language and the archive prompt. Two or more rallies with `phase != complete` is a state error per §12.
- `session-context.sh` scans `.orbit/specs/*/rally.yaml` instead of checking a fixed path, and the `latest_spec` find excludes `*-rally` folders so the workflow surface never mistakes a rally folder for a spec folder.
- CLAUDE.md vocabulary row for Rally state updated to the new folder-per-rally path.

### Added
- **Vocabulary glossary in `/orb:setup`.** The `## Workflow (orbit)` snippet appended to a project's `CLAUDE.md` now carries a six-row `## Orbit vocabulary` block (Card / Memo / Interview / Spec / Progress / Decision) and the "cards describe *what*, specs describe *work*" discipline line. Idempotent setup runs detect the pre-vocabulary shape and offer a targeted migration prompt — on `y`, the legacy "Artefacts live in…" line is replaced with the full `## Orbit vocabulary` block while the skills list and Current Sprint are left untouched.

## [0.3.0] - 2026-04-20

UX uplift rally — four coordinated cards shipped together (PRs #12, #14, #11, #10) to make orbit sessions mission-resilient, visible in real time, and sharper at approval gates.

### Added
- **Mission resilience — three-layer spec fidelity through disruptions.** `progress.md` gains `Spec path:`, `Spec hash:`, `Current AC:` header fields and a `## Detours` section for out-of-order work. The `SessionStart` hook surfaces the current AC on resume, detects spec drift (sha256 mismatch with recorded baseline), and blocks advancement past `(gate)`-annotated ACs until the gate closes. `/orb:implement` §5 now declares detour discipline, spec-hash backfill, drift-halt, and gate-enforcement rules. (#12)
- **Session visibility — first-class TaskList integration for `/orb:implement`.** After writing `progress.md`, the skill emits a `TaskCreate` per hard constraint and per AC (flat, scoped by `metadata.spec_path`, subjects verbatim from progress.md). `TaskUpdate` must land in the same tool-call turn as the progress.md checkbox flip — anything else is a protocol violation. Mid-session resumes reconcile the task list against progress.md via a deterministic cancel-then-recreate algorithm using `TaskUpdate status: cancelled` + `TaskCreate`, with a canonical `RESUME_REBUILD_WARNING`. (#14)
- **`plugins/orb/scripts/parse-progress.sh`** — single source of truth for `progress.md` parsing. Six subcommands: `acs`, `constraints`, `spec-path`, `next-unchecked-ac`, `post-gate-ac`, `has-unchecked`. `## Detours` content is ignored by the AC parser — a `- [x] ac-02` inside Detours never flips ac-02's status. Both the mission-resilience next-AC surface and the session-visibility resume reconcile delegate to this helper. (#14)
- **Monitor heuristic for long test runs.** `/orb:implement` §5 declares that tests expected to run >60 seconds or full-suite should be launched via Monitor with the canonical failure-marker filter `grep --line-buffered -E 'FAIL|ERROR|AssertionError|Traceback'`, so failures stream back mid-run rather than on completion. Short tests stay on Bash. (#14)
- **First-failure checkpoint.** On the first test failure of a run, `/orb:implement` pauses and offers two canonical options (investigate-and-re-run vs let-the-suite-finish-then-triage) via `AskUserQuestion` under an interactive TTY; subsequent failures do not re-prompt. Under `/orb:drive` full (non-interactive), the skill emits a canonical `FIRST_FAILURE_NONINTERACTIVE_MARKER` to stderr and halts with exit 2 for upstream triage. (#14)
- **`/orb:drive` live visibility — heartbeat, escalation ping, four-option verdict gate.** Guided-mode PR gate now offers four canonical choices (GO, NO-GO, read-reviews-first, drop-to-supervised) instead of a binary. Long-running stages emit heartbeat surfaces so the author knows the agent is alive; escalations ping the author with context rather than silently parking. (#11)
- **`/orb:rally` §2b approval gate tightened.** Approval uses canonical labels; the modify flow is now a two-prompt loop (collect edits, confirm, re-present) rather than free-form one-shot. Thin-card refusal still runs unconditionally before the gate. (#10)

### Changed
- `/orb:implement` §1–§4c are byte-identical to the post-mission-resilience baseline (sha256 verified, empty diff) — the session-visibility changes land as §4d + four §5 rules, not as rewrites of the shipped pre-flight behaviour.
- `plugins/orb/scripts/session-context.sh` next-AC surfacing and resume-reconcile blocks refactored to delegate to `parse-progress.sh`; zero `awk|sed` hits remain in those regions.

## [0.2.19] - 2026-04-20

### Added
- **`/orb:rally`** — new top-level orchestration skill for multi-card sprints. Proposes a rally, runs design/implementation in parallel via nested forked Agents with recursive context separation, and enforces a consolidated decision gate. Coherence is enforced via `plugins/orb/scripts/rally-coherence-scan.sh`. See `.orbit/choices/0003-rally-skill-boundary.md`, `0008-rally-subagent-path-discipline.md`, `0009-rally-parallel-drive-full.md`, `0010-rally-thin-card-guard.md`.
- `SessionStart` hook now detects an active `.orbit/specs/rally.yaml` and surfaces rally goal, phase, autonomy mode, per-card status, and parked constraints. Individual drive states are subordinated to the rally display when a rally is active.

### Changed
- **Artefact layout consolidated under `orbit/`.** The four top-level directories (`cards/`, `specs/`, `decisions/`, `discovery/`) have moved to `.orbit/cards/`, `.orbit/specs/`, `.orbit/choices/`, and `.orbit/discovery/`. All skill docs, hooks, examples, and references have been rewritten to point at the new paths. The move was done via `git mv` so history is preserved (`git log --follow` traces every artefact back through the rename).
- `/orb:setup` now detects four repo states — **greenfield** (create fresh `orbit/`), **brownfield** (legacy bare dirs present → single all-or-nothing migration prompt), **idempotent** (already migrated, no-op), and **mixed** (refuse with a clear collision report). Brownfield migration runs one `git mv` transaction covering every detected bare dir; untracked residue is reported after the move.
- `SessionStart` hook (`session-context.sh`) now gates on the presence of `orbit/` and emits a one-line nudge (`orbit: legacy layout detected. Run /orb:setup to migrate.`) when bare-layout dirs are found without `orbit/`. Hardened against partial `orbit/` layouts: `find` pipelines inside the drive and latest-spec scans are guarded with `[[ -d ... ]]` checks plus `|| true`, so the hook survives manually-created `orbit/` directories without `cards/` or `specs/` subdirs.
- `CLAUDE.md` snippet appended by `/orb:setup` now references `.orbit/cards/`, `.orbit/specs/`, and `.orbit/choices/`.
- **`/orb:drive` forks its review stages.** `review-spec` and `review-pr` now run in nested forked Agents with `context: fork` at the architectural root, honouring the context-separation contract that the review skills themselves already declared. Verdict is read from the written artefact rather than the return message. See `.orbit/choices/0005-drive-review-artefact-contract.md`, `0006-drive-cold-re-review.md`, `0007-drive-rerequest-budget.md`.

### Notes
- Prior review artefacts (e.g. `review-pr-*.md`, `review-spec-*.md`) that quoted old bare paths were rewritten in place during the artefact-folder migration. This is a deliberate evidence-fidelity trade-off in favour of a clean end-state; the migration commit itself is the audit trail for the path change.

## [0.2.18] - 2026-04-17

### Added
- `test_prefix` metadata field for specs — disambiguates AC-to-test mapping across multi-spec projects. Skills `spec`, `spec-architect`, `audit`, `review-pr`, and `implement` all consume the prefix.
- `decisions/0002-ac-test-prefix.md` — documents the choice of explicit spec-scoped prefixes over globally unique IDs or auto-derived slugs.

### Changed
- AC naming guidance now recommends slug-style prefixes (`remat`, `introspect`) over version-like prefixes (`v03`), since `metadata.version` already carries the version.
- `/orb:audit` warns when multiple specs exist but any lack `test_prefix`.

## [0.2.17] - 2026-04-16

### Changed
- `/orb:release` — moved from user-level skill (`~/.claude/skills/release/`) into the orbit plugin. Invoked as `/orb:release` instead of `/release`, freeing the `/release` namespace for project-specific release skills.

## [0.2.16] - 2026-04-16

### Changed
- `/orb:drive` pipeline expanded to 5 stages: Design → Spec → **Review-Spec** → Implement → Review-PR. Every spec now gets reviewed as part of the drive.
- `/orb:drive` guided mode removes intermediate go/no-go gates. Reviews ARE the quality gates. The only interactive pause is a rich final summary (spec review verdict, AC coverage, honest assessment) before PR creation. "Let me read the reviews first" is an explicit option.
- `/orb:drive` supervised mode gates now include richer context (AC counts, finding summaries) instead of bare "greenlight?" prompts.
- `/orb:review-spec` replaced with progressive 3-pass model (decision 0001). Pass 1 (structural scan) always runs. Pass 2 (assumption & failure analysis) triggered by findings or content signals. Pass 3 (adversarial review) triggered by structural concerns. Depth scales with findings, not upfront classification.
- Removed risk tier classification (HIGH/STANDARD/SKIP) from `/orb:spec`. Every spec gets reviewed — the progressive model makes tier gating unnecessary.

### Added
- `decisions/0001-progressive-spec-review.md` — first orbit decision record. Documents why tier-based review gating was replaced with progressive review.

## [0.2.15] - 2026-04-15

### Added
- `/orb:drive` Disposition section — defines the agent's working stance: find the way through, treat negatives as constraints on the next iteration, push past the first plateau. Ported from prior agent research disposition.
- Semantic escalation triggers (recurring failure mode, contradicted hypothesis, diminishing signal) alongside the mechanical 3-iteration budget. An honest agent may escalate before the budget is spent.
- Escalation summaries now include "What would have to be true" — what assumptions need revisiting for a future attempt to succeed.

## [0.2.14] - 2026-04-15

### Added
- `/orb:drive` — agent-driven card delivery. Takes a card path and autonomy level (full/guided/supervised), then drives the full orbit pipeline (design → spec → implement → review-pr) as a single inline session. Tracks state in `drive.yaml` for session resumption, with a 3-iteration budget before escalation. Thin cards (< 3 scenarios) are refused for full autonomy.
- `session-context.sh` now detects `drive.yaml` and surfaces active drive state (card, autonomy level, iteration, status, next action) at session start. Escalated drives show a distinct message.

## [0.2.13] - 2026-04-13

### Added
- `/orb:card` "What Gets Closed" section — specs are the closure unit; cards are never closed. A NO-GO result updates the card's `goal` and `maturity`, not its existence.
- `/orb:implement` step 7 "When a Spec Produces a NO-GO" — guidance to record evidence in `progress.md`, mark ACs with the result, update the card's goal, and loop back to `/orb:design` with the new evidence.

## [0.2.12] - 2026-04-12

### Added
- `/orb:keyword-scan` — shared technique for keyword-based search across orbit artifacts. Extracts 5–8 distinctive domain terms from a card, spec, or interview; builds a ripgrep alternation pattern; falls back to `grep -rl` in environments without `rg`. Referenced by all workflow skills rather than inlining the pattern.
- `/orb:spec` now appends the new spec path to the card's `specs` array after saving (write-time enforcement). Agents downstream that read the array get a complete work trail without manual upkeep.
- `/orb:design` reconciles the card's `specs` array against a keyword scan of `specs/` before the session starts — surfaces orphaned specs the author can confirm to link.
- `/orb:distill` checks `cards/` for existing capability overlap before drafting new cards.
- `/orb:card` checks `cards/` and `specs/` for overlap before finalising a new card.
- `/orb:discovery` searches `specs/` and `decisions/` for prior art before the interview begins.
- `/orb:implement` searches the project source for existing code and patterns related to the spec's ACs.
- `/orb:review-pr` searches `decisions/` for architectural choices the implementation should respect.

### Changed
- README workflow diagram shows the multi-spec loop: dashed edge from Ship back to Design when the card goal is not yet met.
- End-to-end walkthrough describes iterative goal pursuit across multiple specs.

## [0.2.11] - 2026-04-11

### Changed
- `/orb:design` now reads the card's `specs` array as cumulative progress. Presents what each prior spec contributed and anchors the session on the gap between current state and goal.
- Design sessions no longer assume linear spec progression — specs may enhance a capability from different angles (infrastructure, data quality, tooling, adjacent work). The session surfaces which path the author intends.
- Interview record template includes goal, prior spec summary, and gap context.

### Added
- `CHANGELOG.md` backfilled from v0.2.0 through v0.2.10 (added in this release cycle).

## [0.2.10] - 2026-04-09

### Added
- `goal` field on cards — specific, measurable target at the current maturity. `so_that` is timeless (why); `goal` is current (what success looks like now). Goals evolve as the capability matures; git history tracks the progression.
- Sprint goal structure in CLAUDE.md — `/orb:setup` scaffolds a `Current Sprint` section listing the objective and card goals.
- README documents goals and sprint concepts.

## [0.2.9] - 2026-04-09

### Changed
- Replaced `priority` field (now/next/later) with `maturity` (planned/emerging/established) on cards. Cards describe capability state, not work priority.

### Added
- `specs` array on cards — lists the specs that have addressed each capability, giving a clear trail of work done.

## [0.2.8] - 2026-04-09

### Changed
- Distill now uses a staged **Draft → Review → Write** flow instead of per-card approve/edit/reject.
- All cards are drafted first and presented as a numbered batch.
- The agent surfaces overlaps, gaps, and low-confidence cards during review.
- Batch feedback (merge, split, drop, rename) replaces individual card gates.
- Nothing is written to disk until the author explicitly says "write."

## [0.2.7] - 2026-04-08

### Changed
- Cards are living documents — the lifecycle table (Open/In progress/Delivered/Closed) replaced with "Cards Are Living Documents" section.
- `cards/done/` directory removed from prescribed structure.
- Distill now accepts files, directories, or natural-language scope descriptions (not just a single file path).
- First-principles lens is always applied: "what does this product do?" not "what's planned next?"

### Added
- `CLAUDE.md` for the orbit repo — establishes that sessions here are about workflow refinement.

## [0.2.6] - 2026-04-08

### Changed
- Renamed `/orb:init` to `/orb:setup` to avoid collision with built-in `/init` command.

## [0.2.5] - 2026-04-08

### Changed
- Use "the author" for the human driving the workflow; reserve "the user" for end-users of the software being built.

## [0.2.4] - 2026-04-07

### Added
- `/orb:audit` skill — audit AC-to-test traceability across specs, finding untested code ACs, orphaned test prefixes, and coverage gaps.
- `ac_type` classification (code/doc/gate/config) for acceptance criteria.

## [0.2.3] - 2026-04-06

### Added
- `/orb:memo` skill — quickly jot rough ideas as freeform markdown in `cards/memos/`.
- README rewritten for orbit 0.3 — end-to-end walkthrough, "four ways in" section.

### Changed
- Evidence hierarchy added to interviewer, design, implement, and spec-architect skills.
- Implement skill proceeds after checklist without waiting for confirmation.

## [0.2.2] - 2026-04-05

### Fixed
- Implement skill: clarify that step 4 proceeds to write code after presenting the checklist.

## [0.2.1] - 2026-04-04

### Added
- Specs for cards 0001–0003.
- Implemented 0001-memos: freeform idea capture in `cards/memos/`.
- Implemented 0002-distill: extract cards from unstructured input.
- Implemented 0003-implement: pre-flight spec check with AC checklist.

### Added
- Cards for memos (0001), distill (0002), and implement (0003) features.

## [0.2.0] - 2026-04-03

### Added
- All 18 orbit workflow skills, README, and LICENSE.
- References field on cards; card-aware interview mode.
- Split interview into separate design and discovery skills.
- SessionStart hook for workflow context injection.

### Removed
- Evaluate and evolve skills (superseded by audit and design).
- `disable-model-invocation` from workflow skills.
