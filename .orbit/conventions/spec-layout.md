# Spec layout convention

Orbit-state v0.1 stores per-spec data as flat sidecars under
`.orbit/specs/`. Every artefact tied to a spec uses the spec's id as
its filename prefix and a sidecar suffix describing the artefact type.

## Sidecar inventory

| Path                                              | Purpose                                                              |
|---------------------------------------------------|----------------------------------------------------------------------|
| `.orbit/specs/<id>.yaml`                          | The spec itself — goal, status, cards, labels, `acceptance_criteria` |
| `.orbit/specs/<id>.tasks.jsonl`                   | Append-only task event stream                                        |
| `.orbit/specs/<id>.notes.jsonl`                   | Append-only timestamped notes                                        |
| `.orbit/specs/<id>.drive.yaml`                    | Drive orchestration state (single-card drive)                        |
| `.orbit/specs/<id>.rally.yaml`                    | Rally orchestration state (multi-card rally lead)                    |
| `.orbit/specs/<id>.decisions.md`                  | Rally per-child decision pack (Stage 2 output)                       |
| `.orbit/specs/<id>.interview.md`                  | Design interview record (feeds the spec's ACs)                       |
| `.orbit/specs/<id>.review-spec-<date>.md`         | Spec review verdict (cycle 1 — no suffix)                            |
| `.orbit/specs/<id>.review-spec-<date>-v2.md`      | Spec review verdict (cycle 2)                                        |
| `.orbit/specs/<id>.review-spec-<date>-v3.md`      | Spec review verdict (cycle 3)                                        |
| `.orbit/specs/<id>.review-pr-<date>.md`           | PR review verdict (cycle 1)                                          |
| `.orbit/specs/<id>.review-pr-<date>-v2.md`        | PR review verdict (cycle 2)                                          |
| `.orbit/specs/<id>.review-pr-<date>-v3.md`        | PR review verdict (cycle 3)                                          |

The cycle-suffix convention: cycle 1 has no suffix; cycles 2 and 3
append `-v2` / `-v3` before the `.md` extension.

## Substrate-scanner rule

`list_yaml_files` in `orbit-state/crates/core/src/layout.rs` filters
spec YAML loads to **dotless-stem files only**:

- `<id>.yaml` — keeps (stem `<id>` is dotless)
- `<id>.drive.yaml` — skips (stem `<id>.drive` contains `.`)
- `<id>.rally.yaml` — skips (stem `<id>.rally` contains `.`)

This filter is consumed by `verify_all`, `Index::rebuild_from_files`,
and `verbs::spec.list` — so adding a new sidecar yaml shape requires
no scanner changes; the dotless-stem rule excludes it automatically.

## Deprecated bd-era folder layout

Earlier orbit-state revisions placed per-spec artefacts inside
per-spec folders:

- `.orbit/specs/<id>/spec.yaml`     (now `.orbit/specs/<id>.yaml`)
- `.orbit/specs/<id>/drive.yaml`    (now `.orbit/specs/<id>.drive.yaml`)
- `.orbit/specs/<id>/decisions.md`  (now `.orbit/specs/<id>.decisions.md`)
- `.orbit/specs/<id>/interview.md`  (now `.orbit/specs/<id>.interview.md`)
- `.orbit/specs/<id>/review-spec-<date>.md`  (now `.orbit/specs/<id>.review-spec-<date>.md`)
- `.orbit/specs/<date>-<slug>-rally/rally.yaml`  (now `.orbit/specs/<date>-<slug>-rally.rally.yaml`)

The folder layout is **deprecated**. Existing on-disk folders from
drives and rallies that ran before the migration remain in place as
historical artefacts (per spec
`2026-05-09-drive-rally-sidecar-layout` ac-06). New drives and rallies
use the sidecar layout exclusively. The substrate scanner only loads
spec YAML from the flat `.orbit/specs/*.yaml` glob with the
dotless-stem filter, so old folders do not interfere with `orbit
verify` or `orbit spec list`.
