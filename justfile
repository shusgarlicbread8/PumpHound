set dotenv-load := true
set shell := ["zsh", "-cu"]
export PYTHONPATH := "ml:."

data_dir := env_var_or_default("PUMPHOUND_DATA_DIR", "./data")
api_bind := env_var_or_default("PUMPHOUND_API_BIND", "127.0.0.1:8080")
model_registry_path := env_var_or_default("PUMPHOUND_MODEL_REGISTRY", "artifacts/models/registry.json")

default:
    @just --list

# Start PostgreSQL, ClickHouse and NATS on loopback.
deps:
    mkdir -p {{data_dir}}/{journal,parquet,models,exports}
    docker compose up -d
    just _wait-stores

_wait-stores:
    #!/usr/bin/env zsh
    set -euo pipefail
    echo "waiting for postgres / clickhouse / nats"
    for i in {1..60}; do
      if docker compose exec -T postgres pg_isready -U pumphound -d pumphound >/dev/null 2>&1 \
         && curl -sf "http://127.0.0.1:8123/ping" >/dev/null \
         && curl -sf "http://127.0.0.1:8222/healthz" >/dev/null; then
        echo "stores ready"
        exit 0
      fi
      sleep 1
    done
    echo "stores failed to become ready" >&2
    exit 1

# Start stores, collector and core-api without the desktop.
capture-live:
    just deps
    cargo build -p pumphound-collector -p pumphound-core-api
    echo "mode=${PUMPHOUND_MODE:-LIVE_WATCH} source=$(just _source-label)"
    cargo run -p pumphound-collector --bin pumphound-collector &
    cargo run -p pumphound-core-api --bin pumphound-core-api

# Start the full local product: stores, collector, API, desktop.
dev:
    just deps
    cargo build -p pumphound-collector -p pumphound-core-api
    echo "mode=${PUMPHOUND_MODE:-LIVE_WATCH} source=$(just _source-label)"
    cargo run -p pumphound-collector --bin pumphound-collector &
    cargo run -p pumphound-core-api --bin pumphound-core-api &
    pnpm --filter @pumphound/desktop dev

# Graceful local shutdown. Data and checkpoints are preserved.
stop:
    pkill -f 'pumphound-collector|pumphound-core-api|pumphound-ml-worker|pumphound-investigator' || true
    docker compose stop

_source-label:
    #!/usr/bin/env zsh
    if [[ -n "${PUMPHOUND_HELIUS_API_KEY:-}" && "${PUMPHOUND_LASERSTREAM_ENTITLED:-0}" == "1" ]]; then
      echo "LIVE MAINNET / LASERSTREAM"
    elif [[ "${PUMPHOUND_MODE:-}" == "REPLAY" ]]; then
      echo "REPLAY"
    else
      echo "LIVE MAINNET / RPC FALLBACK"
    fi

inspect-launch mint:
    cargo run -p pumphound-core-api --bin inspect-launch -- {{mint}}

backfill-history config:
    cargo run -p pumphound-collector --bin backfill -- {{config}}

backfill-history-estimate config:
    cargo run -p pumphound-collector --bin backfill -- --estimate {{config}}

backfill-history-redecode config:
    cargo run -p pumphound-collector --bin backfill -- --redecode {{config}}

# Compatibility alias for the historical corpus backfill.
backfill config:
    just backfill-history {{config}}

graph-refresh manifest:
    uv run --directory . python -m pumphound_ml.graph.refresh {{manifest}}

dataset config:
    uv run --directory . python -m pumphound_ml.extraction.dataset {{config}}

dataset-replay:
    uv run --directory . python -m pumphound_ml.extraction.dataset ml/configs/dataset-replay.yaml

dataset-live:
    uv run --directory . python -m pumphound_ml.extraction.dataset ml/configs/dataset-live.yaml

