---
name: release
description: Bump orbit plugin version, commit, push, and reload into Claude Code
user-invocable: true
---

# /orb:release

Release a new version of the orbit plugin so `/reload-plugins` picks up changes.

## Why This Exists

The Claude Code plugin system caches plugins by version. If you add skills, hooks, or scripts without bumping the version in `plugin.json`, `/reload-plugins` sees the same version and skips the update. This skill ensures the version bump, commit, push, and cache refresh all happen together.

## Usage

```
/orb:release <bump>
```

Where `<bump>` is one of:
- `patch` (default) — e.g. 0.2.2 → 0.2.3
- `minor` — e.g. 0.2.3 → 0.3.0
- `major` — e.g. 0.3.0 → 1.0.0

If no argument is given, default to `patch`.

## Instructions

### 1. Pre-flight Checks

Run these checks from the orbit repo's working tree (the skill expects to be invoked from inside the orbit repo — confirm `git rev-parse --show-toplevel` resolves and the basename is `orbit`):

1. `git status` — must be clean (no uncommitted changes). If dirty, stop and tell the user to commit first.
2. `git log --oneline -5` — show recent commits so the user can verify what's being released.
3. Read `plugins/orb/.claude-plugin/plugin.json` to get the current version.
4. **Substrate-binary parity gate** (HARD BLOCK). Plugin skill prose can quietly assume substrate behaviour that lives in the `orbit` Rust binary on PATH. If `orbit-state/` changed in this release window but the user's PATH `orbit` predates the change, shipping the skill prose alone breaks `orbit verify` for any terminal still on the older binary.

   Run:

   ```bash
   git log --oneline "$(git log --all --grep='Bump version to' -1 --format=%H)..HEAD" -- orbit-state/ | head
   ```

   - **No commits** → no substrate change in this window. Skip the rest of step 4.
   - **Commits exist** → substrate touched. Proceed with the parity check:

   ```bash
   PATH_ORBIT_VERSION=$(orbit --version 2>/dev/null | awk '{print $2}')
   ```

   Compare `PATH_ORBIT_VERSION` against the version we are about to bump TO. If `PATH_ORBIT_VERSION` is missing, OR strictly less than the new version, **REFUSE the release**. Output:

   ```
   BLOCKED: orbit-state changed in this release window but the orbit binary on PATH is at <PATH_ORBIT_VERSION>; about to release <NEW_VERSION>.

   Substrate changes shipped in skill prose without a current binary will break `orbit verify` for users on stale builds (this includes your own terminals).

   Resolve by ONE of:
     (a) Rebuild and reinstall the orbit binary at-or-above <NEW_VERSION> (brew formula bump, cargo install, etc).
     (b) Set `ORBIT=<repo>/orbit-state/target/release/orbit` in any terminals running orbit commands, and confirm dev-binary version >= <NEW_VERSION>.
     (c) Re-run with `--accept-binary-lag` ONLY if you have explicitly determined the substrate change is forward-compatible (the old binary still works against the new skill prose). Document the rationale in the changelog.

   Do NOT proceed without one of the three above.
   ```

   Stop. The release is a no-op until resolved.

5. **Topology drift surface (non-blocking).** Run `orbit audit topology` (only when `.orbit/config.yaml` exists and `docs.topology` is configured — the audit auto-detects this and exits cleanly with a "topology capability not configured" envelope when absent).

   - **Envelope `topology_drift` empty or absent** → no-op.
   - **Envelope `topology_drift` has ≤ 10 entries** → surface the full list verbatim with a one-line framing: `topology drift surfaced — review before bumping`. Operator can proceed regardless (release is NOT gated by topology drift).
   - **Envelope `topology_drift` has > 10 entries** → summarise rather than dump: `<N> topology drift items; run \`orbit audit topology\` for the full list` followed by the first ten entries only. Truncation rule keeps the pre-bump checklist readable.

### 2. Generate Changelog Entry

Collect the commits since the last version bump:

```bash
git log --oneline $(git log --oneline --all --grep="Bump version to" -1 --format=%H)..HEAD
```

