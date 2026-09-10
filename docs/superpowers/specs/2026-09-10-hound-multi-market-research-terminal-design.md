# Hound Multi-Market Research Terminal — Design

Date: 2026-09-10
Status: Approved design draft for implementation planning

## Purpose
Evolve PumpHound into a multi-market research terminal while protecting the working Pump stack. Hyperliquid is the only new venue receiving deep end-to-end research implementation in this cycle; Ostium and Polymarket receive read-only durable monitoring/research stacks. No real-money execution.

## Core principle
Share engines, not market assumptions. Common vocabulary may include Venue, Instrument, MarketObservation, FeatureSnapshot, OutcomeLabel, ModelVersion, Experiment, StrategyBook, PaperOrder, PaperFill, PaperPosition, ExecutionCost, ExecutionTrace, ResearchMetric, and DataHealth. Pump-specific bonding curves, wallet/creator graph, Solana execution, Hyperliquid funding/orderbook/HIP metadata, Ostium rollover/funding/dynamic-spread/keeper mechanics, and Polymarket outcome/resolution semantics remain venue-specific.

Do not refactor stable Pump code merely to fit abstractions. Existing Pump experiments, Execution Twin, evidence repair, collectors, and frozen books remain protected.

## Scope
- Pump: keep live and regression-safe; do not disrupt active history work or Helius quota.
- Hyperliquid: live ingestion -> durable history -> point-in-time lifecycle features -> venue-specific Execution Twin -> economic labels -> deterministic baselines -> specialist ML/ranking -> paper experiments -> UI.
- Ostium: live/read-only markets, OI/skew/caps, funding vs rollover, negative-rollover configuration, liquidations where available, simulated slippage/orderbook/dynamic spread recording, data health. No strategy in this cycle.
- Polymarket: live/read-only markets/events/CLOB/orderbooks, resolution metadata, conservative relationship graph with PROVEN/LIKELY/UNVERIFIED provenance. No autonomous trading in this cycle.

## Hyperliquid research objective
Determine whether newly formed or immature Hyperliquid markets contain repeatable post-cost inefficiencies before they mature. Primary areas: HIP-3/new-market formation, funding persistence and basis, OI formation/liquidations, market maturation, and fair-value context for unusual/pre-IPO markets. Generic BTC direction is not the thesis.

### Data
Collect market identity/type, builder/deployer, first-observed/deployment time, reference/oracle metadata, best bid/ask, mid, mark, oracle/index, spread, depth at representative notionals, trades and aggressor direction when derivable, OI/OI change, funding and cumulative funding, liquidations, volume/acceleration, market status, leverage/risk parameters. Unknown remains UNKNOWN.

### Age snapshots
Persist first-observed/T0, +1s, +3s, +10s, +30s, +60s, +5m, +15m, +1h, +6h, +24h, +3d, +7d. Record actual observation delay when an exact age cannot be sampled.

### Model families
- Maturation: future OI/volume/depth growth, spread compression, survival/activity.
- Basis: residual convergence/widening, half-life, executable convergence return, and which side actually moved.
- Funding: future cumulative funding at 1h/8h/24h/3d, sign reversal, normalization time, persistence; decompose gross funding, entry/exit/hedge cost, basis P&L, net result.
- Liquidation/OI: liquidation-burst probability, OI destruction, funding impact, volatility expansion.
- Tail/directional: net MFE/MAE, +X before -Y, -X before +Y, post-cost return; only after execution-aware labels exist.
- Ranking: LightGBMRanker/XGBRanker where appropriate; evaluate top 1/5/10/20 percent using economic metrics.

### Hyperliquid Execution Twin
Decision snapshot -> order intent -> visible book -> latency -> arrival book -> fill/partial/failure -> fees/slippage -> position -> funding -> mark/oracle/margin evolution -> exit -> post-cost P&L. Model maker/taker fees, spread, depth/slippage, partial fills, latency, cancel/replace, funding, leverage/margin, liquidation, mark/oracle, and external hedge costs when required. Never fill resting paper orders merely because a candle touched the level; use conservative queue assumptions and expose fidelity.

### First experiment
`HIP-NEW-MARKET-V1` with comparable observations/books: RANDOM_NEW_MARKET, MICROSTRUCTURE_BASELINE, MATURATION_RANKER, STRUCTURAL_EDGE, FULL_QUANT, FULL_QUANT_AI_SHADOW. AI remains shadow/advisory. Comparisons with different timing, universe, sizing, or execution assumptions must be marked confounded.

