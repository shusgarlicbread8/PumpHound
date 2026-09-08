# Desktop e2e plan

Placeholder case lists live in `apps/desktop/e2e/*.spec.ts`. Playwright is not a desktop dependency yet; treat those files as checklists until `@playwright/test` is installed.

```text
apps/desktop/e2e/
  live-terminal.spec.ts
  paper.spec.ts
  graph.spec.ts
  replay.spec.ts
  accessibility.spec.ts
```

Run later: `pnpm --filter @pumphound/desktop exec playwright test` plus a packaged Tauri smoke for keychain/notifications.

## Flows (must be real screens, not mockups)

1. Radar updates without refresh; filter stays stable; new launch row appears (live) or fixture row appears with `REPLAY` banner.
2. Token: chart, tape, curve, holders, provenance drawer.
3. Hostile metadata URI → blocked + fallback (see `tests/integration/metadata-security.md`).
4. Watchlist / alerts persist across restart.
5. Paper buy then sell: order → arrival → fill → position → exit. Mode
   `LIVE_PAPER_HEURISTIC`, `LIVE_PAPER_SHADOW_ML`, `LIVE_PAPER_ML`, or `REPLAY` only.
6. Graph: expand/filter/collapse/scrub; node budget respected.
7. Model card shows `MODEL NOT TRAINED` until a real artifact exists.
8. Replay controls: pause, step, speed; virtual clock; same-seed hash.
9. Disconnect/reconnect websocket: health transition, no corrupt tape (`tests/integration/stream-resume.md`).
10. Keyboard: focus order, colour-independent risk, 1280×800 and 1920×1080, reduced motion.

## Load

`scripts/load-replay.sh` at 1× / 5× / 10× once a captured partition exists. Record p50/p95/p99, queue/disk, CPU/RAM, dropped-record count. Fixtures in `fixtures/` are synthetic and must be labelled in any screenshot.

## Out of scope

Live signing, seed input, bundle submit. Those cases must be rejected in UI and API (`422 UNSUPPORTED_EXECUTION_MODE`).
