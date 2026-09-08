#!/usr/bin/env bash
# Package the macOS desktop app when apps/desktop exists.

set -euo pipefail
. "$(cd "$(dirname "$0")" && pwd)/lib.sh"

if [[ ! -d "$ROOT/apps/desktop" ]]; then
  echo "apps/desktop is not in this tree. Desktop packaging is skipped."
  echo "See tests/e2e/PLAN.md for the verification plan once the Tauri app exists."
  exit 2
fi

need pnpm
mkdir -p "$ROOT/release"
pnpm --filter @pumphound/desktop build
if [[ -f "$ROOT/apps/desktop/src-tauri/tauri.conf.json" ]]; then
  pnpm --filter @pumphound/desktop tauri build
else
  echo "src-tauri/tauri.conf.json missing; frontend build only" >&2
  exit 1
fi
echo "package-desktop finished — copy the installer into release/ without secrets"
