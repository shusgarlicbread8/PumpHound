# Runbook: graceful shutdown and restart

## Signal

- Operator stop, host reboot, deploy, or disk-pressure halt.
- Processes not responding to `/api/v1/health`.

## Safety

1. Stop paper admissions before killing the API if a fill is in flight; wait for the current transaction lock or mark the order `QUEUED` for resume.
2. Do not `docker compose down -v` unless you intend to destroy volumes.
3. Checkpoints and journals are the resume source of truth.

## Recovery

1. `bash scripts/stop.sh` or `just stop` — SIGTERM collector/API, then `docker compose stop`.
2. Confirm pidfiles under `data/run/` are gone.
3. `bash scripts/start.sh` or `just deps` + collector/API.
4. Collector loads `source_checkpoints` and resumes. Duplicate deliveries must idempotently no-op.
5. Desktop (when present) reconnects; on epoch change it rehydrates via REST.

## Done when

- Health ready; last slot ≥ shutdown slot (or honest backfill).
- No duplicate economic trades across the restart window.
- Paper portfolio cash matches the ledger sum.
