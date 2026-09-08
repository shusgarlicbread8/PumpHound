# Integration note: stream resume

**Status:** harness note. `/api/v1/stream` exists with subscribe + 5s heartbeats. Resume buffer / `resync_required` is incomplete.

## Intent

Clients subscribe with `{ type, request_id, topics, resume: { epoch, sequence } }`. Sequences are per client subscription stream. Epoch changes on server restart. If resume history expired, server emits `resync_required`; client REST-hydrates and applies only messages newer than the snapshot `stream_cursor`.

## Cases

| Case | Expected |
|---|---|
| Fresh subscribe `radar` + `system` | `snapshot` then `delta`/`event`/`heartbeat` |
| Pause 30s within buffer (once implemented) | Resume without full reload |
| Restart core-api (epoch change) | Client detects new epoch, GET `/launches`, atomic swap |
| Slow client | Radar coalesced ≤10 Hz; order/alert/retract messages not coalesced away |
| Gap in sequence | `resync_required` for affected topics, not a silent hole |

## Probe now

```bash
# After core-api is up (no secrets in the URL)
# python3 -c "import asyncio,websockets,json; ..."  connecting to ws://127.0.0.1:8080/api/v1/stream
```

Send the subscribe object from `contracts/03-api-and-stream-contracts.md`. Confirm heartbeat `schema_version=1`.

## Automate later

TypeScript client in the desktop app asserting epoch change and REST rehydrate. See `tests/e2e/PLAN.md`.