dataset-existing:
    UV_OFFLINE=1 uv run --offline --directory . python -m pumphound_ml.extraction.dataset ml/configs/dataset-existing.yaml
    UV_OFFLINE=1 uv run --offline --directory . python -m pumphound_ml.extraction.readiness artifacts/datasets/existing-shadow-v1

adaptive-snapshot config="ml/configs/dataset-adaptive-existing.yaml":
    UV_OFFLINE=1 uv run --offline --directory . python -m pumphound_ml.labels.short_horizon {{config}}

adaptive-retrain dataset_config="ml/configs/dataset-adaptive-existing.yaml" train_config="ml/configs/train-adaptive-short-horizon.yaml":
    UV_OFFLINE=1 uv run --offline --directory . python -c 'import json; from pumphound_ml.lifecycle.retraining import run_retraining; print(json.dumps(run_retraining(dataset_config_path="{{dataset_config}}", train_config_path="{{train_config}}"), indent=2, default=str))'

dataset-diagnostics:
    UV_OFFLINE=1 uv run --offline --directory . python -m pumphound_ml.extraction.readiness artifacts/datasets/existing-shadow-v1

train config:
    uv run --directory . python -m pumphound_ml.training.tune {{config}}

train-shadow-existing:
    UV_OFFLINE=1 uv run --offline --directory . python -m pumphound_ml.extraction.readiness artifacts/datasets/existing-shadow-v1
    UV_OFFLINE=1 uv run --offline --directory . python -m pumphound_ml.training.tune ml/configs/train-shadow-existing.yaml

train-shadow:
    just train-shadow-existing

prepare-shadow-comparison:
    UV_OFFLINE=1 uv run --offline --directory . python -m pumphound_ml.evaluation.shadow_input

evaluate-strategies manifest:
    uv run --directory . python -m pumphound_ml.evaluation.strategies {{manifest}}

compile-paper-policy report:
    uv run --directory . python -m pumphound_ml.policy.compiler {{report}}

sync-model-registry:
    uv run --directory . python -m pumphound_ml.registry.store sync-postgres --registry {{model_registry_path}}

replay manifest:
    cargo run -p pumphound-core-api --bin replay -- {{manifest}}

# Validate the synthetic fixture and print the replay contract. Not live mainnet.
load-replay:
    bash scripts/load-replay.sh

# Run collector+API against the labelled replay fixture (GUI must show REPLAY).
capture-replay:
    #!/usr/bin/env zsh
    set -euo pipefail
    export PUMPHOUND_MODE=REPLAY
    export PUMPHOUND_REPLAY_PATH="${PUMPHOUND_REPLAY_PATH:-fixtures/chain/sample_logs.json}"
    export PUMPHOUND_REPLAY_CANONICAL="${PUMPHOUND_REPLAY_CANONICAL:-fixtures/chain/decoded_events.json}"
    export PUMPHOUND_REPLAY_SPEED="${PUMPHOUND_REPLAY_SPEED:-0}"
    just deps
    echo "mode=REPLAY source=REPLAY fixture=$PUMPHOUND_REPLAY_PATH"
    cargo run -p pumphound-collector --bin pumphound-collector &
    cargo run -p pumphound-core-api --bin pumphound-core-api

verify-ledger portfolio_id:
    cargo run -p pumphound-core-api --bin verify-ledger -- {{portfolio_id}}

verify:
    cargo test --workspace --exclude pumphound-desktop
    uv run pytest tests/python -q --ignore=tests/python/test_investigator_grounding.py
    pnpm --filter @pumphound/desktop exec tsc --noEmit

verify-live:
    cargo run -p pumphound-collector --bin verify-live

# Re-decode the labelled fixture (or a raw archive path). Must hash identically twice.
verify-archive *args:
    cargo run -q -p pumphound-collector --bin verify-archive -- {{args}}

release-check:
    bash scripts/release-check.sh

backup:
    bash scripts/backup.sh

restore-check backup_path:
    bash scripts/restore-check.sh {{backup_path}}
