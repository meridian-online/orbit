#!/usr/bin/env bash
# setup-topology.sh — implements /orb:setup §6d (topology scaffolding).
#
# Pipeline (per spec 2026-05-18-topology-substrate-wires, ac-01):
#   1. Detect whether .orbit/config.yaml exists and whether it carries
#      docs.topology. If both present, no-op (idempotent).
#   2. Otherwise prompt to wire the topology capability. Decline → leave
#      unconfigured; the rest of orbit still works.
#   3. On accept:
#        a. Ensure .orbit/config.yaml exists and carries
#           docs.topology: docs/topology.md (default).
#        b. If the target path's parent directory does not exist, create
#           the directory tree before writing the stub.
#        c. If no file exists at the target path, create a stub
#           (heading + one-paragraph explainer + empty entry list).
#        d. If a file already exists at the target path, wire the pointer
#           but do NOT overwrite the existing file.
#
# Usage:
#   setup-topology.sh --project-root <path> [--answer-wire y|n]
#
# Test affordance:
#   --answer-wire    scripts the wire-topology prompt (default: interactive).

set -euo pipefail

usage() {
  cat >&2 <<'EOF'
Usage: setup-topology.sh --project-root <path> [--answer-wire y|n]

Required:
  --project-root <path>   Project root containing .orbit/

Optional:
  --answer-wire y|n       Script the wire-topology prompt (default: interactive).
EOF
  exit 2
}

project_root=""
answer_wire=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --project-root) project_root="$2"; shift 2 ;;
    --answer-wire) answer_wire="$2"; shift 2 ;;
    *) echo "setup-topology.sh: unknown option: $1" >&2; usage ;;
  esac
done

if [[ -z "$project_root" ]]; then
  usage
fi

if [[ ! -d "$project_root" ]]; then
  echo "setup-topology.sh: project root not found: $project_root" >&2
  exit 2
fi

config_yaml="$project_root/.orbit/config.yaml"
default_topology_rel="docs/topology.md"

# Check whether docs.topology is already wired.
already_wired=0
if [[ -f "$config_yaml" ]]; then
  if grep -Eq '^[[:space:]]*topology:[[:space:]]*\S' "$config_yaml"; then
    # A topology line under any parent (likely docs.topology) — treat as wired.
    # The strict parse lives in orbit-state; this is a coarse check sufficient
    # for the prompt-or-no-op decision.
    already_wired=1
  fi
fi

if [[ "$already_wired" -eq 1 ]]; then
  # Idempotent path — pointer is set. Still ensure the stub exists if the
  # target file is absent (brownfield-accept rule still applies on re-run).
  topology_rel=$(awk '
    /^docs:/ { in_docs=1; next }
    in_docs && /^[^[:space:]]/ { in_docs=0 }
    in_docs && /^[[:space:]]+topology:/ {
      sub(/^[[:space:]]+topology:[[:space:]]*/, "")
      sub(/[[:space:]]*$/, "")
      print; exit
    }
  ' "$config_yaml")
  if [[ -z "$topology_rel" ]]; then
    topology_rel="$default_topology_rel"
  fi
  topology_path="$project_root/$topology_rel"
  if [[ ! -e "$topology_path" ]]; then
    topology_parent="$(dirname "$topology_path")"
    if [[ ! -d "$topology_parent" ]]; then
      mkdir -p "$topology_parent"
    fi
    cat > "$topology_path" <<'STUB'
# Topology

This document is the architecture-level analogue of /orb:code-investigate — a subsystem-keyed index that points to the canonical sources (authoritative code, owning decision record, operational doc, test surface) for each subsystem. Agents reach for it via /orb:topology before reasoning about how a multi-file capability works.

## Entries
STUB
    echo "orbit: created topology stub at $topology_rel."
  fi
  exit 0
fi

# Not wired — prompt to add.
echo "orbit: topology capability not wired (docs.topology absent from .orbit/config.yaml)."
echo "orbit: wiring scaffolds .orbit/config.yaml with docs.topology: $default_topology_rel and creates a stub at that path."

if [[ -n "$answer_wire" ]]; then
  answer="$answer_wire"
else
  read -r -p "Wire topology now? (y/N) " answer || answer=""
fi

case "${answer,,}" in
  y|yes)
    : # fall through to scaffold
    ;;
  *)
    echo "orbit: topology capability left unconfigured."
    exit 0
    ;;
esac

# Ensure .orbit/ exists.
mkdir -p "$project_root/.orbit"

# Scaffold or extend .orbit/config.yaml.
if [[ ! -f "$config_yaml" ]]; then
  cat > "$config_yaml" <<EOF
docs:
  topology: $default_topology_rel
EOF
else
  # Config exists but lacks docs.topology. Add it under an existing or new docs: section.
  if grep -Eq '^docs:[[:space:]]*$' "$config_yaml"; then
    # docs: block exists; insert topology under it (after the docs: line).
    python3 - "$config_yaml" "$default_topology_rel" <<'PY'
import re, sys
path, value = sys.argv[1], sys.argv[2]
with open(path) as f:
    text = f.read()
# Insert "  topology: <value>\n" after the "docs:" line.
text = re.sub(r'(^docs:[\t ]*\n)', r'\1  topology: ' + value + '\n', text, count=1, flags=re.MULTILINE)
with open(path, 'w') as f:
    f.write(text)
PY
  else
    # No docs: block. Append one at end-of-file.
    [[ -s "$config_yaml" && "$(tail -c1 "$config_yaml")" == $'\n' ]] || printf '\n' >> "$config_yaml"
    cat >> "$config_yaml" <<EOF
docs:
  topology: $default_topology_rel
EOF
  fi
fi

# Brownfield-accept: scaffold the stub if the target path does not exist.
# Parent-dir creation is part of this rule — paths like docs/architecture/topology.md
# must have docs/architecture/ created before the stub write.
topology_rel="$default_topology_rel"
topology_path="$project_root/$topology_rel"
topology_parent="$(dirname "$topology_path")"

if [[ ! -e "$topology_path" ]]; then
  if [[ ! -d "$topology_parent" ]]; then
    mkdir -p "$topology_parent"
  fi
  cat > "$topology_path" <<'STUB'
# Topology

This document is the architecture-level analogue of /orb:code-investigate — a subsystem-keyed index that points to the canonical sources (authoritative code, owning decision record, operational doc, test surface) for each subsystem. Agents reach for it via /orb:topology before reasoning about how a multi-file capability works.

## Entries
STUB
  echo "orbit: wired docs.topology=$topology_rel; created stub at $topology_rel."
else
  echo "orbit: wired docs.topology=$topology_rel; existing file at $topology_rel left untouched."
fi

exit 0
