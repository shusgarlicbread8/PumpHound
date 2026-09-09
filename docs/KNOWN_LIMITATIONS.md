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

## Execution Twin (2026-09-09)

- Legacy paper books (ALPHA-LEARN, RANDOM_CONTROL_V1, LIVE_PAPER_*) remain on `legacy-paper-v1`:
  1 s fixed delay, decision-time fill, flat fee, no failures. Their P&L is not comparable to
  `EXECUTION-EDGE-V1` books and is labelled as such in strategy reports.
- Landing model is a hand-set prior (`landing-v1-prior`); shadow `landing_observations` are being
  written but have not yet been used to fit it.
- CU usage is a static per-instruction estimate (`compute-budget-v1-static`), not profiled.
- Surfpool integration test exists (`just execution-twin-surfpool`) but only the BUY path is
  covered and it requires a running fork; it has not been run in CI.
- No Mollusk/LiteSVM fast-path test yet; deterministic protocol parity is via transcribed math +
  96 archived mainnet trades, not instruction execution.
- Post-cost outcome labels, Loss Filter, Tail Ranker and Exit Hazard model are not built; the
  ML system still trains on the legacy targets. Do not read any model score as post-cost EV.
- Confidence-derived sizing is disabled (`PUMPHOUND_CONFIDENCE_SIZING` unset) per GATE 9. The
  inversion has been quarantined, not explained.
- Sample sizes for `EXECUTION-EDGE-V1` books are in the engineering-sanity range (<30 closed
  trades per book). Nothing about their P&L is evidence.
- See `docs/EXECUTION_TWIN.md` for the full modeled-cost list and simulation gaps.

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
