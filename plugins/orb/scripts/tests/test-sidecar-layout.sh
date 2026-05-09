#!/usr/bin/env bash
# test-sidecar-layout.sh — smoke test for spec 2026-05-09-drive-rally-sidecar-layout ac-07.
#
# Verifies the sidecar-layout substrate end-to-end against a synthetic card under
# a temp --root. The test verifies path SHAPE — that the substrate accepts the
# new sidecar paths and the scanner correctly excludes them from spec parsing —
# not the SKILL.md snippet bodies.
#
# Steps:
#   (a) Promote a synthetic card → flat spec at .orbit/specs/<spec-id>.yaml.
#   (b) Write a drive sidecar at .orbit/specs/<spec-id>.drive.yaml; test path-shape
#       detection — `[[ -f $SID.drive.yaml ]]` — confirming the sidecar is reachable.
#   (c) Write a rally sidecar at .orbit/specs/<rally-id>.rally.yaml; iterate the
#       glob `for f in .orbit/specs/*.rally.yaml`, confirming the rally is reached.
#   (d) `orbit verify` returns clean (proves the scanner-fix excludes both shapes).
#   (e) `orbit spec list` does NOT surface the sidecar ids as specs.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
PROMOTE="$REPO_ROOT/plugins/orb/scripts/promote.sh"

# Resolve the orbit binary — prefer an explicit ORBIT env, then the dev build,
# then PATH. The dev build is required for the scanner-fix to take effect; a
# brew-installed orbit predating ac-00 will fail the verify/spec-list gates.
if [[ -n "${ORBIT:-}" ]]; then
  ORBIT_BIN="$ORBIT"
elif [[ -x "$REPO_ROOT/orbit-state/target/release/orbit" ]]; then
  ORBIT_BIN="$REPO_ROOT/orbit-state/target/release/orbit"
elif command -v orbit >/dev/null 2>&1; then
  ORBIT_BIN=$(command -v orbit)
else
  echo "FAIL: cannot find an orbit binary (set ORBIT, build orbit-state, or install via brew)" >&2
  exit 1
fi

if [[ ! -x "$PROMOTE" ]]; then
  echo "FAIL: promote.sh not found or not executable at $PROMOTE" >&2
  exit 1
fi

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

mkdir -p "$TMP/.orbit/cards"

CARD_SLUG="0001-sidecar-smoke"
CARD_PATH="$TMP/.orbit/cards/${CARD_SLUG}.yaml"
cat > "$CARD_PATH" <<'EOF'
feature: Sidecar smoke
as_a: smoke test
i_want: a synthetic card to drive promote.sh against a temp --root
so_that: the sidecar-layout smoke test has a real card to consume
goal: verify the sidecar-layout migration end-to-end under a temp root
maturity: planned
scenarios:
- name: First scenario
  given: a fresh substrate
  when: promote runs
  then: a flat spec is produced
  gate: false
- name: Second scenario
  given: a flat spec
  when: a drive sidecar is written
  then: the scanner ignores it
  gate: false
- name: Third scenario
  given: a rally sidecar
  when: orbit verify runs
  then: the scanner returns clean
  gate: false
EOF

echo "=== Step (a): promote synthetic card ==="
SPEC_ID=$("$PROMOTE" "$CARD_PATH" --root "$TMP")
SPEC_PATH="$TMP/.orbit/specs/${SPEC_ID}.yaml"
if [[ ! -f "$SPEC_PATH" ]]; then
  echo "FAIL (a): expected flat spec at $SPEC_PATH, missing" >&2
  exit 1
fi
echo "  PASS (a): promote produced flat spec id=$SPEC_ID"

echo "=== Step (b): drive sidecar path detection ==="
DRIVE_SIDECAR="$TMP/.orbit/specs/${SPEC_ID}.drive.yaml"
cat > "$DRIVE_SIDECAR" <<EOF
spec_id: $SPEC_ID
card_path: $CARD_PATH
autonomy: guided
iteration: 1
stage: review-spec
review_spec_cycle: 0
review_spec_date: null
review_pr_cycle: 0
review_pr_date: null
iteration_history: []
EOF

# Path-shape detection mirroring drive/SKILL.md §Input contract no-argument flow
sid="$SPEC_ID"
if [[ -f "$TMP/.orbit/specs/${sid}.drive.yaml" ]]; then
  echo "  PASS (b): drive sidecar reachable via [[ -f *.drive.yaml ]]"
else
  echo "FAIL (b): drive sidecar not reachable at sidecar path" >&2
  exit 1
fi

echo "=== Step (c): rally sidecar glob iteration ==="
RALLY_ID="2026-05-09-smoke-rally"
RALLY_SIDECAR="$TMP/.orbit/specs/${RALLY_ID}.rally.yaml"
cat > "$RALLY_SIDECAR" <<EOF
rally_id: $RALLY_ID
goal: smoke
autonomy: guided
phase: approved
started: 2026-05-09T00:00:00Z
completed: null
children: []
EOF

# Glob iteration mirroring rally/SKILL.md §Input contract no-argument flow
found_rally=""
for f in "$TMP/.orbit/specs/"*.rally.yaml; do
  [[ -f "$f" ]] || continue
  found_rally=$(basename "$f" .rally.yaml)
done
if [[ "$found_rally" == "$RALLY_ID" ]]; then
  echo "  PASS (c): rally sidecar reached via *.rally.yaml glob"
else
  echo "FAIL (c): rally glob did not surface $RALLY_ID (got '$found_rally')" >&2
  exit 1
fi

echo "=== Step (d): orbit verify clean with sidecars on disk ==="
if "$ORBIT_BIN" --root "$TMP" verify >/tmp/sidecar-verify.out 2>&1; then
  echo "  PASS (d): orbit verify returned clean"
else
  echo "FAIL (d): orbit verify failed with sidecars on disk:" >&2
  cat /tmp/sidecar-verify.out >&2
  exit 1
fi

echo "=== Step (e): orbit spec list excludes sidecar ids ==="
LIST_OUT=$("$ORBIT_BIN" --root "$TMP" --json spec list 2>/dev/null)
ids=$(echo "$LIST_OUT" | python3 -c 'import sys, json; d=json.load(sys.stdin); print("\n".join(s["id"] for s in d["data"]["result"]["specs"]))')
if echo "$ids" | grep -qx "$SPEC_ID"; then
  : # primary spec is expected to surface
else
  echo "FAIL (e): primary spec $SPEC_ID missing from spec.list output" >&2
  echo "$LIST_OUT" >&2
  exit 1
fi
if echo "$ids" | grep -qx "${SPEC_ID}.drive"; then
  echo "FAIL (e): drive sidecar surfaced as spec id" >&2
  exit 1
fi
if echo "$ids" | grep -qx "${RALLY_ID}.rally"; then
  echo "FAIL (e): rally sidecar surfaced as spec id" >&2
  exit 1
fi
if echo "$ids" | grep -qx "$RALLY_ID"; then
  echo "FAIL (e): rally sidecar surfaced as spec id (without .rally suffix)" >&2
  exit 1
fi
echo "  PASS (e): spec.list returned only the primary spec; sidecars absent"

echo
echo "=== test-sidecar-layout: ALL PASS ==="
