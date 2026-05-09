---
name: release
description: Bump orbit plugin version, commit, push, and reload into Claude Code
user-invocable: true
model: sonnet
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

Update `plugins/orb/.claude-plugin/plugin.json` with the new version.

### 4. Commit and Push

```bash
git add plugins/orb/.claude-plugin/plugin.json CHANGELOG.md
git commit -m "Bump version to <new_version>"
git push origin main
```

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
