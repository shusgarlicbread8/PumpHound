# Runbook: provider loss and gap recovery

## Signal

- Health `chain` is `disconnected`, `delayed` or `backfilling`.
- `lag_ms` above 2000 or rising.
- Collector logs `chain source stopped` / websocket reconnect.
- Slot gaps or missing signatures versus a second RPC.
- UI (when present) shows degraded source, not a green lie.

## Safety

1. Do not invent trades to fill a gap.
2. Freeze **new** paper entries if lag exceeds policy (`maximum_chain_lag_ms`, default 2000) or quality is C/UNAVAILABLE.
3. Keep journaling whatever still arrives. Do not delete checkpoints.
4. Do not flip `PUMPHOUND_LASERSTREAM_ENTITLED=1` without a real entitlement.

## Recovery

1. Confirm which source failed: LaserStream vs public RPC vs replay path. `GET /api/v1/system/status` and collector stdout (`LIVE MAINNET / …` or `REPLAY`).
2. Check local network and provider status pages. Rotate to the configured fallback RPC/WebSocket.
3. On reconnect, resume from `source_checkpoints` (source `solana-ws` or laserstream, subscription `pump-programs`).
4. Backfill missing slot ranges with `just backfill <manifest>` once history API access exists. Same decoder and idempotent IDs.
5. Reconcile processed events to canonical block membership when `tx_index` becomes known. Retract orphans via `docs/runbooks/fork-rollback.md`.
6. If the provider is gone for the session, set mode `REPLAY` only for fixture/historical work — never relabel live as replay.

## Done when

- Source health returns to `healthy` or an honest `backfilling` with a shrinking gap.
- No duplicate `event_id` economic trades for the recovered signatures.
- Checkpoint slot advanced; paper entries remain frozen until quality ≥ policy minimum.
- Incident note: provider, outage window, slots backfilled, lag after recovery.
