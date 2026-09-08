#!/usr/bin/env bash
# One-command local start: stores, collector, core-api. Desktop is optional.

set -euo pipefail
. "$(cd "$(dirname "$0")" && pwd)/lib.sh"
load_dotenv_keys_only

mkdir -p "$DATA_DIR"/{journal,parquet,models,exports,run}

if [[ "${1:-}" != "--skip-doctor" ]]; then
  bash "$ROOT/scripts/doctor.sh" || {
    echo "doctor reported failures; fix them or re-run with --skip-doctor" >&2
    exit 1
  }
fi

echo "starting stores"
"${COMPOSE_LOCAL[@]}" up -d
echo "waiting for postgres / clickhouse / nats"
for _ in $(seq 1 60); do
  if docker compose -f "$ROOT/compose.yaml" exec -T postgres pg_isready -U pumphound -d pumphound >/dev/null 2>&1 \
     && curl -sf "http://127.0.0.1:8123/ping" >/dev/null \
     && curl -sf "http://127.0.0.1:8222/healthz" >/dev/null; then
    echo "stores ready"
    break
  fi
  sleep 1
done

mode="${PUMPHOUND_MODE:-LIVE_WATCH}"
if [[ "$mode" == "REPLAY" ]]; then
  source_label="REPLAY"
elif [[ -n "${PUMPHOUND_HELIUS_API_KEY:-}" && "${PUMPHOUND_LASERSTREAM_ENTITLED:-0}" == "1" ]]; then
  source_label="LIVE MAINNET / LASERSTREAM"
else
  source_label="LIVE MAINNET / RPC FALLBACK"
fi
echo "mode=$mode source=$source_label"

if ! have cargo; then
  echo "cargo not available; stores are up. Build collector/core-api on a machine with Rust." >&2
  exit 0
fi

cd "$ROOT"
cargo build -p pumphound-collector -p pumphound-core-api

if [[ -f "$DATA_DIR/run/collector.pid" ]] && kill -0 "$(cat "$DATA_DIR/run/collector.pid")" 2>/dev/null; then
  echo "collector already running pid=$(cat "$DATA_DIR/run/collector.pid")"
else
  nohup cargo run -q -p pumphound-collector --bin pumphound-collector >"$DATA_DIR/run/collector.log" 2>&1 &
  echo $! >"$DATA_DIR/run/collector.pid"
  echo "collector pid=$(cat "$DATA_DIR/run/collector.pid")"
fi

if [[ -f "$DATA_DIR/run/core-api.pid" ]] && kill -0 "$(cat "$DATA_DIR/run/core-api.pid")" 2>/dev/null; then
  echo "core-api already running pid=$(cat "$DATA_DIR/run/core-api.pid")"
else
  nohup cargo run -q -p pumphound-core-api --bin pumphound-core-api >"$DATA_DIR/run/core-api.log" 2>&1 &
  echo $! >"$DATA_DIR/run/core-api.pid"
  echo "core-api pid=$(cat "$DATA_DIR/run/core-api.pid")"
fi

if [[ -d "$ROOT/apps/desktop" && "${PUMPHOUND_START_DESKTOP:-1}" != "0" ]]; then
  if have pnpm; then
    nohup pnpm --filter @pumphound/desktop dev >"$DATA_DIR/run/desktop.log" 2>&1 &
    echo $! >"$DATA_DIR/run/desktop.pid"
    echo "desktop pid=$(cat "$DATA_DIR/run/desktop.pid")"
  else
    echo "desktop present but pnpm missing"
  fi
else
  echo "desktop not started (missing tree or PUMPHOUND_START_DESKTOP=0)"
fi

echo "API bind ${PUMPHOUND_API_BIND:-127.0.0.1:8080}"
echo "paper only — no live signer"
