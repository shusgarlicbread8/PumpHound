# Known limitations

Honest list as of 2026-09-07. This is not a complete product.

## Not built

- Desktop source exists (`apps/desktop`) but this operations pass did not record a live GUI session. e2e files are checklists, not a Playwright run.
- Python `ml-worker` and investigator packages. Compose images expose health stubs that abstain.
- LaserStream gRPC path is compiled in but unused until a Helius key **and** `PUMPHOUND_LASERSTREAM_ENTITLED=1` are set.
- Trained models, promotion registry UI, strategy lab, funding crawls, holder snapshots, candle history reader.
- Comprehensive milestone-9 fixture corpus against real captured transactions.
- Isolated restore into a throwaway Compose project (hash check exists; full DB clone is still a runbook step).

## Partial

- Collector decodes Pump / PumpSwap and journals raw batches. Synthetic fixture instruction bytes are placeholders; unknown discriminators must quarantine.
- Core API implements the `/api/v1` catalog. Several read models return empty items with an explicit quality/reason (trades tape, graph, holders, predictions).
- Paper quotes/orders run in-process for the three `LIVE_PAPER_*` modes and `REPLAY`.
  The durable ledger, reservations, arrival repricing, fills, positions, and FIFO lots are
  PostgreSQL-backed. There is no signer or live transaction submission path.
- Replay binary prints the manifest and states that it shares the live engine; `load-replay.sh` validates the JSON fixture. End-to-end same-seed content hashes are not yet produced.
- Websocket `/api/v1/stream` exists with subscribe/heartbeat. Resume-buffer expiry and `resync_required` are incomplete.
- Metadata SSRF protections specified in the operations spec are not implemented (no fetcher yet).

## Product limits that remain even when complete

- **No live execution.** Paper and replay only. Counterfactual impact cannot know how other traders would have reacted (`bounded_shadow` vs `impact_overlay`).
- Public RPC can delay, duplicate, omit or fork. Websocket fallback often lacks `tx_index`; order is then provisional.
- Optional APIs (Birdeye, Jupiter, Pyth, DexScreener, SiliconFlow) are omitted rather than invented.
- Inferred wallet/creator links are **not ownership** and not a fraud verdict.
- Token metadata is untrusted.
- Strategy results may be negative or inconclusive. Software completeness is not an edge.
- Holder/supply claims are covered-subset estimates unless a slot-context snapshot exists.

## Label this evidence correctly

Synthetic fixtures in `fixtures/` are labelled `SYNTHETIC — not live mainnet`. Screenshots of fixtures must say so. Live evidence must name source and date range.
