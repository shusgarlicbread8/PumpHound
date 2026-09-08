# PumpHound operations

## Topology

Local Mac: Docker Compose for PostgreSQL 17, ClickHouse 26.x and NATS JetStream. Collector and core-api run as native Rust processes. Desktop (when present) talks to `127.0.0.1:8080`.

Remote research VM: `deploy/compose.production.yaml`. Databases and NATS listen only on the internal overlay. The API is published on `127.0.0.1:8080`. Reach it over Tailscale, WireGuard or SSH. Do not expose Postgres, ClickHouse or NATS publicly. Do not put production secrets or the stateful database on a marketplace GPU host.

## Commands

| Action | Command |
|---|---|
| Preflight | `bash scripts/doctor.sh` |
| Start local | `just dev` or `bash scripts/start.sh` |
| Capture only | `just capture-live` |
| Stop, keep data | `just stop` or `bash scripts/stop.sh` |
| Replay fixture | `just load-replay` or `bash scripts/load-replay.sh` |
| Replay collector+API | `just capture-replay` |
| Backup | `just backup` / `bash scripts/backup.sh` |
| Restore check | `just restore-check <path>` |
| Partition export | `bash scripts/export-partitions.sh` |
| Production lint | `bash scripts/deploy-check.sh` |
| Release gate | `just release-check` |

## Environment

Copy `.env.example` to `.env`. `*_REF` values are secret-store references. Browser/webview code never receives provider keys. Doctor and logs must not print DSN passwords or API keys.

Replay:

```text
PUMPHOUND_MODE=REPLAY
PUMPHOUND_REPLAY_PATH=fixtures/chain/sample_logs.json
PUMPHOUND_REPLAY_CANONICAL=fixtures/chain/decoded_events.json
PUMPHOUND_REPLAY_SPEED=0
```

Speed `0` means maximum throughput (no inter-event sleep).

## Observability

Required families (export when the process implements them): source events/s and lag; decode failures; gaps/rollbacks; store/disk; NATS consumer lag; paper quote age and reconciliation; AI latency. Correlation IDs: `event_id`, `launch_id`, `prediction_id`, `decision_id`, `order_id`, `replay_id`. Do not put raw public keys on high-cardinality metric labels.

Production Compose starts Prometheus (`127.0.0.1:9090`) and an OTLP collector. Redact `authorization`, `api_key` and RPC query strings at the collector.

Health vocabulary: `healthy`, `delayed`, `backfilling`, `degraded`, `disconnected`, `inconsistent`.

## Capacity shed order

1. Pause AI investigations.
2. Reduce external enrichment.
3. Defer historical funding crawls.
4. Reduce UI graph refresh.
5. Keep journaling and normalizing program events.
6. If persistence is threatened, freeze new paper entries and show a hard degraded banner.

Never silently sample away candidate events.

## Backup and restore

Daily `scripts/backup.sh` writes `data/exports/YYYY-MM-DD/manifest.json` with dump hashes and table counts. A backup is **not accepted** until `scripts/restore-check.sh` matches hashes and an isolated restore plus replay succeeds (`docs/runbooks/restore.md`).

## Degradation and no-wallet

Optional providers missing → degrade that enrichment and show quality, never invent numbers. There is no signer, keypair loader or bundle submit path.

## Runbooks

| Signal | Runbook |
|---|---|
| Provider disconnect / gap | `docs/runbooks/provider-loss.md` |
| Unknown IDL / discriminator | `docs/runbooks/decoder-drift.md` |
| NATS / queue lag | `docs/runbooks/queue-lag.md` |
| Restore | `docs/runbooks/restore.md` |
| ClickHouse / disk | `docs/runbooks/disk-pressure.md` |
| Paper ledger mismatch | `docs/runbooks/ledger-divergence.md` |
| Bad model | `docs/runbooks/model-rollback.md` |
| Key leak | `docs/runbooks/key-rotation.md` |
| Processed fork | `docs/runbooks/fork-rollback.md` |
| Corrupt replay manifest | `docs/runbooks/replay-manifest.md` |
| Restart | `docs/runbooks/graceful-shutdown.md` |
