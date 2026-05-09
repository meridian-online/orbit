# Pre-Merge Review

**Date:** 2026-05-09
**Reviewer:** Context-separated agent (fresh session)
**Branch:** drive/sidecar-layout
**Spec:** 2026-05-09-drive-rally-sidecar-layout
**Verdict:** APPROVE

---

## Test Results

| Check                                           | Result | Details                                                |
|-------------------------------------------------|--------|--------------------------------------------------------|
| Cargo workspace tests                           | PASS   | 138 passed across 8 suites                             |
| Cargo `orbit-state-core` tests                  | PASS   | 120 passed (incl. new sidecar-skip + verify tests)     |
| Cargo release build                             | PASS   | 0 errors, 1 unrelated dead-code warning                |
| `test-sidecar-layout.sh` (ac-07 smoke)          | PASS   | Steps a–e all green                                    |
| `test-setup-method.sh`                          | PASS   | METHOD.md alignment scenarios all green                |
| `orbit verify` (dev binary at HEAD)             | PASS   | `clean` against repo `.orbit/`                          |
| AC verification greps (ac-01/02/03/04)          | PASS   | All zero-hit                                           |
| `diff METHOD.md setup/METHOD.md` (ac-08d)       | PASS   | Files identical                                         |

## AC Coverage Report

The spec mixes scanner (code) ACs with doc/grep/policy ACs and an integration AC. Coverage assessed against the specific verification each AC declares.

| AC    | Status | Verification mechanism                                                                                  |
|-------|--------|---------------------------------------------------------------------------------------------------------|
| ac-00 | PASS   | `list_spec_files_skips_sidecar_shapes` (layout.rs) + `verify_excludes_sidecar_yaml_shapes` (verify.rs)  |
| ac-01 | PASS   | `grep -rnE '<…>/drive\.yaml\|\$[A-Z_]+/drive\.yaml' plugins/orb/` returns zero hits                     |
| ac-02 | PASS   | `grep -nE '<spec-id>/spec\.yaml\|\$SPEC_ID/spec\.yaml' plugins/orb/skills/drive/SKILL.md` zero hits     |
| ac-03 | PASS   | rally SKILL grep zero hits; `RALLY_DIR` collapsed; `*.rally.yaml` glob in resumption                    |
| ac-04 | PASS   | three-file review-path grep zero hits; cycle suffix preserved (`-v2`, `-v3`)                            |
| ac-05 | PASS   | drive SKILL.md line 48 detection snippet uses `$sid.drive.yaml`                                         |
| ac-06 | PASS   | `orbit verify` (dev binary) clean against repo with 11 historical bd-era folders still present          |
| ac-07 | PASS   | `plugins/orb/scripts/tests/test-sidecar-layout.sh` exits zero across steps a–e                          |
| ac-08 | PASS   | `.orbit/conventions/spec-layout.md` exists; layout.rs doc-comment cites it; METHOD.md tables identical  |
| ac-09 | PASS   | In-spec dogfood: drive sidecar + 3 review-spec cycles migrated to sidecar paths (notes.jsonl entry)     |

Cross-language test prefix scan: only ac-00 and ac-07 are code-bearing; both have explicit AC labels in their test bodies (ac-00 cited in `verify.rs:314`, ac-07 cited in `test-sidecar-layout.sh:2`). The remaining ACs are doc/grep/policy/integration; their verifications are deterministic shell commands enumerated in the AC text and re-run successfully above.

## Findings

### [LOW] Stale empty folder remains for the spec being migrated
**Category:** edge-case
**Description:** `.orbit/specs/2026-05-09-drive-rally-sidecar-layout/` exists on disk as an empty directory after the in-spec dogfood (ac-09) moved its contents to sidecar paths. The ac-09 note claims "folder removed cleanly" but the directory is still present (untracked, no children).
**Evidence:** `find .orbit/specs/2026-05-09-drive-rally-sidecar-layout/ -type f` returns empty; `git ls-files .orbit/specs/2026-05-09-drive-rally-sidecar-layout/` returns empty; `ls .orbit/specs/2026-05-09-drive-rally-sidecar-layout/` succeeds.
**Recommendation:** `rmdir .orbit/specs/2026-05-09-drive-rally-sidecar-layout/` before merge. Not a blocker — git won't track an empty directory and `orbit verify` is clean — but it contradicts the ac-09 note and leaves a stub that future agents may second-guess.

