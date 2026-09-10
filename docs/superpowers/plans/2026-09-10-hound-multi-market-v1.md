# Hound Multi-Market V1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expand PumpHound into a paper-only multi-market research terminal with a deep Hyperliquid laboratory plus read-only Ostium and Polymarket evidence collectors, without regressing or interrupting the live Pump research stack.

**Architecture:** Keep the Solana/Pump hot path intact. Add a thin venue-neutral contract crate and a separate non-Solana market collector so Hyperliquid, Ostium, and Polymarket can fail, rate-limit, and persist independently. Hyperliquid gets its own execution twin and research/ML package; Ostium and Polymarket stop at truthful durable observation and UI. Core API and desktop expose the new work through small venue-specific modules instead of making the existing large Pump route/paper files larger.

**Tech Stack:** Rust 1.85 workspace, Tokio, reqwest/tokio-tungstenite, Axum, SQLx/PostgreSQL, ClickHouse, NATS, Python/uv with LightGBM/XGBoost, React 19 + TanStack Query/Router/Table + lightweight-charts, Tauri 2, pnpm, existing PumpHound verification tooling.

**Spec:** `docs/superpowers/specs/2026-09-10-hound-multi-market-research-terminal-design.md`

## Global Constraints

- Start from the user's current local HEAD, not blindly from the design-doc branch; preserve any newer work.
- Never reset, clean, stash, stop, or overwrite user work or running Pump processes merely to make the branch convenient.
- Do not disrupt the active Pump history/backfill process or start another Helius-heavy job while it is using quota.
- Do not refactor stable Pump-specific code merely to fit shared abstractions.
- Hyperliquid is the only new venue allowed through paper execution and ML in this cycle.
- Ostium and Polymarket remain read-only research/data products in this cycle.
- No private keys, wallet signing, live orders, or real-money execution.
- Unknown data remains unknown; never coerce missing market data to zero.
- Time-series validation is chronological/walk-forward; no random split for trading claims.
- Every trading label and reported edge must be post-cost and execution-aware.
- Public/read-only venue APIs are preferred. Missing optional credentials degrade the feature to `DISABLED_MISSING_KEY` instead of blocking the run.
- A provider 429/outage degrades only that provider/venue and must not stall Pump or other venue collectors.
- Make small coherent commits after each independently testable task.

---

### Task 1: Establish a non-destructive operational baseline

**Files:**
- Read: `docs/superpowers/specs/2026-09-10-hound-multi-market-research-terminal-design.md`
- Read: `docs/KNOWN_LIMITATIONS.md`
- Read: `docs/OPERATIONS.md`
- Read: `docs/EXECUTION_TWIN.md`
- Read: `justfile`
- Create only if useful to preserve evidence: `artifacts/verification/multi-market-v1/baseline.md`

**Interfaces:**
- Consumes: current local git/process/store state.
- Produces: a recorded baseline and a safe feature branch from the current local HEAD.

- [ ] **Step 1: Inspect without mutating anything**

```bash
git status --short --branch
git log -8 --oneline
ps aux | grep -E 'backfill|pumphound-collector|pumphound-core-api' | grep -v grep || true
df -h . "${PUMPHOUND_DATA_DIR:-./data}" 2>/dev/null || df -h .
du -sh data/history-recent-90d 2>/dev/null || true
```

Record the current branch/HEAD, dirty files, active Pump processes, history-corpus size, and free disk. Do not stop or restart anything.

- [ ] **Step 2: Reconcile the approved spec with the current HEAD**

If the spec file is absent because the local HEAD advanced on another branch, retrieve the file from commit `a7df35a1851ac0e49eb52e535a9a76e38b85691a` without resetting the worktree. The current local code is authoritative when it contains newer Pump fixes.

- [ ] **Step 3: Create a safe implementation branch only when non-destructive**

```bash
git switch -c feat/hound-multi-market-v1
```

If the current worktree is dirty, do not use `git reset`, `git clean`, or an automatic stash. Continue on a safe branch only after understanding which files belong to the user's active work.

- [ ] **Step 4: Run existing deterministic verification**

```bash
just verify
just execution-twin-test
```

If an existing test is already failing, record the exact pre-existing failure and do not silently attribute it to new work.

- [ ] **Step 5: Commit only baseline documentation if it was created**

```bash
git add artifacts/verification/multi-market-v1/baseline.md
git commit -m "docs: record multi-market implementation baseline"
```

Do not make an empty commit.

---

### Task 2: Add minimal venue-neutral contracts

