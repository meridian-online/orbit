#!/usr/bin/env bash
# setup-method.sh — implements /orb:setup §6 (METHOD.md copy + CLAUDE.md @-import).
#
# Pipeline (per spec 2026-05-09-orbit-method-md, ac-03):
#   1. Legacy-CLAUDE.md detection — scan for ## Workflow (orbit) / ## Orbit vocabulary
#      / ## Current Sprint markers. If present, prompt to migrate atomically with the
#      @-import addition. Decline → REFUSE the entire operation (no METHOD.md copy,
#      no @-import). Atomic semantics — never leave dual-source drift.
#   2. Copy plugins/orb/skills/setup/METHOD.md to .orbit/METHOD.md. If destination
#      exists, byte-for-byte compare (entire file including 'How to update' line);
#      mismatch prompts before overwriting.
#   3. Ensure CLAUDE.md contains an `@.orbit/METHOD.md` line. Idempotent: append at
#      end-of-file with leading blank line if missing; no marker heading.
#
# Usage:
#   setup-method.sh --project-root <path> [--canonical <path>] [--answer-legacy y|n] [--answer-drift y|n]
#
# Test affordances:
#   --answer-legacy   scripts the legacy-migration prompt
#   --answer-drift    scripts the METHOD.md drift prompt
# Both default to interactive (read from stdin).

set -euo pipefail

usage() {
  cat >&2 <<'EOF'
Usage: setup-method.sh --project-root <path> [--canonical <path>] [--answer-legacy y|n] [--answer-drift y|n]

Required:
  --project-root <path>   Project root containing CLAUDE.md and .orbit/

Optional:
  --canonical <path>      Path to canonical METHOD.md (defaults to the in-plugin file
                          plugins/orb/skills/setup/METHOD.md, resolved relative to
                          this script).
  --answer-legacy y|n     Script the legacy-migration prompt (default: interactive).
  --answer-drift  y|n     Script the METHOD.md drift prompt (default: interactive).
EOF
  exit 2
}

project_root=""
canonical=""
answer_legacy=""
answer_drift=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --project-root) project_root="$2"; shift 2 ;;
    --canonical) canonical="$2"; shift 2 ;;
    --answer-legacy) answer_legacy="$2"; shift 2 ;;
    --answer-drift) answer_drift="$2"; shift 2 ;;
    *) echo "setup-method.sh: unknown option: $1" >&2; usage ;;
  esac
done

if [[ -z "$project_root" ]]; then
  usage
fi

if [[ ! -d "$project_root" ]]; then
  echo "setup-method.sh: project root not found: $project_root" >&2
  exit 2
fi

# Default canonical to the script-relative location.
if [[ -z "$canonical" ]]; then
  script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  canonical="$script_dir/../skills/setup/METHOD.md"
fi

if [[ ! -f "$canonical" ]]; then
  echo "setup-method.sh: canonical METHOD.md not found: $canonical" >&2
  exit 2
fi

claude_md="$project_root/CLAUDE.md"
method_md="$project_root/.orbit/METHOD.md"

LEGACY_MARKERS=(
  '## Workflow (orbit)'
  '## Orbit vocabulary'
  '## Current Sprint'
)

# 6a — legacy CLAUDE.md detection.
legacy_present=0
if [[ -f "$claude_md" ]]; then
  for marker in "${LEGACY_MARKERS[@]}"; do
    if grep -Fxq "$marker" "$claude_md"; then
      legacy_present=1
      break
    fi
  done
fi

if [[ "$legacy_present" -eq 1 ]]; then
  echo "orbit: CLAUDE.md contains legacy workflow blocks (## Workflow (orbit) / ## Orbit vocabulary / ## Current Sprint)."
  echo "orbit: migration removes them and adds @.orbit/METHOD.md as the single source of truth."

  if [[ -n "$answer_legacy" ]]; then
    answer="$answer_legacy"
  else
    read -r -p "Migrate now? (y/N) " answer || answer=""
  fi

  case "${answer,,}" in
    y|yes)
      # Atomic migrate: remove legacy blocks AND copy METHOD.md AND add @-import in one go.
      # Use python3 for the multi-block removal to keep the bash light.
      mkdir -p "$project_root/.orbit"
      cp "$canonical" "$method_md"

      python3 - "$claude_md" <<'PY'
import re, sys
path = sys.argv[1]
text = open(path).read() if open is not None else ''
with open(path) as f:
    text = f.read()

markers = ['## Workflow (orbit)', '## Orbit vocabulary', '## Current Sprint']

def strip_section(body: str, marker: str) -> str:
    # Remove from `marker` (at line start) up to the next top-level heading or EOF.
    pattern = re.compile(
        r'(^|\n)' + re.escape(marker) + r'\s*\n.*?(?=\n##\s|\n#\s|\Z)',
        flags=re.DOTALL,
    )
    return pattern.sub('', body)

for m in markers:
    text = strip_section(text, m)

# Collapse 3+ consecutive blank lines back to 2.
text = re.sub(r'\n{3,}', '\n\n', text)

# Ensure exactly one @.orbit/METHOD.md line at end-of-file with a blank line above.
if '@.orbit/METHOD.md' not in text:
    if not text.endswith('\n'):
        text += '\n'
    if not text.endswith('\n\n'):
        text += '\n'
    text += '@.orbit/METHOD.md\n'

with open(path, 'w') as f:
    f.write(text)
PY
      echo "orbit: legacy blocks removed; .orbit/METHOD.md created; @.orbit/METHOD.md added to CLAUDE.md."
      exit 0
      ;;
    *)
      echo "orbit: setup aborted. Re-run /orb:setup once you have removed the legacy blocks, or accept the migration prompt." >&2
      exit 1
      ;;
  esac
fi

# 6b — copy METHOD.md (no legacy blocks present).
mkdir -p "$project_root/.orbit"

if [[ -f "$method_md" ]]; then
  if cmp -s "$canonical" "$method_md"; then
    : # byte-identical, no-op
  else
    echo "orbit: .orbit/METHOD.md differs from the canonical (the plugin has updated, or the file has been edited locally)."

    if [[ -n "$answer_drift" ]]; then
      answer="$answer_drift"
    else
      read -r -p "Overwrite with canonical? (y/N) " answer || answer=""
    fi

    case "${answer,,}" in
      y|yes)
        cp "$canonical" "$method_md"
        echo "orbit: .orbit/METHOD.md overwritten with canonical."
        ;;
      *)
        echo "orbit: keeping local .orbit/METHOD.md (canonical not applied)."
        ;;
    esac
  fi
else
  cp "$canonical" "$method_md"
fi

# 6c — ensure CLAUDE.md @-import (idempotent).
if [[ ! -f "$claude_md" ]]; then
  printf '\n@.orbit/METHOD.md\n' > "$claude_md"
elif ! grep -Fxq '@.orbit/METHOD.md' "$claude_md"; then
  # Append on its own line at end-of-file with a blank line above.
  if [[ -s "$claude_md" ]]; then
    # Ensure file ends with a newline.
    [[ "$(tail -c1 "$claude_md")" == $'\n' ]] || printf '\n' >> "$claude_md"
    printf '\n@.orbit/METHOD.md\n' >> "$claude_md"
  else
    printf '@.orbit/METHOD.md\n' >> "$claude_md"
  fi
fi

exit 0
