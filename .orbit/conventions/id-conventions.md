# Id conventions

Per choice 0022, orbit's four canonical entity types use three distinct id-shape families. Each shape matches its entity's lifecycle.

## The three families

| Family       | Entities          | Shape                  | Example                                    |
|--------------|-------------------|------------------------|--------------------------------------------|
| Enumerated   | cards, choices    | `NNNN-slug`            | `0008-consolidated-orbit-artefact-folder`  |
| Dated        | specs             | `YYYY-MM-DD-slug`      | `2026-05-10-card-id-field-and-conventions` |
| Keyed        | memories          | `key`                  | `four-pillars`                             |

**Enumerated** ids are allocated at creation, monotonically increased per type, padded to four digits. The set is countable and orderable; "card 31" is the 31st card.

**Dated** ids embed creation time. Specs are temporal — the date is part of the identity. Multiple specs may share a date; the slug disambiguates.

**Keyed** ids are free-form lookup keys for an associative store. Memories aren't enumerated and don't need temporal ordering; they need to be retrieved by topic.

## YAML field names

| Entity   | Field      | Value form                                                |
|----------|------------|-----------------------------------------------------------|
| Card     | `id`       | full slug (`0008-consolidated-orbit-artefact-folder`)     |
| Choice   | `id`       | numeric prefix only (`'0021'`) — the slug is in the title |
| Spec     | `id`       | full date-slug (`2026-05-10-card-id-field-and-conventions`) |
| Memory   | `key`      | the lookup key (`four-pillars`)                           |

The asymmetry between `Card.id` (full slug) and `Choice.id` (numeric prefix only) is intentional and pre-dates choice 0022: choices carry their human-readable label in the `title` field, so the `id` field only needs the index. Cards have no separate title field, so `id` carries the full slug.

## Reference style — agent-to-author prose

Bare numeric references must be type-qualified. Three entity types share the NNNN namespace, so `0008` alone is ambiguous (`cards/0008-consolidated-orbit-artefact-folder.yaml` and `choices/0008-rally-subagent-path-discipline.yaml` both exist).

Required forms:

- `card 0008` (or `card 8`) — cards
- `choice 0008` (or `choice 8`) — choices
- `spec 2026-05-10-card-id-field-and-conventions` — specs
- `memory four-pillars` — memories

The bare-NNNN shorthand drops leading zeros in prose (`card 8`, not `card 0008`) but stays padded in filenames and yaml fields (`0008-`).

## CLI lookup

Per choice 0022, `orbit card show` and `orbit choice show` accept three forms:

- Full slug: `orbit card show 0008-consolidated-orbit-artefact-folder`
- Padded NNNN: `orbit card show 0008`
- Bare NNNN: `orbit card show 8`

Bare and padded forms resolve via filename prefix-match. Errors:

- Zero matches → `not-found`
- Two or more matches → `ambiguous` (should never happen within a type's NNNN sequence; the error path exists for defence-in-depth)

`orbit spec show <date-slug>` and `orbit memory ...` keep their existing lookup form (no prefix-match — date-slugs and keys aren't numeric).

## Cross-references

- Choice 0022 (`.orbit/choices/0022-entity-id-conventions.yaml`) — the rationale and decision record.
- `Card`, `Choice`, `Spec`, `Memory` structs in `orbit-state/crates/core/src/schema.rs` — the schema.
- `resolve_numeric_slug` in `orbit-state/crates/core/src/verbs.rs` — the prefix-match resolver.
