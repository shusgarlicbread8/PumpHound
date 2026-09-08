# Live Terminal and Paper Desk Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make real launches visible in the desktop and complete a real-time paper-order lifecycle before building advanced graph/ML features.

**Architecture:** The collector journals and decodes ordered chain events; core-api reduces them into market state and publishes UI deltas. The paper engine uses the same state plus a versioned future-arrival simulator and transactional ledger.

**Tech Stack:** Rust/Tokio/Axum, Tauri/React, PostgreSQL, ClickHouse, JetStream, Lightweight Charts.

**Spec:** [UX](../spec/01-product-and-ux.md), [data protocols](../spec/03-data-sources-and-protocols.md), [paper/replay](../spec/06-paper-trading-and-replay.md), [domain](../contracts/01-domain-and-event-contracts.md), [API](../contracts/03-api-and-stream-contracts.md).

## Global Constraints

- V1 contains no transaction signer or live-money submission path.
- Atomic quantities and slots cross JSON boundaries as decimal strings.
- Unknown/stale/partial data stays explicit.
- Build with narrow smoke checks now; comprehensive fixtures and stress suites are milestone 9.

---

### Task 1 — Capture actual Solana transactions

**Files:** `services/collector/src/main.rs`, `services/collector/src/config.rs`, `crates/chain-source/src/{lib,laserstream,websocket}.rs`, `crates/domain/src/{units,time,raw}.rs`, `compose.yaml`, `.env.example`, `justfile`, root Rust manifests.

**Consumes:** provider endpoint/key references and configured program IDs.

**Produces:** `ChainSource::run(cursor, sink)` emitting `RawChainEnvelope`; `just capture-live`.

- [ ] Create only the Cargo members used in this task; add Docker services for the three selected stores because this slice immediately persists to them.
- [ ] Load typed configuration; check whether Helius mainnet gRPC entitlement exists. Otherwise use configured Solana websocket/RPC. Print `LIVE MAINNET / LASERSTREAM` or `LIVE MAINNET / RPC FALLBACK`.
- [ ] Subscribe separately to the two program IDs, slots and required account/block metadata. Preserve successful and failed transaction status distinctly.
- [ ] Journal received batches before acknowledging progress; store payload hash, source cursor and receipt time.
- [ ] Reconnect with bounded exponential backoff and jitter; resume cursor where supported and create a visible backfill job for gaps.
- [ ] Wire a diagnostic output displaying slot, signature, source lag and raw record ID. Run until several real program transactions arrive; do not treat an empty connection as success.

Control flow:

```rust
while let Some(raw) = receiver.recv().await {
    let durable = journal.append(raw).await?;
    raw_store.persist_batch(&durable).await?;
    checkpoints.record_persisted(&durable).await?;
    decode_queue.send(durable).await?;
}
```

`journal`, `raw_store`, `checkpoints` and `decode_queue` are instantiated dependencies of collector, not global singletons. The durable batch contains its ID, hash, rows and cursor.

**Smoke evidence:** captured row count, exact signatures, non-null observed timestamps, source lag and a successful restart from the last cursor.

### Task 2 — Decode Pump and PumpSwap into canonical events

**Files:** `vendor/pump-idl/{pump.json,pump_amm.json,pump_fees.json,SOURCE.lock}`, `crates/protocol-pump/src/{lib,events,accounts}.rs`, `crates/protocol-pumpswap/src/{lib,events,accounts}.rs`, `crates/domain/src/events.rs`, `crates/storage/src/{clickhouse,checkpoints,publication}.rs`, `db/postgres/001_core.sql`, `db/clickhouse/001_events.sql`.

**Consumes:** raw transactions/accounts and official pinned IDLs.

**Produces:** `LaunchpadDecoder::decode(&RawTransaction) -> Result<Vec<CanonicalEvent>, DecodeError>` and `chain.normalized.v1`.

