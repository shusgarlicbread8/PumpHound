#!/usr/bin/env bash
# Validate production packaging. Does not print secret values.

set -euo pipefail
. "$(cd "$(dirname "$0")" && pwd)/lib.sh"

errors=0
note_fail() { fail "$1"; errors=$((errors + 1)); }
note_warn() { warn "$1"; }

echo "deploy-check"

need docker || note_fail "docker required"

compose="$ROOT/deploy/compose.production.yaml"
[[ -f "$compose" ]] || note_fail "missing $compose"

for f in \
  "$ROOT/deploy/collector.Dockerfile" \
  "$ROOT/deploy/core-api.Dockerfile" \
  "$ROOT/deploy/ml-worker.Dockerfile" \
  "$ROOT/deploy/investigator.Dockerfile" \
  "$ROOT/deploy/observability/prometheus.yaml" \
  "$ROOT/deploy/observability/otel-collector.yaml"; do
  [[ -f "$f" ]] && ok "$(basename "$f")" || note_fail "missing $f"
done

# Placeholder values only for `compose config` validation. Never printed.
if POSTGRES_PASSWORD=check CLICKHOUSE_PASSWORD=check \
  docker compose -f "$compose" config >/dev/null 2>&1; then
  ok "compose.production.yaml parses"
else
  note_fail "compose.production.yaml does not parse (check YAML, not secrets)"
fi

echo "private bind checks"
if grep -E '^\s+-\s+"0\.0\.0\.0:(5432|8123|9000|4222)' "$compose"; then
  note_fail "database/NATS published on 0.0.0.0"
else
  ok "no public 0.0.0.0 publishes for postgres/clickhouse/nats"
fi
if grep -q '127.0.0.1:8080:8080' "$compose"; then
  ok "core-api published on loopback only"
else
  note_fail "core-api is not bound to 127.0.0.1:8080"
fi
if grep -q '127.0.0.1:8080:8080' "$compose" && ! grep -E '^\s+-\s+"[0-9.]+:(5432|8123|9000|4222)' "$compose"; then
  ok "stores are not published; API is loopback-only"
else
  note_fail "store publish / API bind contract failed"
fi

echo "healthchecks and limits"
for svc in postgres clickhouse nats collector core-api ml-worker investigator; do
  if grep -A40 "^  ${svc}:" "$compose" | grep -q healthcheck; then
    ok "$svc healthcheck"
  else
    note_fail "$svc missing healthcheck"
  fi
  if grep -A80 "^  ${svc}:" "$compose" | grep -q 'memory:'; then
    ok "$svc memory limit"
  else
    note_fail "$svc missing memory limit"
  fi
done

echo "secrets in compose files"
if grep -Ei 'helius-|sk-|BEGIN (RSA |OPENSSH )?PRIVATE' "$compose" "$ROOT/compose.yaml"; then
  note_fail "possible secret material in compose files"
else
  ok "no obvious secret material in compose files"
fi

echo "image pin reminder"
if grep -E 'image: .+=sha256:' "$compose" >/dev/null; then
  ok "at least one image uses a digest pin"
else
  warn "images are tag-pinned only; record digests after first pull before remote promote"
fi

if (( errors > 0 )); then
  echo "deploy-check FAILED errors=$errors"
  exit 1
fi
echo "deploy-check PASSED"
