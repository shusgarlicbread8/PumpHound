#!/usr/bin/env bash
# Shared helpers for operator scripts. Never print secret values.

set -euo pipefail

repo_root() {
  local here
  here="$(cd "$(dirname "${BASH_SOURCE[1]}")" && pwd)"
  cd "$here/.." && pwd
}

ROOT="${ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
DATA_DIR="${PUMPHOUND_DATA_DIR:-$ROOT/data}"
COMPOSE_LOCAL=(docker compose -f "$ROOT/compose.yaml")
COMPOSE_PROD=(docker compose -f "$ROOT/deploy/compose.production.yaml")

redact() {
  sed -E \
    -e 's/(api[_-]?key|password|secret|token|authorization|dsn)[=:][^[:space:]]+/\1=<redacted>/Ig' \
    -e 's#(postgres://[^:]+:)[^@]+#\1<redacted>#' \
    -e 's#([?&]api-key=)[^&[:space:]]+#\1<redacted>#Ig'
}

have() { command -v "$1" >/dev/null 2>&1; }

need() {
  if ! have "$1"; then
    echo "missing dependency: $1" >&2
    return 1
  fi
}

ok() { printf '  [ok]  %s\n' "$*"; }
warn() { printf '  [warn] %s\n' "$*"; }
fail() { printf '  [fail] %s\n' "$*"; }

env_present() {
  local key="$1"
  if [[ -n "${!key:-}" ]]; then
    echo "set"
  else
    echo "absent"
  fi
}

load_dotenv_keys_only() {
  # Load names into the environment for presence checks. Values are not printed.
  local file="$ROOT/.env"
  [[ -f "$file" ]] || return 0
  set -a
  # shellcheck disable=SC1090
  source "$file"
  set +a
}

compose_running() {
  docker compose -f "$1" ps --status running -q 2>/dev/null | grep -q .
}

pick_compose() {
  if compose_running "$ROOT/deploy/compose.production.yaml"; then
    echo prod
  elif compose_running "$ROOT/compose.yaml"; then
    echo local
  else
    echo none
  fi
}