**Files:**
- Create: `crates/venue-core/Cargo.toml`
- Create: `crates/venue-core/src/lib.rs`
- Create: `crates/venue-core/src/health.rs`
- Create: `crates/venue-core/src/instrument.rs`
- Create: `crates/venue-core/src/observation.rs`
- Modify: `Cargo.toml`

**Interfaces:**
- Produces: `Venue`, `InstrumentId`, `InstrumentKind`, `ObservationId`, `ObservationTime`, `DataHealth`, `HealthState`, and typed optional observation metadata.
- Consumes: only workspace serde/time/uuid dependencies; it must not depend on Pump crates.

- [ ] **Step 1: Write contract tests before implementation**

Add tests in the new modules asserting that: a Hyperliquid instrument can exist without Pump fields; missing values serialize as `null`/absent rather than `0`; `HealthState` supports `Healthy`, `Stale`, `Degraded`, `DisabledMissingKey`, `Unsupported`, and `Unknown`; observation timestamps preserve source and received-at time separately.

Example test shape:

```rust
#[test]
fn unknown_numeric_value_is_not_zero() {
    let obs = MarketObservationMeta::new(Venue::Hyperliquid, "xyz:NVDA", 10);
    assert_eq!(obs.sequence, Some(10));
    assert!(obs.source_timestamp.is_none());
}
```

- [ ] **Step 2: Run the new crate test and confirm RED**

```bash
cargo test -p pumphound-venue-core
```

Expected before implementation: compile/test failure because the crate/types do not exist.

- [ ] **Step 3: Implement the smallest neutral contract set**

Keep the types deliberately small. Do not add creator, funding, rollover, resolution, or bonding-curve fields to common structs. Venue-specific payloads will own those.

- [ ] **Step 4: Run tests and workspace check**

```bash
cargo test -p pumphound-venue-core
cargo check --workspace
```

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/venue-core
git commit -m "feat: add venue-neutral research contracts"
```

---

### Task 3: Add multi-market durable storage without changing Pump tables

**Files:**
- Create: `db/postgres/020_multi_market_core.sql`
- Create: `db/clickhouse/003_multi_market.sql`
- Create: `crates/storage/src/venues.rs`
- Modify: `crates/storage/src/lib.rs`
- Test: add focused Rust storage tests next to `venues.rs` or existing storage test module.

**Interfaces:**
- Consumes: `pumphound_venue_core` identifiers/health.
- Produces storage methods for instruments, normalized observations, provider health, retention metrics, and venue-specific JSON payloads with provenance.

- [ ] **Step 1: Write failing persistence tests**

Cover idempotent instrument upsert, append-only observation insert, unknown-value round trip, source vs received timestamp, provider-health upsert, and duplicate observation key rejection/idempotency.

- [ ] **Step 2: Run focused tests and confirm RED**

```bash
cargo test -p pumphound-storage -- venues
```

- [ ] **Step 3: Implement schemas**

Postgres should hold durable identities, lifecycle/current state, provider health, experiment metadata, and lower-frequency normalized snapshots. ClickHouse should hold higher-frequency append-only observations/order-book/depth/quote samples. Use explicit `venue`, `instrument_id`, `source_ts`, `received_ts`, `schema_version`, and provenance fields. Do not alter or repurpose existing Pump event tables.

- [ ] **Step 4: Implement storage module**

Expose focused methods such as:

```rust
pub async fn upsert_venue_instrument(&self, instrument: &VenueInstrumentRecord) -> anyhow::Result<()>;
pub async fn append_venue_observations(&self, rows: &[VenueObservationRecord]) -> anyhow::Result<()>;
pub async fn set_provider_health(&self, health: &ProviderHealthRecord) -> anyhow::Result<()>;
```

Use the project's existing `Stores` connection and migration patterns.

- [ ] **Step 5: Verify**

```bash
cargo test -p pumphound-storage -- venues
just verify
```

- [ ] **Step 6: Commit**

```bash
git add db/postgres/020_multi_market_core.sql db/clickhouse/003_multi_market.sql crates/storage Cargo.toml Cargo.lock
git commit -m "feat: persist multi-market observations and health"
```

---

### Task 4: Add an isolated non-Solana market collector supervisor

**Files:**
- Create: `services/market-collector/Cargo.toml`
- Create: `services/market-collector/src/main.rs`
- Create: `services/market-collector/src/config.rs`
- Create: `services/market-collector/src/supervisor.rs`
- Create: `services/market-collector/src/retention.rs`
- Modify: `Cargo.toml`
- Modify: `compose.yaml` only if the repo's local process model benefits from it; otherwise expose a `just` target first.
- Modify: `justfile`

**Interfaces:**
- Produces an independent process that supervises Hyperliquid, Ostium, and Polymarket read-only adapters.
- Each adapter reports health independently and sends/persists normalized observations without using Helius.

- [ ] **Step 1: Write supervisor tests**

Use fake providers proving that one provider returning 429/backing off does not prevent another provider from emitting observations, and that stale timestamps become `Stale` without becoming zero-valued market state.

- [ ] **Step 2: Confirm RED**

```bash
cargo test -p pumphound-market-collector
```

- [ ] **Step 3: Implement provider supervisor**

Define a small adapter contract such as:

```rust
#[async_trait::async_trait]
pub trait VenueAdapter: Send + Sync {
    fn venue(&self) -> Venue;
    async fn run(self: Arc<Self>, sink: ObservationSink) -> anyhow::Result<()>;
}
```

Backoff must be bounded with jitter; status changes must be persisted. Never retry a 429 in a tight loop.

- [ ] **Step 4: Add operator targets**

Add a `just capture-markets` target that starts only the new market collector against existing stores, and extend `just dev` only after proving it does not interfere with the Pump collector.

- [ ] **Step 5: Verify and commit**

```bash
cargo test -p pumphound-market-collector
cargo check --workspace
just verify

