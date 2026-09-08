# Integration note: provider outage

**Status:** harness note. Not an automated live test. Desktop is absent.

## Intent

When LaserStream or the Solana websocket dies, capture degrades visibly and does not invent trades. Paper entries freeze if lag/quality gates fire. Restart resumes from `source_checkpoints` without duplicating economic `event_id`s.

## Setup

1. `just deps` and collector in `LIVE_WATCH` against the configured RPC (or `REPLAY` with `fixtures/chain/sample_logs.json` plus a process kill mid-file to simulate drop).
2. Watch `GET /api/v1/system/status` and collector stdout. Do not log API keys.

## Cases

| Case | Action | Expected |
|---|---|---|
| WS disconnect | Block egress to the RPC host or kill the source process | `chain=disconnected` or `delayed`; no fabricated Radar rows |
| Fallback | With LaserStream entitled, disable gRPC only | Label becomes RPC fallback; same decoder |
| Gap | Drop 10s of slots then restore | Backfill or visible gap; checkpoint not rewound to genesis |
| Duplicate replay | Re-deliver the same synthetic signatures | One economic trade per `event_id` |
| Optional APIs | Leave Birdeye/Jupiter unset | Capabilities false; no numeric placeholders |

## Evidence to keep

Status JSON (redacted), collector line counts (`raw`, `decoded`, `quarantined`), checkpoint slot before/after, statement that no live signer was used.

## Automate later

`tests/integration/provider-outage.ts` against a stub RPC once the desktop/e2e host exists. Follow `docs/runbooks/provider-loss.md`.
