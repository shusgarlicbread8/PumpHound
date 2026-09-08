#!/usr/bin/env bash
# Load the synthetic chain fixture through the replay path.
# This is not live mainnet.

set -euo pipefail
. "$(cd "$(dirname "$0")" && pwd)/lib.sh"

export PUMPHOUND_MODE=REPLAY
export PUMPHOUND_REPLAY_PATH="${PUMPHOUND_REPLAY_PATH:-$ROOT/fixtures/chain/sample_logs.json}"
export PUMPHOUND_REPLAY_CANONICAL="${PUMPHOUND_REPLAY_CANONICAL:-$ROOT/fixtures/chain/decoded_events.json}"
export PUMPHOUND_REPLAY_SPEED="${PUMPHOUND_REPLAY_SPEED:-0}"
manifest="${1:-$ROOT/fixtures/replay/manifest.json}"

echo "REPLAY"
echo "manifest=$manifest"
echo "logs=$PUMPHOUND_REPLAY_PATH"
echo "canonical=$PUMPHOUND_REPLAY_CANONICAL"
echo "virtual_clock=enabled"
echo "label=SYNTHETIC — not live mainnet"

if [[ ! -f "$PUMPHOUND_REPLAY_PATH" ]]; then
  echo "missing fixture $PUMPHOUND_REPLAY_PATH" >&2
  exit 1
fi
if [[ ! -f "$PUMPHOUND_REPLAY_CANONICAL" ]]; then
  echo "missing canonical sidecar $PUMPHOUND_REPLAY_CANONICAL" >&2
  exit 1
fi
if [[ ! -f "$manifest" ]]; then
  echo "missing $manifest" >&2
  exit 1
fi

python3 - "$PUMPHOUND_REPLAY_PATH" <<'PY'
import json, sys
from pathlib import Path
p = Path(sys.argv[1])
rows = json.loads(p.read_text())
assert isinstance(rows, list) and rows, "sample_logs.json must be a non-empty array"
required = {"signature", "slot", "account_keys", "instructions", "log_messages", "raw_json"}
for i, row in enumerate(rows):
    missing = required - set(row)
    assert not missing, f"row {i} missing {missing}"
    assert row.get("raw_json", {}).get("synthetic") is True, f"row {i} is not labelled synthetic"
print(f"fixture_ok rows={len(rows)} first_slot={rows[0]['slot']}")
PY

if have cargo; then
  (cd "$ROOT" && cargo run -q -p pumphound-core-api --bin replay -- "$manifest")
else
  echo "cargo missing; fixture schema check only"
fi

echo "load-replay complete (same reducer/paper engine as live when wired)"