- [ ] Download the official IDLs and record commit/hash; derive allowlisted program IDs from their `address` fields and verify the expected IDs.
- [ ] Decode Anchor event payloads and inner instruction/CPI structure, including versioned transactions and loaded address-table keys.
- [ ] Implement creation, trade, completion/migration, pool creation, liquidity changes and fee/config snapshots. Normalize quote mint/decimals and preserve original raw fields.
- [ ] Detect failed transactions and settlement-transfer roles so failed or duplicate economic activity does not enter the trade tape.
- [ ] Assign deterministic event IDs/revisions, quarantine unknown discriminators, persist canonical revisions, then publish with idempotent batch accounting.
- [ ] Inspect one creation, buy, sell and migrated pool from actual captured or explicitly backfilled data. Compare normalized amounts/reserves against raw event fields.

**Smoke evidence:** four named event categories with evidence IDs; an unknown-layout record visibly quarantined rather than guessed.

### Task 3 — Maintain executable market state

**Files:** `crates/market-state/src/{lib,reducer,curve,pool,checkpoints,reconcile}.rs`, `crates/domain/src/market.rs`, `services/core-api/src/{main,state,consumer}.rs`, `services/core-api/src/bin/inspect_launch.rs`.

**Consumes:** canonical event revisions and protocol-config snapshots.

**Produces:** `MarketReducer::apply`, `MarketReducer::snapshot` and `just inspect-launch <mint>`.

- [ ] Apply events atomically per transaction, ordered by slot/transaction/instruction/event index.
- [ ] Keep reserves in checked integers; include PumpSwap virtual quote reserves; bind every market to a quote mint and config version.
- [ ] Bootstrap markets discovered mid-life from consistent account snapshots and replay events after the snapshot cutoff.
- [ ] Treat account writes as reconciliation, not a second application of trade deltas.
- [ ] Persist checkpoints and retraction/reconciliation state. Freeze executable quotes if order or state is unknown.
- [ ] Expose an inspection command showing current curve/pool reserves, last trade, completion/migration and quality.

**Smoke evidence:** a market changes through several real trades; reconnect does not multiply balances/volume; an existing pool is correctly bootstrapped.

### Task 4 — Render the live Radar in Tauri

**Files:** `apps/desktop/src/app/{App,router}.tsx`, `apps/desktop/src/features/radar/{RadarPage,RadarTable,radarColumns}.tsx`, `apps/desktop/src/lib/{api,stream}.ts`, `apps/desktop/src/styles/tokens.css`, `apps/desktop/src-tauri/src/{lib,main}.rs`, `services/core-api/src/routes/{health,launches,stream}.rs`.

**Consumes:** `GET /launches`, `/health`, `/system/status` and `radar` stream.

**Produces:** a packaged-app live launch table, navigation and connection status.

- [ ] Build the specified dark terminal layout, sidebar, top mode/health bar and virtualized table.
- [ ] Render identity, age, quote asset, price, flow, buyers, curve progress and quality using real API values.
- [ ] Leave untrained probabilities visibly unavailable; implement a labelled heuristic rank only from actual market features.
- [ ] Add sort/filter/pause-display controls. Display pause freezes the view, not ingestion or paper risk management.
- [ ] Implement snapshot + sequenced delta hydration and reconnect resync. New launches appear without page refresh.
- [ ] Make `just dev` start the live app; show only implemented navigation destinations until their tasks land.

**Smoke evidence:** screenshot and short captured sequence showing a new launch enter Radar and a sort/filter remaining stable under updates.

### Task 5 — Token detail, chart and trade tape

**Files:** `apps/desktop/src/features/token/{TokenPage,TokenHeader,PriceChart,TradeTape,CurvePanel,MarketMetrics}.tsx`, `apps/desktop/src/lib/{format,units}.ts`, `services/core-api/src/routes/{token,trades,candles}.rs`, `crates/feature-core/src/{windows,market}.rs`.

**Consumes:** selected Radar launch, canonical trades and market snapshots.

**Produces:** token overview with real-time chart/tape/reserves and deep links.