git add Cargo.toml Cargo.lock services/market-collector justfile compose.yaml
git commit -m "feat: add isolated multi-market collector supervisor"
```

Only add `compose.yaml` if actually modified.

---

### Task 5: Implement the Hyperliquid read-only adapter and live archive

**Files:**
- Create: `crates/venue-hyperliquid/Cargo.toml`
- Create: `crates/venue-hyperliquid/src/lib.rs`
- Create: `crates/venue-hyperliquid/src/client.rs`
- Create: `crates/venue-hyperliquid/src/types.rs`
- Create: `crates/venue-hyperliquid/src/normalize.rs`
- Create: `crates/venue-hyperliquid/src/lifecycle.rs`
- Create: `crates/venue-hyperliquid/src/depth.rs`
- Create: `crates/venue-hyperliquid/tests/fixtures/*.json`
- Create: `services/market-collector/src/hyperliquid.rs`
- Modify: `services/market-collector/src/main.rs`
- Modify: `Cargo.toml`

**Interfaces:**
- Produces normalized native/HIP-1/HIP-3 market identities, L2/depth, trades, mark/oracle/index, OI, funding, liquidation observations, risk parameters, and freshness.
- Uses current official Hyperliquid public/read-only API semantics discovered at implementation time; do not hardcode fee/risk values from old notes when the API exposes truth.

- [ ] **Step 1: Capture small sanitized fixtures from current official endpoints**

Do not put secrets in fixtures. Record enough responses to test market metadata, L2, funding/OI, and at least one HIP-3 market if currently available.

- [ ] **Step 2: Write failing parser/normalizer tests**

Tests must cover native vs HIP-3 classification, unknown metadata, bid/ask ordering, USD depth bucket calculation, funding sign preservation, and source timestamp/freshness.

- [ ] **Step 3: Confirm RED**

```bash
cargo test -p pumphound-venue-hyperliquid
```

- [ ] **Step 4: Implement public client + normalization**

Prefer WebSocket for live book/trades when officially supported and use HTTP snapshots for reconciliation/metadata. Add explicit reconnect and sequence-gap handling. Never pretend a stale book is current.

- [ ] **Step 5: Wire collector persistence**

Persist high-frequency book/depth/trade samples in ClickHouse and instrument/current/lifecycle truth in Postgres through Task 3 interfaces. Measure bytes written during a bounded run.

- [ ] **Step 6: Run a bounded live smoke test**

Run the collector for several minutes while the Pump history process remains untouched. Verify real market count, observation count, current funding/OI rows, and nonzero depth rows. Record actual storage growth.

- [ ] **Step 7: Verify and commit**

```bash
cargo test -p pumphound-venue-hyperliquid
cargo test -p pumphound-market-collector -- hyperliquid
just verify

git add Cargo.toml Cargo.lock crates/venue-hyperliquid services/market-collector
git commit -m "feat: ingest Hyperliquid market state"
```

---

### Task 6: Persist Hyperliquid lifecycle-age snapshots and economic labels

**Files:**
- Create: `db/postgres/021_hyperliquid_research.sql`
- Create: `crates/venue-hyperliquid/src/snapshots.rs`
- Create: `crates/venue-hyperliquid/src/labels.rs`
- Modify: `crates/venue-hyperliquid/src/lib.rs`
- Modify: `crates/storage/src/venues.rs`
- Test: `crates/venue-hyperliquid` unit/integration tests.

**Interfaces:**
- Produces age snapshots for T0/1s/3s/10s/30s/60s/5m/15m/1h/6h/24h/3d/7d with actual observation delay, and future-outcome records that are not available until their horizon closes.

- [ ] **Step 1: Write failing temporal tests**

Use deterministic timestamps proving: nearest observation is chosen without backdating; data arriving after a decision cannot appear in its feature snapshot; labels remain pending before horizon close; market lifecycle transitions are monotonic unless a venue event explicitly reopens/replaces a market.

- [ ] **Step 2: Confirm RED**

```bash
cargo test -p pumphound-venue-hyperliquid -- snapshots
cargo test -p pumphound-venue-hyperliquid -- labels
```

- [ ] **Step 3: Implement snapshot scheduler/materializer**

Store target age, actual source timestamp, actual received timestamp, and delay. Support `UNKNOWN` fields explicitly.

- [ ] **Step 4: Implement initial non-trading and economic labels**

Include maturation labels, basis movement where reference truth exists, funding integrals, and post-cost MFE/MAE hooks. Do not create a directional trade label from a price candle without execution state.

- [ ] **Step 5: Verify and commit**

```bash
cargo test -p pumphound-venue-hyperliquid
cargo test -p pumphound-storage -- venues

git add db/postgres/021_hyperliquid_research.sql crates/venue-hyperliquid crates/storage
git commit -m "feat: add Hyperliquid lifecycle snapshots and labels"
```

---

### Task 7: Build the Hyperliquid Execution Twin

**Files:**
- Create: `crates/hyperliquid-twin/Cargo.toml`
- Create: `crates/hyperliquid-twin/src/lib.rs`
- Create: `crates/hyperliquid-twin/src/contract.rs`
- Create: `crates/hyperliquid-twin/src/fill.rs`
- Create: `crates/hyperliquid-twin/src/fees.rs`
- Create: `crates/hyperliquid-twin/src/funding.rs`
- Create: `crates/hyperliquid-twin/src/margin.rs`
- Create: `crates/hyperliquid-twin/src/twin.rs`
- Create: `db/postgres/022_hyperliquid_paper.sql`
- Modify: `Cargo.toml`

**Interfaces:**
- Consumes immutable decision-time and arrival-time Hyperliquid market snapshots.
- Produces `HyperliquidExecutionIntent`, `HyperliquidExecutionResult`, cost components, fill/failure reason, paper position updates, funding accrual, and final post-cost P&L.

- [ ] **Step 1: Write failing deterministic twin tests**

Tests must prove: taker crossing consumes visible levels; insufficient depth partially fills or rejects per policy; maker orders do not fill solely because a candle touched; fee values come from versioned config/live truth rather than a magic constant; funding accrues by held notional/time; liquidation/margin paths close consistently; replaying the same snapshots gives the same result.

- [ ] **Step 2: Confirm RED**

```bash
cargo test -p pumphound-hyperliquid-twin
```

- [ ] **Step 3: Implement minimal deterministic twin**

Use L2 snapshots and a configurable latency/queue-fidelity model. If queue position cannot be observed, default to conservative fill assumptions and report fidelity explicitly.

- [ ] **Step 4: Add paper persistence**

Persist intents, arrival snapshots, fills/failures, costs, funding, positions, exits, and trace IDs. Keep these tables separate from Pump's execution-twin tables unless a truly neutral shared table already fits without semantic loss.

- [ ] **Step 5: Verify and commit**

```bash
cargo test -p pumphound-hyperliquid-twin
cargo check --workspace

git add Cargo.toml Cargo.lock crates/hyperliquid-twin db/postgres/022_hyperliquid_paper.sql
git commit -m "feat: add Hyperliquid paper execution twin"
```

---

### Task 8: Add `HIP-NEW-MARKET-V1` deterministic research books

**Files:**
- Create: `crates/hyperliquid-research/Cargo.toml`
- Create: `crates/hyperliquid-research/src/lib.rs`
- Create: `crates/hyperliquid-research/src/eligibility.rs`
- Create: `crates/hyperliquid-research/src/books.rs`
- Create: `crates/hyperliquid-research/src/metrics.rs`
- Create: `crates/hyperliquid-research/src/strategy_diff.rs`
- Modify: `Cargo.toml`

**Interfaces:**
- Produces one experiment family `HIP-NEW-MARKET-V1` with RANDOM_NEW_MARKET, MICROSTRUCTURE_BASELINE, MATURATION_RANKER, STRUCTURAL_EDGE, FULL_QUANT, and FULL_QUANT_AI_SHADOW identities; only books backed by available models become active.

- [ ] **Step 1: Write comparability tests**

Prove that random and deterministic books use the same eligible cohort/timing/size/execution assumptions unless explicitly declared. A comparison with mismatched assumptions must return `CONFOUNDED`, not a lift number.

- [ ] **Step 2: Confirm RED**

```bash
cargo test -p pumphound-hyperliquid-research
```

- [ ] **Step 3: Implement eligibility + first two executable baselines**

Make RANDOM_NEW_MARKET and MICROSTRUCTURE_BASELINE functional against the Hyperliquid Twin. Register the other book identities as `INSUFFICIENT_DATA`/`NO_MODEL` until their prerequisites exist; do not fake decisions.

- [ ] **Step 4: Implement economic metrics**

Report post-cost expectancy, net P&L, drawdown, profit factor, turnover, capacity estimate, tail concentration, and age bucket. Never rank by raw classification accuracy alone.

- [ ] **Step 5: Verify and commit**

```bash
cargo test -p pumphound-hyperliquid-research

git add Cargo.toml Cargo.lock crates/hyperliquid-research
git commit -m "feat: add Hyperliquid new-market research baselines"
```

---

### Task 9: Add Hyperliquid ML/ranking pipeline only on valid labels

**Files:**
- Create: `ml/pumphound_ml/venues/__init__.py`
- Create: `ml/pumphound_ml/venues/hyperliquid/__init__.py`
- Create: `ml/pumphound_ml/venues/hyperliquid/dataset.py`
- Create: `ml/pumphound_ml/venues/hyperliquid/features.py`
- Create: `ml/pumphound_ml/venues/hyperliquid/labels.py`
- Create: `ml/pumphound_ml/venues/hyperliquid/train.py`
- Create: `ml/pumphound_ml/venues/hyperliquid/ranking.py`
- Create: `ml/pumphound_ml/venues/hyperliquid/evaluate.py`
- Create: `tests/python/test_hyperliquid_dataset.py`
- Create: `tests/python/test_hyperliquid_walk_forward.py`
- Create: `tests/python/test_hyperliquid_ranking.py`
- Create: `ml/configs/hyperliquid-new-market-v1.yaml`

**Interfaces:**
- Consumes completed point-in-time Hyperliquid snapshot/label rows.
- Produces versioned maturation/funding/basis/ranking candidates and walk-forward reports; no model is promoted merely because training succeeded.

- [ ] **Step 1: Write leakage and chronology tests**

Create synthetic timestamped rows proving future observations cannot enter feature rows and that train/validation/test folds are strictly chronological.

- [ ] **Step 2: Confirm RED**

```bash
uv run pytest tests/python/test_hyperliquid_dataset.py tests/python/test_hyperliquid_walk_forward.py tests/python/test_hyperliquid_ranking.py -q
```

- [ ] **Step 3: Implement dataset/features/labels adapters**

Reuse generic evaluation utilities only when their assumptions match. Do not import Pump creator/graph features into Hyperliquid datasets.

- [ ] **Step 4: Implement baseline models then rankers**

Train simple deterministic/statistical baselines before LightGBM/XGBoost. Report top 1/5/10/20 percent economic outcomes and compare against the deterministic books.

- [ ] **Step 5: Gate model activation on sample truth**

When the live dataset is not yet sufficient for a horizon, report `INSUFFICIENT_SAMPLE`; do not synthesize a champion. Longer-horizon labels may naturally be unavailable in this first run.

- [ ] **Step 6: Verify and commit**

```bash
uv run pytest tests/python/test_hyperliquid_dataset.py tests/python/test_hyperliquid_walk_forward.py tests/python/test_hyperliquid_ranking.py -q
just verify

git add ml/pumphound_ml/venues ml/configs/hyperliquid-new-market-v1.yaml tests/python
git commit -m "feat: add leakage-safe Hyperliquid research models"
```

---

### Task 10: Expose venue APIs without expanding the giant Pump route module

**Files:**
- Create: `services/core-api/src/venue_health.rs`
- Create: `services/core-api/src/hyperliquid_routes.rs`
- Create: `services/core-api/src/ostium_routes.rs`
- Create: `services/core-api/src/prediction_routes.rs`
- Modify: `services/core-api/src/main.rs`
- Modify: `services/core-api/src/routes.rs` only for small router nesting/wiring.
- Modify: `services/core-api/src/state.rs` only for small shared handles if required.
- Modify: `schemas/openapi/pumphound-v1.yaml`

**Interfaces:**
- Produces read APIs under `/api/v1/venues`, `/api/v1/hyperliquid/...`, `/api/v1/ostium/...`, and `/api/v1/prediction/...`.

- [ ] **Step 1: Write route tests for truth states**

Assert that unavailable provider data returns explicit stale/degraded/unsupported state and nullable fields, not HTTP 200 with fabricated zeros.

- [ ] **Step 2: Confirm RED**

```bash
cargo test -p pumphound-core-api -- venue
cargo test -p pumphound-core-api -- hyperliquid
```

- [ ] **Step 3: Implement thin route modules**

Query durable store/research crates; do not duplicate market calculations in HTTP handlers.

- [ ] **Step 4: Document OpenAPI responses and verify**

```bash
cargo test -p pumphound-core-api -- venue hyperliquid
just verify
```

- [ ] **Step 5: Commit**

```bash
git add services/core-api schemas/openapi/pumphound-v1.yaml
git commit -m "feat: expose multi-market research APIs"
```

---

### Task 11: Build the Hyperliquid terminal workspace

**Files:**
- Create: `apps/desktop/src/features/hyperliquid/HyperliquidPage.tsx`
- Create: `apps/desktop/src/features/hyperliquid/OverviewPanel.tsx`
- Create: `apps/desktop/src/features/hyperliquid/NewMarketsTable.tsx`
- Create: `apps/desktop/src/features/hyperliquid/MarketDetail.tsx`
- Create: `apps/desktop/src/features/hyperliquid/FundingPanel.tsx`
- Create: `apps/desktop/src/features/hyperliquid/BasisPanel.tsx`
- Create: `apps/desktop/src/features/hyperliquid/LiquidationsPanel.tsx`
- Create: `apps/desktop/src/features/hyperliquid/ResearchPanel.tsx`
- Create: `apps/desktop/src/features/hyperliquid/PaperPanel.tsx`
- Modify: `apps/desktop/src/app/router.tsx`
- Modify: `apps/desktop/src/app/LeftNav.tsx`
- Modify: `apps/desktop/src/lib/api.ts`
- Modify: `apps/desktop/src/lib/types.ts`
- Test: add/update `apps/desktop/e2e/` coverage for navigation and unknown/degraded states.

**Interfaces:**
- Consumes Task 10 APIs.
- Produces dense research UI for Overview/New Markets/HIP-3/Funding/Basis/Liquidations/Research/Paper.

- [ ] **Step 1: Add failing navigation/data-state e2e coverage**

Verify Hyperliquid route is reachable and explicitly renders `UNKNOWN`/`STALE`/`INSUFFICIENT SAMPLE` rather than zero when fixtures omit fields.

- [ ] **Step 2: Implement API types and page shell**

Use existing safe parsing/query patterns. Do not create a second design system.

- [ ] **Step 3: Implement data-dense panels**

New Markets table should support market, age, type, builder/deployer, price, spread, depth, volume, OI, funding, basis availability, lifecycle, model rank, and paper state. Market detail should show formation, OI, funding, basis, liquidations, model outputs, execution costs, trace, and paper state when data exists.

- [ ] **Step 4: Typecheck and e2e**

```bash
pnpm --filter @pumphound/desktop exec tsc --noEmit
pnpm --filter @pumphound/desktop exec playwright test --if-present
```

If the repo uses a different existing e2e command, use that exact configured command after inspecting package/workspace scripts; do not invent a new framework.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop
git commit -m "feat: add Hyperliquid research workspace"
```

---

### Task 12: Add Ostium read-only protocol truth and quote-surface recording

**Files:**
- Create: `crates/venue-ostium/Cargo.toml`
- Create: `crates/venue-ostium/src/lib.rs`
- Create: `crates/venue-ostium/src/client.rs`
- Create: `crates/venue-ostium/src/types.rs`
- Create: `crates/venue-ostium/src/normalize.rs`
- Create: `services/market-collector/src/ostium.rs`
- Create: `db/postgres/023_ostium_observation.sql`
- Create: `apps/desktop/src/features/ostium/OstiumPage.tsx`
- Create supporting focused panels under `apps/desktop/src/features/ostium/`
- Modify: `Cargo.toml`, `services/market-collector/src/main.rs`, core API Ostium routes, router/nav/api/types.

**Interfaces:**
- Produces read-only market/config/OI/skew/cap/funding/rollover/liquidation/quote-surface observations and UI.

- [ ] **Step 1: Verify current live protocol/API semantics before coding assumptions**

From official Ostium contracts/docs/SDK, verify current live pair configuration for funding and `isNegativeRolloverAllowed`, and identify the current read-only endpoints/methods for simulated slippage/orderbook/dynamic spread. Record source/version in code/docs. Do not assume every pair has active funding merely because the contract supports it.

- [ ] **Step 2: Write fixture tests**

Cover funding and rollover as separate fields, negative rollover allowed vs floored, OI skew/cap math, and stale simulated quote handling.

- [ ] **Step 3: Implement adapter and from-now recorder**

Backfill onchain/config history only where practical without a huge uncontrolled RPC crawl. Continuously record the non-reconstructable quote surface at bounded size buckets and measured cadence.

- [ ] **Step 4: Implement read-only UI**

Markets, Positioning, Structural Yield, Liquidations, Execution Surface, Data Health. No strategy, ML, or paper alpha claim.

- [ ] **Step 5: Verify and commit**

```bash
cargo test -p pumphound-venue-ostium
pnpm --filter @pumphound/desktop exec tsc --noEmit
just verify

git add Cargo.toml Cargo.lock crates/venue-ostium services/market-collector db/postgres/023_ostium_observation.sql services/core-api apps/desktop
git commit -m "feat: add Ostium read-only research workspace"
```

---

### Task 13: Add Polymarket read-only evidence corpus and relationship truth

**Files:**
- Create: `crates/venue-polymarket/Cargo.toml`
- Create: `crates/venue-polymarket/src/lib.rs`
- Create: `crates/venue-polymarket/src/client.rs`
- Create: `crates/venue-polymarket/src/types.rs`
- Create: `crates/venue-polymarket/src/normalize.rs`
- Create: `crates/venue-polymarket/src/relationships.rs`
- Create: `services/market-collector/src/polymarket.rs`
- Create: `db/postgres/024_prediction_markets.sql`
- Create: `apps/desktop/src/features/prediction/PredictionPage.tsx`
- Create supporting focused panels under `apps/desktop/src/features/prediction/`
- Modify: `Cargo.toml`, `services/market-collector/src/main.rs`, prediction API routes, router/nav/api/types.

**Interfaces:**
- Produces live/upcoming/resolved market/event/CLOB/resolution records and relationship records with `PROVEN_RELATION`, `LIKELY_RELATION`, `UNVERIFIED_RELATION` provenance.

- [ ] **Step 1: Write parser and relationship tests from public fixtures**

Cover YES/NO price preservation, event/market hierarchy, exact resolution rule/source storage, orderbook depth, and the rule that similar titles cannot create `PROVEN_RELATION`.

- [ ] **Step 2: Implement public market/CLOB ingestion**

Persist live/upcoming/resolved markets, bids/asks/trades/depth, liquidity/volume, expiry/resolution timing, rules/source/deadlines, category, parent event, and lifecycle.

- [ ] **Step 3: Implement conservative relationship engine**

Deterministic evidence may create/promote proven relations. Heuristic/LLM candidates remain likely/unverified with provenance; do not wire an LLM promotion path to `PROVEN_RELATION`.

- [ ] **Step 4: Implement UI**

Markets, Events, Orderbooks, Relationships, Resolution, Data Health. No autonomous trading or market making.

- [ ] **Step 5: Verify and commit**

```bash
cargo test -p pumphound-venue-polymarket
pnpm --filter @pumphound/desktop exec tsc --noEmit
just verify

git add Cargo.toml Cargo.lock crates/venue-polymarket services/market-collector db/postgres/024_prediction_markets.sql services/core-api apps/desktop
git commit -m "feat: add Polymarket evidence workspace"
```

---

### Task 14: Add global venue, storage, and research-health surfaces

**Files:**
- Create: `apps/desktop/src/features/system/VenueHealthPanel.tsx`
- Create: `apps/desktop/src/features/system/StorageGrowthPanel.tsx`
- Modify: `apps/desktop/src/features/home/HomePage.tsx`
- Modify: `apps/desktop/src/features/system/SystemPage.tsx`
- Modify: `services/core-api/src/venue_health.rs`
- Modify: `services/core-api/src/routes.rs` only for route wiring.

**Interfaces:**
- Produces a single truthful view of Pump/Hyperliquid/Ostium/Polymarket health, observation freshness, market counts, data growth, model/experiment/paper state.

- [ ] **Step 1: Write data-state tests**

Mixed venue states must remain mixed: e.g. Helius degraded + Hyperliquid healthy + Ostium stale + Polymarket healthy. Global status may summarize but must preserve per-venue truth.

- [ ] **Step 2: Implement storage accounting**

Return measured current bytes and recent growth when available. Compute 30d/90d extrapolations from measured growth and tag them `estimated`; do not present estimates as measured facts.

- [ ] **Step 3: Implement Home/System panels**

Keep the existing Pump research summary. Add venue cards/rows and links rather than replacing the current Pump truth surfaces.

- [ ] **Step 4: Verify and commit**

```bash
cargo test -p pumphound-core-api -- venue
pnpm --filter @pumphound/desktop exec tsc --noEmit
just verify

git add services/core-api apps/desktop
git commit -m "feat: add multi-market health and storage cockpit"
```

---

### Task 15: Operations, API-key documentation, bounded live verification, and final report

**Files:**
- Modify: `.env.example`
- Modify: `docs/API_KEYS.md`
- Modify: `docs/OPERATIONS.md`
- Modify: `docs/KNOWN_LIMITATIONS.md`
- Modify: `justfile`
- Create: `artifacts/verification/multi-market-v1/operator-report.md`

**Interfaces:**
- Produces reproducible local run/verify commands and the required evidence report.

- [ ] **Step 1: Document only keys that code actually reads**

Public/read-only defaults should work where possible. Optional `EXA_API_KEY` may enable slow evidence enrichment later; do not block collection on it. Never copy current secret values into docs or logs.

- [ ] **Step 2: Add safe convenience targets**

Provide operator commands for starting/verifying the non-Solana collector and querying venue health. Do not make `just stop` unexpectedly kill the Pump history backfill unless that behavior is already explicitly intended/documented.

- [ ] **Step 3: Run the full deterministic suite**

```bash
just verify
just execution-twin-test
cargo test -p pumphound-hyperliquid-twin
cargo test -p pumphound-hyperliquid-research
uv run pytest tests/python/test_hyperliquid_dataset.py tests/python/test_hyperliquid_walk_forward.py tests/python/test_hyperliquid_ranking.py -q
pnpm --filter @pumphound/desktop exec tsc --noEmit
```

- [ ] **Step 4: Run bounded live read-only smoke verification**

With Pump processes left alone, run the market collector long enough to prove observations are arriving and storage grows at a measurable rate. Query the API/UI for each venue. Do not run unbounded historical crawls.

- [ ] **Step 5: Re-check Pump regression and history-job status**

Confirm collector/EXEC-SELECT remain healthy and record the history job's current completion/ETA. If it finished naturally, only begin the already-approved import/mint-backfill sequence when Helius usage is no longer competing; otherwise leave it for the next operator action.

- [ ] **Step 6: Write the operator report**

The report must contain:

```text
BRANCH / COMMITS
WHAT ACTUALLY SHIPPED
PUMP STATUS: collector, backfill, EXEC-SELECT, regressions
HYPERLIQUID: markets, observations, history depth, age snapshots, labels, twin fidelity, paper results, model state, limitations
OSTIUM: markets, funding config, rollover, OI/skew/caps, quote-surface samples, liquidations, limitations
POLYMARKET: markets/events/orderbooks, resolution records, relationships by provenance, limitations
STORAGE: current usage, measured growth/day, 30d estimate, 90d estimate
API / KEY REQUIREMENTS
EXACT TEST COMMANDS + RESULTS
UI ROUTES + SCREENSHOT/ARTIFACT PATHS
MEASURED VS ESTIMATED VS UNKNOWN
UNSUPPORTED / INSUFFICIENT-SAMPLE FEATURES
NEXT THREE HIGHEST-VALUE ACTIONS
```

No marketing claims and no statement of proven trading edge unless the prospective post-cost evidence actually satisfies an explicit promotion gate.

- [ ] **Step 7: Final verification commit**

```bash
git add .env.example docs justfile artifacts/verification/multi-market-v1/operator-report.md
git commit -m "docs: verify Hound multi-market V1"
git status --short --branch
```

The final status should contain no unexplained generated files or destructive cleanup.

## Execution priority if the long run cannot finish every task

Do not leave four half-built venues. Preserve coherent gates in this order:

1. Pump safety baseline.
2. Venue core + storage + isolated collector.
3. Hyperliquid live data and durable history.
4. Hyperliquid lifecycle + Execution Twin + deterministic `HIP-NEW-MARKET-V1` books.
5. Hyperliquid UI and paper research truth.
6. Ostium read-only live recorder/UI.
7. Polymarket read-only corpus/UI.
8. Hyperliquid ML only after valid labels exist.
9. Global polish/optional evidence enrichment.

If time/context runs short, stop at the end of a passing committed gate and report exactly what remains rather than scaffolding unverified placeholders.