### [LOW] Choice records still cite bd-era folder paths
**Category:** test-gap
**Description:** Three choice MADRs cite spec paths in the deprecated folder form (`<id>/spec.yaml`): `0009-rally-parallel-drive-full.yaml`, `0008-rally-subagent-path-discipline.yaml`, `0004-drive-verdict-contract.yaml`. Choice 0008 also asserts the rally allowlist is `.orbit/specs/rally.yaml` — pre-rally-folder phrasing.
**Evidence:** `grep -E "drive\.yaml|rally\.yaml|spec\.yaml|specs/<" .orbit/choices/*.yaml` (output excerpted in review session).
**Recommendation:** Out of scope for this PR — choices are immutable MADR history per orbit's vocabulary, and the ACs do not enumerate `.orbit/choices/` as a migration surface. Note this for the implementer's awareness; the rally SKILL.md (line 350) already states the current allowlist correctly as `.orbit/specs/<rally-id>.rally.yaml`, so no behaviour drift.

### [LOW] User-PATH `orbit` binary predates ac-00 fix
**Category:** environment-mismatch
**Description:** `which orbit` resolves to `/home/linuxbrew/.linuxbrew/bin/orbit` reporting `0.4.3`. Running it against the working tree fails: `parse failed: unknown field 'spec_id'` on the new sidecar `.drive.yaml`. The dev binary at `orbit-state/target/release/orbit` returns clean (built from this branch, has the fix).
**Evidence:** `orbit verify` from the brew binary shows the parse failure; `./orbit-state/target/release/orbit verify --root .` returns `clean`.
**Recommendation:** Out of scope for this spec — release flow (`/orb:release`) is responsible for shipping the new binary. Flag at merge time so the post-merge release ships before any user-PATH invocation hits the new sidecar layout. Not a code fix in this PR.

---

## Honest Assessment

Strong implementation. The crux of the migration is ac-00's substrate-scanner fix — the dotless-stem filter at `list_yaml_files` is the right place to catch every caller (`list_spec_files`, `verify_all` via the same helper, `Index::rebuild_from_files` via `list_spec_files`), and both new unit tests pin the contract: a non-Spec-shaped `<id>.drive.yaml` must (a) not surface in `list_spec_files`, (b) not break `verify_all`. The smoke test (ac-07) extends the same coverage end-to-end through the CLI surfaces. The code change is small, self-contained, and dogfooded in-spec.

The doc/grep ACs (ac-01..05, ac-08) are mechanically verifiable and all greps return zero hits. The conventions doc (`.orbit/conventions/spec-layout.md`) and the synced METHOD.md vocab tables (ac-08c/d) close the loop on canonicalisation. ac-09's dogfood — migrating this very spec's drive sidecar and three review-spec cycle files from folder to sidecar layout — is convincing evidence the sequencing held: sidecar yaml on disk + scanner fix + `orbit verify` clean is reproducible.

Three minor blemishes, none merge-blocking: an empty folder stub from the in-spec dogfood (rmdir before merge), MADR choice records that still cite bd-era paths (out of scope; choices are immutable), and the brew-installed `orbit` binary needs a release to catch up. The empty-folder stub is the only one that might briefly confuse a future reader of `.orbit/specs/`.

The implementer's deviation note (cycle-3 polish-tier RC treated as effective APPROVE) is well-reasoned and follows precedent from `2026-05-09-orbit-method-md`. Author approval is recorded in the drive sidecar.

Recommend merge. Run `rmdir .orbit/specs/2026-05-09-drive-rally-sidecar-layout/` first to keep the substrate tidy.