- [ ] Implement token header, copy mint/address, venue/quote badges, age and commitment state.
- [ ] Aggregate event-derived candles with an explicit interval; show no-trade gaps and quote unit. Do not use candle close as executable fill price.
- [ ] Render newest trades with side, base/quote amount, wallet, age and evidence link.
- [ ] Compute rolling buyer/flow/retention/price-impact metrics with bounded windows.
- [ ] Make migration switch the market view without changing token/launch identity; preserve pre-migration history.
- [ ] Support keyboard navigation and chart crosshair synchronized to the tape cutoff.

**Smoke evidence:** open a live launch from Radar, see new trades/curve change and navigate through a migrated token's two venues.

### Task 6 — Evidence drawer and safe metadata

**Files:** `apps/desktop/src/components/{EvidenceDrawer,QualityBadge,EmptyState}.tsx`, `services/core-api/src/routes/evidence.rs`, `services/core-api/src/metadata/{fetch,sanitize,images}.rs`.

**Consumes:** evidence IDs and token metadata URI.

**Produces:** source-grounded details and safe token imagery/text.

- [ ] Render evidence class, signature, slot, source/decoder version, timestamps and source links.
- [ ] Fetch metadata server-side with SSRF protections, size limits, redirect revalidation and sanitized image caching.
- [ ] Show unavailable/blocked metadata as a neutral fallback icon; do not remove the token from Radar.
- [ ] Add explicit quality explanations to every affected panel.
- [ ] Wire copy/open-explorer actions only for validated public keys/signatures and allowlisted explorer URL templates.

**Smoke evidence:** normal metadata renders; a private-address URL is blocked; the underlying token/tape remains usable.

### Task 7 — System, settings, watchlists and alerts

**Files:** `apps/desktop/src/features/system/{SystemPage,ProviderPanel,JobsPanel}.tsx`, `apps/desktop/src/features/settings/SettingsPage.tsx`, `apps/desktop/src/features/radar/{SavedFilters,WatchlistPanel}.tsx`, `services/core-api/src/routes/{settings,watchlists,alerts,jobs}.rs`, `services/core-api/src/alerts/{rules,evaluate}.rs`.

**Consumes:** provider capabilities, health, canonical state and alert rules.

**Produces:** configurable live product with persistent lists, alerts and visible degradation.

- [ ] Store non-secret settings in Postgres and secrets by OS credential-store reference.
- [ ] Add provider connection checks, usage ceilings and source selection; never display a secret after saving.
- [ ] Implement saved filters/watchlists and in-app/desktop notifications for new launch, curve threshold, flow burst and data degradation.
- [ ] Add cooldown/deduplication, acknowledgement and evidence links.
- [ ] Show queue lag, decode quarantine, gap-recovery jobs, coverage and disk status.

**Smoke evidence:** watchlist survives restart; one rule fires once during its cooldown; a disabled provider appears degraded without crashing the app.

### Task 8 — Exact quote math and fee versions

**Files:** `crates/paper-core/src/{lib,quote,fees,latency,venue}.rs`, `crates/protocol-pump/src/quote.rs`, `crates/protocol-pumpswap/src/quote.rs`, `services/core-api/src/routes/paper_quotes.rs`.

**Consumes:** MarketSnapshot, protocol configuration and PaperOrderRequest input fields.

**Produces:** `PaperVenue::quote -> PaperQuote` and `POST /paper/quotes`.

- [ ] Implement quote arithmetic from pinned official SDK/program documentation using checked integer intermediates and exact rounding.
- [ ] Represent gross input, reserve input, protocol/creator/LP fees, output and network fees separately; prevent double charging.
- [ ] Model native SOL settlement versus wrapped-SOL mint identity, token-account rent lock/refund and optional transfer-fee extensions.
- [ ] Reject unsupported token-program extensions, fee config, quote mint or pool layout with a specific reason.
- [ ] Add arrival-state quotes, slippage minimums and fee-version lookup by slot.
- [ ] Optionally compare migrated-market quotes using Jupiter `/swap/v2/build`; never call execute/submit.

**Smoke evidence:** fee breakdown and expected/minimum output for one curve and one PumpSwap market; quote becomes unavailable when its config is removed.

### Task 9 — Paper orders, fill lifecycle and ledger

