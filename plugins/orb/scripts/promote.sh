#!/usr/bin/env bash
# promote.sh — card-to-spec promotion against the orbit-state substrate.
#
# Reads a card YAML and creates an orbit spec whose acceptance_criteria mirror
# the card's scenarios. The spec is materialised at
# .orbit/specs/<spec-id>/spec.yaml (folder-sidecar layout — per choice 0021
# and .orbit/conventions/spec-layout.md).
#
# Pipeline:
#   1. Parse the card (python3, no yq dependency).
#   2. Derive spec id `<YYYY-MM-DD>-<card-slug>` and card id from the filename.
#   3. `orbit spec create <id> <goal> --card <card-id>` — creates the sidecar
#      folder with spec.yaml inside and empty acceptance_criteria.
#   4. Replace acceptance_criteria with one entry per scenario, preserving
#      `gate` and seeding `checked: false`.
#   5. `orbit canonicalise` — fix any byte drift introduced by the direct edit.
#
# Stdout: the created spec id (one line, no trailing whitespace) — preserving
# the contract that drive/rally call sites depend on.
#
# Usage:
#   promote.sh <card-path> [--dry-run] [--root <path>]

set -euo pipefail

usage() {
  cat >&2 <<'EOF'
Usage: promote.sh <card-path> [--dry-run] [--root <path>]

Options:
  --dry-run      Print the planned spec id, goal, card id, and AC table
                 without creating anything.
  --root <path>  Pass through to orbit (defaults to current directory).
EOF
  exit 2
}

if [[ $# -lt 1 ]]; then
  usage
fi

card_path="$1"
shift

dry_run=0
root=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run) dry_run=1; shift ;;
    --root) root="$2"; shift 2 ;;
    *) echo "promote.sh: unknown option: $1" >&2; usage ;;
  esac
done

if [[ ! -f "$card_path" ]]; then
  echo "promote.sh: card not found: $card_path" >&2
  exit 2
fi

# Resolve the card's absolute path so derivations are stable regardless of CWD.
card_abs=$(cd "$(dirname "$card_path")" && pwd)/$(basename "$card_path")

# Parse the card via python3. Emit a single JSON blob covering everything
# downstream needs: goal, scenarios, derived ids.
card_meta=$(CARD_PATH="$card_abs" python3 - <<'PY'
import json, os, re, sys
from datetime import date

import yaml

card_path = os.environ["CARD_PATH"]
with open(card_path) as f:
    card = yaml.safe_load(f) or {}

goal = (card.get("goal") or "").strip()
scenarios = card.get("scenarios") or []
basename = os.path.basename(card_path)
if basename.endswith(".yaml"):
    basename = basename[:-5]

card_id = basename
slug = re.sub(r"^\d+-", "", basename)
spec_id = f"{date.today().isoformat()}-{slug}"

ac_rows = []
for i, s in enumerate(scenarios, 1):
    name = (s.get("name") or "").strip()
    then_clause = (s.get("then") or "").strip()
    description = f"{name} — {then_clause}" if then_clause else name
    ac_rows.append({
        "id": f"ac-{i:02d}",
        "description": description,
        "gate": bool(s.get("gate", False)),
    })

print(json.dumps({
    "card_id": card_id,
    "spec_id": spec_id,
    "goal": goal,
    "ac_rows": ac_rows,
}))
PY
)

card_id=$(printf '%s' "$card_meta" | python3 -c "import sys,json; print(json.load(sys.stdin)['card_id'])")
spec_id=$(printf '%s' "$card_meta" | python3 -c "import sys,json; print(json.load(sys.stdin)['spec_id'])")
goal=$(printf '%s' "$card_meta" | python3 -c "import sys,json; print(json.load(sys.stdin)['goal'])")
ac_count=$(printf '%s' "$card_meta" | python3 -c "import sys,json; print(len(json.load(sys.stdin)['ac_rows']))")

if [[ -z "$goal" ]]; then
  echo "promote.sh: card has no goal: $card_path" >&2
  exit 2
fi

if [[ "$ac_count" -eq 0 ]]; then
  echo "promote.sh: card has no scenarios: $card_path" >&2
  exit 2
fi

# Pre-build the orbit invocation so dry-run and real paths share the args.
orbit_root_args=()
if [[ -n "$root" ]]; then
  orbit_root_args=(--root "$root")
fi

if [[ "$dry_run" -eq 1 ]]; then
  echo "=== DRY RUN ==="
  echo "Spec id: $spec_id"
  echo "Card id: $card_id"
  echo "Goal:    $goal"
  echo ""
  echo "Acceptance criteria:"
  printf '%s' "$card_meta" | python3 -c "
import sys, json
rows = json.load(sys.stdin)['ac_rows']
for row in rows:
    flag = '[gate]' if row['gate'] else '      '
    print(f\"  {row['id']} {flag} {row['description']}\")
"
  exit 0
fi

# Create the spec via orbit.
create_envelope=$(orbit "${orbit_root_args[@]}" --json spec create \
  "$spec_id" "$goal" --card "$card_id" 2>&1) || {
  echo "promote.sh: orbit spec create failed:" >&2
  echo "$create_envelope" >&2
  exit 2
}

# Resolve the spec's on-disk path (root may be elsewhere). Folder-sidecar
# layout: spec.yaml lives inside .orbit/specs/<id>/.
root_for_path="${root:-$(pwd)}"
spec_path="$root_for_path/.orbit/specs/$spec_id/spec.yaml"

if [[ ! -f "$spec_path" ]]; then
  echo "promote.sh: orbit spec create did not produce expected file: $spec_path" >&2
  echo "$create_envelope" >&2
  exit 2
fi

# Replace the empty acceptance_criteria array with one entry per scenario.
SPEC_PATH="$spec_path" CARD_META="$card_meta" python3 - <<'PY'
import json, os, sys

import yaml

spec_path = os.environ["SPEC_PATH"]
ac_rows = json.loads(os.environ["CARD_META"])["ac_rows"]

with open(spec_path) as f:
    spec = yaml.safe_load(f) or {}

spec["acceptance_criteria"] = [
    {
        "id": row["id"],
        "description": row["description"],
        "gate": row["gate"],
        "checked": False,
    }
    for row in ac_rows
]

with open(spec_path, "w") as f:
    yaml.safe_dump(spec, f, sort_keys=False, allow_unicode=True, width=10**9)
PY

# Run canonicalise to normalise byte form. orbit canonicalise rewrites in place.
canonicalise_out=$(orbit "${orbit_root_args[@]}" canonicalise 2>&1) || {
  echo "promote.sh: orbit canonicalise failed:" >&2
  echo "$canonicalise_out" >&2
  exit 2
}

# Stdout contract: just the spec id.
printf '%s\n' "$spec_id"
