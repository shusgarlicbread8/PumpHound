Completed milestone/task IDs:
  M1 tasks 1–3 in code; live collector has ingested real mainnet signatures (public RPC, then Helius RPC after key)
  M2 tasks 4–7 in code (Radar, token, evidence, system/settings/watchlists); GUI opened, Radar still sparse until CreateEvents land
  M3 tasks 8–11 partial (integer quotes, delayed fill, heuristic, paper GUI)
  M4–M7 Python/research + investigator code exists; not live-proven
  M8 ops packaging in deploy/scripts/runbooks (Docker reinstalled locally)
  M9 not claimed

Current working end-to-end path:
  Docker stores → collector + core-api on 127.0.0.1:8080 → desktop
  Health: LIVE_WATCH; Helius key present → Helius RPC/WS (label still RPC FALLBACK unless LASERSTREAM_ENTITLED=1)

Exact launch command:
  just capture-live
  # desktop: pnpm --filter @pumphound/desktop dev
  # or just dev

Live/replay source and captured date range:
  Live Solana mainnet, 2026-09-07, slots around 445115260+
  Helius RPC used after .env key set (restart required)

Last observed health/lag:
  core-api ready; collector connected; Radar row count still often 0 until launch.created decodes

Next concrete task:
  Confirm Radar shows a live launch without refresh; open one token tape/curve/reserves

Known blockers and degraded enrichments:
  PUMPHOUND_LASERSTREAM_ENTITLED=0 (correct unless the Helius plan includes LaserStream)
  Birdeye/Jupiter/Pyth/AI omitted
  MODEL NOT TRAINED
  Paper ledger not fully persisted from the API process
  Graph empty until just graph-refresh has events
  Postgres published on 5433 (host 5432 already used)

Changed contract/decoder/model versions:
  decoder_version=pump-idl-2026-09-07
  IDL commit 9c82f61cb711b044a17f770ab8ce9f9bdf78f333
