# `verify_all` swallows future-version migration errors

**Date:** 2026-05-16
**Triggered by:** `/orb:review-pr` LOW finding on spec `2026-05-16-ac-taxonomy`

## What the finding is

`verify.rs:92` calls `let _ = ensure_current(layout);` — discarding the migration runner's return value. The discard is documented as "Errors here surface as round-trip failures on the schema-version path so the caller sees a single channel of diagnostics rather than a mid-run abort." That contract holds for KNOWN-older-version inputs (the migration walks the chain to current and writes the new schema-version file; if a step fails mid-chain, the next round-trip check picks up the half-migrated state).

It DOESN'T hold for future-versioned schema-version files. A user running an older orbit binary against a newer tree hits this path:

1. `ensure_current` calls `migrations::run(layout, CURRENT_SCHEMA_VERSION)`.
2. `run` reads on-disk `version: '0.4'` (say), checks `is_known_version` against the registry — neither a registry source nor the target — returns `Error::malformed("schema-version file has unknown version `0.4`; known versions: 0.1, 0.2, 0.3")`.
3. `verify.rs:92` discards that error.
4. The subsequent round-trip check on the schema-version file parses `version: '0.4'` cleanly (it's a valid `SchemaVersion`), reserialises identically, no drift detected.
5. `verify_all` returns `VerifyOutcome::default()` — green light.

The user gets a "verify clean" signal against a substrate the binary cannot operate on. The immediate next verb (any other one) will fail when it tries to read entities the binary doesn't understand, but `orbit verify` itself misled.

## Why this is LOW, not MEDIUM

- `verify_all` doesn't promise "the binary can operate on this tree" — it promises "every canonical file round-trips cleanly under the schema this binary knows." That's literally true.
- The failure mode requires a specific operator sequence: install an older orbit, then point it at a newer tree, then run `orbit verify` and trust the result. It's neither common nor a silent data-loss path — the next non-verify verb fails loudly.
- Fix is small: change `verify.rs:92` to surface the error as a synthetic `RoundTripFailure` of kind `ParseFailed`, so the future-version case lands in the existing diagnostics channel rather than being silently dropped.

## Suggested fix sketch

```rust
match ensure_current(layout) {
    Ok(_) => {}
    Err(e) => {
        outcome.round_trip_failures.push(RoundTripFailure {
            path: layout.schema_version_file(),
            kind: RoundTripFailureKind::ParseFailed(format!(
                "schema-version unrunnable: {}", e
            )),
        });
    }
}
```

Then `outcome.has_failures()` returns true and the caller's exit code reflects reality.

## Why this isn't part of 2026-05-16-ac-taxonomy

Pre-existing behaviour. The discard pattern was introduced in spec `2026-05-15-agent-learning-loop` (when `init_schema_version` was first wired into `verify_all`). This spec's ac-04 changed `init_schema_version` to `ensure_current` but kept the same discard pattern — extending the latent bug, not introducing it.

Filing as a memo so the next session sees the finding cold and can either distill into a card (small, well-scoped) or attach to an existing card (probably 0020-orbit-state — substrate integrity).