**Files:** `crates/paper-core/src/{engine,orders,ledger,lots,reservations}.rs`, `crates/storage/src/paper.rs`, `services/core-api/src/routes/{paper_orders,portfolios}.rs`, `db/postgres/002_paper.sql`.

**Consumes:** accepted quote, policy/risk snapshot and future canonical events.

**Produces:** immutable order transitions, PaperFill, cash movements, lots and positions.

- [ ] Create paper portfolio with explicit starting asset lots; default $1,000 is a valuation budget converted into configured SOL/USDC paper cash using the current reference price.
- [ ] Under a portfolio/run lock, check and reserve input plus worst-case fees; write decision/order/outbox atomically.
- [ ] Queue simulated arrival after the configured latency; fill only against a state at or after arrival.
- [ ] Enforce atomic all-or-reject outcome, no invented partial fill.
- [ ] Commit fill, movements, lot creation/consumption and publication in one transaction; failed landed transactions can incur only the modeled applicable costs.
- [ ] Add canonical correction branches for orphaned fills, preserving the original result.
- [ ] Add reconciliation command for balances, reservations and consumed lots.

```text
spendable(asset) = SUM(ledger signed_atomic) - SUM(active reservations)
remaining_lot = acquired_atomic - SUM(consumed_atomic)
admit order only when every spendable asset remains nonnegative
```

**Smoke evidence:** buy and sell a live token on paper; inspect exact ledger movements; repeated client request returns the same order; ledger reconciles.

### Task 10 — Paper desk GUI and risk controls

**Files:** `apps/desktop/src/features/paper/{PaperPage,OrderTicket,CostPreview,PositionsTable,OrdersTable,EquityChart,RiskPanel,LedgerDrawer}.tsx`, `crates/paper-core/src/{risk,marks}.rs`, `services/core-api/src/routes/paper_performance.rs`.

**Consumes:** paper APIs and `paper:{portfolio_id}` stream.

**Produces:** fully usable manual live paper desk.

- [ ] Add a token-page paper ticket with notional, slippage, cost preview and explicit `PAPER BUY`/`PAPER SELL` confirmation.
- [ ] Render available/reserved cash, executable position value, gross/net P&L, orders, fill failures and fees.
- [ ] Implement per-token, gross, creator/cluster, daily-loss and drawdown limits from the versioned policy.
- [ ] Keep exits active when entry breakers trip; show audited reset controls.
- [ ] Add partial position exits as deliberately smaller separate atomic orders, not partial fills.
- [ ] Add exact ledger and provenance drawer for each trade.

**Smoke evidence:** user places a paper order through GUI, sees its latency/fill and closes it; an over-limit order is visibly rejected.

### Task 11 — Automated heuristic paper policy

**Files:** `crates/paper-core/src/{policy,exits,decision_log}.rs`, `apps/desktop/src/features/paper/PolicyEditor.tsx`, `services/core-api/src/routes/policies.rs`, `ml/configs/paper-conservative-v1.yaml`.

**Consumes:** current market features, quality and portfolio risk state; no trained-model dependency yet.

**Produces:** unattended live paper decisions with explicit heuristic version.

Begin with [the supplied default policy](../contracts/04-default-policy.yaml); it is disabled until the user starts it and is labelled as a research heuristic, not a proven strategy.

- [ ] Implement quality/cost/liquidity/age/independent-buyer gates using measured inputs; keep graph gates unavailable until M4.
- [ ] Build configurable fixed-stop, staged-profit, time-stop and trailing-exit policies with deterministic precedence.
- [ ] Save every decision including abstention; persist before placing a paper order.
- [ ] Expose start/pause/new-entry-disable controls with current policy hash and active constraints.
- [ ] Make policy changes create a new immutable version and audit event.
- [ ] Run a bounded live paper session and display accepted/rejected decisions and cost attribution, without asserting profitability.

**Milestone 3 outcome:** the user can already observe actual Solana launches, investigate transaction evidence, manually paper trade and run a labelled heuristic paper strategy. Advanced models come next.