## Ostium
Collect real live price/market state, market hours, risk config, long/short OI, skew, caps/utilization, funding config/current state, rollover long/short, broker premium, negative-rollover flag, liquidation/position information where available, simulated slippage/orderbook/dynamic-spread surfaces, provider health. Backfill chain history where practical and version by contract/protocol regime. Start recording difficult-to-reconstruct off-chain quote surfaces immediately. Do not assume funding alpha until live pair configuration proves funding is enabled/non-zero.

## Polymarket
Collect live/upcoming/resolved markets, parent events, YES/NO prices, CLOB bids/asks/spread/depth/trades, liquidity/volume, resolution times/rules/sources/deadlines, categories, related-market candidates and lifecycle. Represent relationships with provenance: PROVEN_RELATION, LIKELY_RELATION, UNVERIFIED_RELATION. Similar titles do not prove equivalence. LLMs may propose relationships but cannot promote them to proven without deterministic/reviewed evidence.

## Shared storage and provider rules
Preserve raw source material plus normalized long-lived observations. Use measured retention: short raw high-frequency retention, normalized long-term observations, periodic depth snapshots, and event-triggered snapshots unless measured storage supports more. Expose current bytes, last-24h growth, 30d and 90d estimates per venue.

Each provider gets independent rate limits, bounded retry/backoff, queue depth, error counters and stale-data health. A 429 degrades one provider/venue, not the entire application. Use public/read-only APIs first. Missing optional keys should produce DISABLED_MISSING_KEY and not block other work. Never commit or print secrets.

## Research methodology
Mechanism -> Observation -> Execution -> Label -> Baseline -> Model -> Prospective paper -> Promotion. Chronological/walk-forward validation only for time series. Statistical model quality is not trading-edge proof; post-cost economic evaluation is mandatory.

## Non-goals
No real-money execution, private keys, wallet signing, live autonomous orders, generic foundation model, RL trading agent, giant cross-market neural model, GNN for novelty, generic BTC predictor as primary goal, fake maker fills, random time-series train/test split, missing-to-zero coercion, or alpha claims from in-sample results.

## Required UI
Home/global health plus Pump, Hyperliquid, Ostium, Prediction, System workspaces. Hyperliquid includes Overview, New Markets, HIP-3, Funding, Basis, Liquidations, Research, Paper. Ostium includes Markets, Positioning, Structural Yield, Liquidations, Execution Surface, Data Health. Prediction includes Markets, Events, Orderbooks, Relationships, Resolution, Data Health. Keep a dense professional research-terminal aesthetic.

## Definition of done
Pump remains healthy and regression-safe. Hyperliquid has real live durable data, new/HIP-3 identification, orderbook/depth/funding/OI/liquidation state, lifecycle snapshots, venue-specific paper twin, deterministic baseline research, economic labels where future truth exists, and initial specialist models only where label truth supports them. Ostium has real live/config data and quote-surface recording without fake strategies. Polymarket has real live markets/orderbooks/resolution corpus and provenance-aware relationships without trading. Global UI exposes venue/collector/storage/model/experiment/paper health.

## Operator report
Final implementation report must include branch/commits, functionality shipped, Pump collector/backfill/EXEC-SELECT status, Hyperliquid market/observation/history/label/paper/model status and limitations, Ostium market/funding/rollover/OI/flow/liquidation status and limitations, Polymarket market/orderbook/resolution/relationship status and limitations, storage use/growth/30d/90d estimates, API/key requirements, exact tests/results, UI routes/screenshots, measured vs estimated truth, unsupported items, and next three highest-value actions. No marketing language.

## Implementation decomposition
1. Protect and inspect Pump/live-process state.
2. Add minimal shared venue/research contracts.
3. Hyperliquid live ingestion/durable state.
4. Hyperliquid lifecycle snapshots/labels.
5. Hyperliquid Execution Twin.
6. HIP-NEW-MARKET-V1 baselines/experiments.
7. Specialist models/rankers only after valid labels.
8. Hyperliquid UI.
9. Ostium read-only adapter/protocol truth/quote recorder/UI.
10. Polymarket read-only adapter/resolution corpus/relationships/UI.
11. Global venue/storage/data-health surfaces.
12. Regression/integration/truth verification.
13. Operator report.

Parallelize only independent adapters/tests/UI that do not mutate shared schemas/stores. A single lead path owns shared schemas, migrations, experiment contracts and acceptance gates.