Summarise these commits into a changelog entry following [Keep a Changelog](https://keepachangelog.com/) format, grouped by `Added`, `Changed`, `Fixed`, `Removed` as applicable. Write concise, user-facing descriptions — not commit messages verbatim.

Prepend the new entry to `CHANGELOG.md` (after the header, before the previous release). Use today's date.

### 3. Bump the Version

Parse the current version string (MAJOR.MINOR.PATCH) and apply the requested bump:

- `patch`: increment PATCH
- `minor`: increment MINOR, reset PATCH to 0
- `major`: increment MAJOR, reset MINOR and PATCH to 0

Update **all** of the following to the new version — the plugin and the substrate binary release in lockstep, by convention:

- `plugins/orb/.claude-plugin/plugin.json` — the `version` field
- `orbit-state/Cargo.toml` — `[workspace.package] version`

Then refresh the lockfile and reinstall the binary so the local `orbit` on PATH matches the about-to-be-released version. This both satisfies §1.4's parity gate retroactively and verifies the substrate compiles before the release commit lands:

```bash
( cd orbit-state && cargo build --release --quiet && cargo install --path crates/cli --quiet )
orbit --version    # must print the new version
```

If `cargo build` fails or `orbit --version` doesn't print the new version, **stop**. Fix the build before continuing — a release tag pushed against an uncompiling tree triggers the workflow against broken source.

### 4. Commit, Tag, and Push

The version-bump commit is the one that gets tagged. The tag is what triggers `.github/workflows/release.yml` to build cross-platform binaries and update `meridian-online/homebrew-tap` — without it, the plugin lands on the marketplace cache but the brew formula stays on the previous version.

```bash
git add plugins/orb/.claude-plugin/plugin.json CHANGELOG.md \
        orbit-state/Cargo.toml orbit-state/Cargo.lock
git commit -m "Bump version to <new_version>"
git push origin main

# Tag the bump commit and push it — release.yml triggers on `v*` tag push.
git tag "v<new_version>"
git push origin "v<new_version>"
```

Watch the workflow to completion before considering the release done. The workflow updates the brew tap on success; until it finishes, `brew upgrade meridian-online/tap/orbit` will report the old version. A typical run takes a few minutes (cross-platform builds + tap update):

```bash
RUN_ID=$(gh run list --workflow=release.yml --limit 1 --json databaseId --jq '.[0].databaseId')
gh run watch "$RUN_ID" --exit-status
```

If `gh run watch` exits non-zero, surface the failure to the user and **do not proceed to step 5** — the marketplace cache shouldn't advertise a new version whose binary distribution failed.

### 5. Update the Marketplace Cache

Pull the latest into the marketplace repo that Claude Code reads from:

```bash
git -C ~/.claude/plugins/marketplaces/orbit pull origin main
```

### 6. Update the Install Record

The file `~/.claude/plugins/installed_plugins.json` tracks which version and cache path is active. If it still points to the old version, `/reload-plugins` will load from the old cache and miss new skills.

Update the `orb@orbit` entry:
- `installPath` → point to the new version cache directory (e.g. `~/.claude/plugins/cache/orbit/orb/<new_version>`)
- `version` → the new version string
- `gitCommitSha` → the new HEAD SHA from the marketplace repo (`git -C ~/.claude/plugins/marketplaces/orbit rev-parse HEAD`)
- `lastUpdated` → current ISO 8601 timestamp

### 7. Confirm

Tell the user:

```
Released orbit v<new_version>.
Run /reload-plugins to pick up the new version.
```

Show the changelog entry that was just added.

If the §1.4 substrate-binary parity gate was triggered (substrate changed in this release window AND the user resolved it), restate the resolution path used so the binary state is part of the release record:

```
Binary state: PATH `orbit` at <PATH_ORBIT_VERSION>; resolved via <a|b|c>.
```

If the gate did NOT fire (no orbit-state changes), state that explicitly:

```
Binary state: orbit-state unchanged this window; PATH binary version not gated.
```
