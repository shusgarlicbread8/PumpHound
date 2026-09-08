#!/usr/bin/env bash
# Graceful local shutdown. Data, journals and checkpoints are kept.

set -euo pipefail
. "$(cd "$(dirname "$0")" && pwd)/lib.sh"

stop_pidfile() {
  local file="$1" name="$2"
  if [[ -f "$file" ]]; then
    local pid
    pid="$(cat "$file")"
    if kill -0 "$pid" 2>/dev/null; then
      kill -TERM "$pid" 2>/dev/null || true
      for _ in $(seq 1 20); do
        kill -0 "$pid" 2>/dev/null || break
        sleep 0.2
      done
      if kill -0 "$pid" 2>/dev/null; then
        echo "pid $pid ($name) still running after SIGTERM"
      else
        echo "stopped $name"
      fi
    fi
    rm -f "$file"
  fi
}

stop_pidfile "$DATA_DIR/run/collector.pid" collector
stop_pidfile "$DATA_DIR/run/core-api.pid" core-api
stop_pidfile "$DATA_DIR/run/desktop.pid" desktop

pkill -f 'pumphound-collector|pumphound-core-api|pumphound-ml-worker|pumphound-investigator' 2>/dev/null || true

if compose_running "$ROOT/compose.yaml"; then
  "${COMPOSE_LOCAL[@]}" stop
  echo "local compose stores stopped"
fi
if compose_running "$ROOT/deploy/compose.production.yaml"; then
  echo "production compose is running; not stopped (use: docker compose -f deploy/compose.production.yaml stop)"
fi

echo "shutdown complete; data in $DATA_DIR preserved"
