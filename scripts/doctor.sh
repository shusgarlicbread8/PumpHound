#!/usr/bin/env bash
# Preflight: disk, RAM, ports, dependencies and clock. Does not dump secrets.

set -euo pipefail
. "$(cd "$(dirname "$0")" && pwd)/lib.sh"
load_dotenv_keys_only

errors=0
warnings=0

note_fail() { fail "$1"; errors=$((errors + 1)); }
note_warn() { warn "$1"; warnings=$((warnings + 1)); }

echo "PumpHound doctor  root=$ROOT"

echo "dependencies"
for cmd in docker curl; do
  if have "$cmd"; then ok "$cmd"; else note_fail "$cmd not on PATH"; fi
done
if docker compose version >/dev/null 2>&1; then
  ok "docker compose"
else
  note_fail "docker compose plugin missing"
fi
for cmd in rustc cargo just; do
  if have "$cmd"; then ok "$cmd $($cmd --version 2>/dev/null | head -n1 | redact)"; else note_warn "$cmd not on PATH (needed for native collector/API)"; fi
done
for cmd in python3 pnpm node uv; do
  if have "$cmd"; then ok "$cmd"; else note_warn "$cmd not on PATH (desktop/research later)"; fi
done

echo "resources"
if have df; then
  avail_kb="$(df -k "$ROOT" | awk 'NR==2 {print $4}')"
  avail_gb=$((avail_kb / 1024 / 1024))
  if (( avail_gb < 5 )); then
    note_fail "free disk ${avail_gb}G on repo volume (need ≥5G, recommend ≥20G)"
  elif (( avail_gb < 20 )); then
    note_warn "free disk ${avail_gb}G on repo volume (recommend ≥20G for ClickHouse + journals)"
  else
    ok "free disk ${avail_gb}G on repo volume"
  fi
fi
if [[ -d "$DATA_DIR" ]]; then
  data_kb="$(df -k "$DATA_DIR" | awk 'NR==2 {print $4}')"
  ok "data dir exists ($DATA_DIR, $((data_kb / 1024 / 1024))G free)"
else
  note_warn "data dir $DATA_DIR does not exist yet (start.sh / just deps will create it)"
fi

ram_gb=""
if [[ "$(uname -s)" == "Darwin" ]]; then
  ram_gb=$(( $(sysctl -n hw.memsize) / 1024 / 1024 / 1024 ))
elif [[ -r /proc/meminfo ]]; then
  ram_gb=$(( $(awk '/MemTotal/ {print $2}' /proc/meminfo) / 1024 / 1024 ))
fi
if [[ -n "$ram_gb" ]]; then
  if (( ram_gb < 8 )); then
    note_warn "RAM ${ram_gb}G (8G minimum comfortable, 16G preferred)"
  else
    ok "RAM ${ram_gb}G"
  fi
else
  note_warn "could not read RAM"
fi

echo "ports (loopback listeners)"
check_port() {
  local port="$1" role="$2"
  if have lsof; then
    if lsof -nP -iTCP:"$port" -sTCP:LISTEN >/dev/null 2>&1; then
      ok "port $port in use ($role)"
    else
      printf '  [info] port %s free (%s)\n' "$port" "$role"
    fi
  elif have nc; then
    if nc -z 127.0.0.1 "$port" >/dev/null 2>&1; then
      ok "port $port reachable ($role)"
    else
      printf '  [info] port %s free (%s)\n' "$port" "$role"
    fi
  else
    note_warn "no lsof/nc to inspect port $port"
  fi
}
check_port 5433 postgres
check_port 8123 clickhouse-http
check_port 9000 clickhouse-native
check_port 4222 nats
check_port 8222 nats-monitor
check_port 8080 core-api

echo "clock"
local_utc="$(date -u +%s)"
remote=""
if remote_date="$(curl -fsSI --max-time 5 https://1.1.1.1 2>/dev/null | awk -F': ' 'tolower($1)=="date" {print $2}' | tr -d '\r')"; then
  if [[ -n "$remote_date" ]]; then
    if have gdate; then
      remote="$(gdate -u -d "$remote_date" +%s 2>/dev/null || true)"
    elif date -u -j -f "%a, %d %b %Y %H:%M:%S %Z" "$remote_date" +%s >/dev/null 2>&1; then
      remote="$(date -u -j -f "%a, %d %b %Y %H:%M:%S %Z" "$remote_date" +%s)"
    elif date -u -d "$remote_date" +%s >/dev/null 2>&1; then
      remote="$(date -u -d "$remote_date" +%s)"
    fi
  fi
fi
if [[ -n "$remote" ]]; then
  skew=$(( local_utc - remote ))
  abs=${skew#-}
  if (( abs > 5 )); then
    note_warn "clock skew ${skew}s versus network Date header"
  else
    ok "clock skew ${skew}s versus network Date header"
  fi
else
  note_warn "could not compare clock to a public Date header (offline is fine)"
fi
ok "local UTC $(date -u +%Y-%m-%dT%H:%M:%SZ)"

echo "configuration (presence only)"
if [[ -f "$ROOT/.env" ]]; then
  ok ".env present"
else
  note_warn ".env absent — copy .env.example (do not commit it)"
fi
printf '  [info] PUMPHOUND_MODE=%s\n' "${PUMPHOUND_MODE:-LIVE_WATCH (default)}"
printf '  [info] PUMPHOUND_API_BIND=%s\n' "${PUMPHOUND_API_BIND:-127.0.0.1:8080 (default)}"
printf '  [info] Helius key %s  LaserStream entitled=%s\n' \
  "$(env_present PUMPHOUND_HELIUS_API_KEY)" \
  "${PUMPHOUND_LASERSTREAM_ENTITLED:-0}"
printf '  [info] Birdeye %s  Jupiter %s  Pyth %s  AI key %s\n' \
  "$(env_present PUMPHOUND_BIRDEYE_KEY)" \
  "$(env_present PUMPHOUND_JUPITER_KEY)" \
  "$(env_present PUMPHOUND_PYTH_KEY)" \
  "$(env_present PUMPHOUND_AI_KEY)"

echo "boundary"
if git -C "$ROOT" grep -nE 'sign_transaction|submit_bundle|private_key|secret_key|keypair' -- \
  ':!PumpHound-Blueprint' ':!docs' ':!tests' ':!scripts' ':!fixtures' >/tmp/pumphound-doctor-signer 2>/dev/null; then
  if grep -qE 'sign_transaction|submit_bundle' /tmp/pumphound-doctor-signer; then
    note_fail "possible live signer/submit path — see doctor output file (paths only)"
    redact </tmp/pumphound-doctor-signer | head -n 20
  else
    ok "no sign/submit symbols in application paths"
  fi
else
  ok "no sign/submit symbols in application paths"
fi
rm -f /tmp/pumphound-doctor-signer

echo
if (( errors > 0 )); then
  echo "doctor FAILED  errors=$errors warnings=$warnings"
  exit 1
fi
echo "doctor PASSED  warnings=$warnings"
echo "mode is paper/watch/replay only. There is no live execution."
