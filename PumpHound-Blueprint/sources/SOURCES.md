# Primary Sources and Research Notes

Research cutoff: **7 September 2026**. Protocols, APIs, prices, entitlements and model catalogs can change. The implementation must pin an actual commit/version and inspect actual payloads; this document is not a substitute for a source lock.

The requirements in this pack are engineering choices. Research findings motivate experiments, not promised investment returns or proven identification of wallet ownership.

## 1. Protocol and data sources used for the design

| Source | What it supports | Build implication |
|---|---|---|
| [Pump official public repository](https://github.com/pump-fun/pump-public-docs) | program documentation, IDLs, updated interfaces | vendor official IDLs and track commit/hash |
| [Pump program documentation](https://github.com/pump-fun/pump-public-docs/blob/main/docs/PUMP_PROGRAM_README.md) | curve accounts/lifecycle | reconstruct state from decoded program data |
| [PumpSwap documentation](https://github.com/pump-fun/pump-public-docs/blob/main/docs/PUMP_SWAP_README.md) | pool state and effective quote reserves | include virtual quote reserves in pool math |
| [Pump IDL](https://github.com/pump-fun/pump-public-docs/blob/main/idl/pump.json) | exact instruction/event/account schema | never invent event layouts from log text |
| [PumpSwap IDL](https://github.com/pump-fun/pump-public-docs/blob/main/idl/pump_amm.json) | swap/pool/fee event fields | decode exact pinned version |
| [Pump fee IDL](https://github.com/pump-fun/pump-public-docs/blob/main/idl/pump_fees.json) | fee program interface | version fee configuration by slot |
| [Pump fees](https://pump.fun/docs/fees) | current published fees and variability | use dated/on-chain schedules, not a universal constant |
| [Solana RPC documentation](https://solana.com/docs/rpc) | transaction/account/history/commitment methods | canonical raw evidence and reconciliation |
| [Solana WebSocket methods](https://solana.com/docs/rpc/websocket) | log/account/slot subscriptions | development fallback and health tracking |
| [Solana logsSubscribe](https://solana.com/docs/rpc/websocket/logssubscribe) | subscription filter/response semantics | per-program log subscription; fetch full transactions |
| [Solana transaction structure](https://solana.com/docs/core/transactions) | signatures, accounts and instructions | transaction atomicity and routing context |
| [Yellowstone gRPC](https://github.com/rpcpool/yellowstone-grpc) | Geyser-compatible protobuf/source implementation | typed streaming adapter |
| [Helius LaserStream](https://www.helius.dev/docs/laserstream) | gRPC stream/replay/commitment capabilities | primary entitled stream, still idempotent/reconciled |
| [Helius historical data](https://www.helius.dev/docs/rpc/historical-data) | archival/history access | bounded resumable wallet/program backfill |
| [Helius pricing](https://www.helius.dev/pricing) | subscription/credit/stream entitlements | account-specific procurement and usage limits |
| [Birdeye WebSocket docs](https://docs.birdeye.so/docs/websocket) | optional realtime market/wallet feeds | enrichment only, subject to plan access |
| [Birdeye data onboarding](https://data.birdeye.so/docs/getting-started) | API onboarding and current access path | separate entitlement checks |
| [Jupiter Swap API overview](https://developers.jup.ag/docs/swap/index) | unified `/swap/v2`, API key, build versus execution paths | quote-only `/build`; never `/execute` or submit |
| [DexScreener API reference](https://docs.dexscreener.com/api/reference) | pair/token endpoints and rate limits | cached cross-check, not canonical launch capture |
| [Pyth documentation](https://docs.pyth.network/) | price-feed/Hermes interfaces | versioned, timestamped reference valuation |
| [Jito low-latency documentation](https://docs.jito.wtf/lowlatencytxnsend/) | transaction/bundle/fee/region concepts | future execution research only; no sender in v1 |

### Current-interface details that matter

- The Pump repository documents updated quote-mint-aware trade interfaces and PumpSwap virtual quote reserves. Support is gated by the actual on-chain configuration and pinned decoder; a dated announcement does not prove every market currently supports every quote asset.
- Jupiter's current overview distinguishes read/build requests from signed execution/landing. PumpHound uses only the former, with the required server-side API key.
- Helius mainnet low-latency streaming entitlement must be checked separately from ordinary RPC or devnet access. A key alone is not proof of entitlement.
- Processed data can be provisional. PumpHound's rollback/idempotency design remains necessary even when a provider advertises stronger delivery guarantees.

## 2. Research informing experiments

### Persistent cohorts

[Coordinated Sniper Cohorts on Pump.fun, arXiv:2607.02795v3](https://arxiv.org/abs/2607.02795v3) reports persistent early-buyer cohorts across a multi-launch sample. The revised analysis reduces a much larger naive association after accounting for contamination and matching; its placebo analysis is itself presented as biased. Use it to motivate recurrent-cohort features and null-model checks, not to claim common ownership or a proven causal/profitable signal.

### Early collapse/rug-like prediction

[Catching the Rug, arXiv:2608.20271](https://arxiv.org/abs/2608.20271) studies early Solana memecoin classification with tabular ML and cross-platform generalization. It motivates a five-minute boosted-tree baseline, explicit label definitions and domain-shift evaluation. It does not establish PumpHound's future performance, five-second performance or net trading profitability.

### Heavy-tail and paper-trading fragility

[Hour-Aware Adaptive Risk Management, arXiv:2606.08232v3](https://arxiv.org/abs/2606.08232v3) reports a small 190-trade paper sample whose profitability changes when its top three trades are removed. Its time-of-day findings are non-confirmatory. It motivates preserving rejected candidates, distinguishing sample statistics from bankroll returns, and showing outlier/cost sensitivity.

These are preprints. Reproduce available code/data where licensed, evaluate later revisions, and run independent out-of-sample experiments. Do not import the earlier conversation's subjective “chance of success” percentages, startup valuations or return examples as requirements or forecasts.

## 3. Official implementation and license anchors

The following are starting documentation/repository anchors for selected dependencies, not a claim that every latest release has been compatibility-tested together:

| Area | Primary anchors |
|---|---|
| Desktop | [Tauri](https://v2.tauri.app/), [React](https://react.dev/), [Vite](https://vite.dev/), [TypeScript](https://www.typescriptlang.org/docs/) |
| UI state/components | [TanStack](https://tanstack.com/), [Zustand](https://github.com/pmndrs/zustand), [Radix](https://www.radix-ui.com/), [Tailwind](https://tailwindcss.com/docs) |
| Financial charting | [Lightweight Charts repository and notices](https://github.com/tradingview/lightweight-charts) |
| Graph UI | [Sigma.js](https://www.sigmajs.org/), [Graphology](https://graphology.github.io/) |
| Rust services | [Tokio](https://tokio.rs/), [Axum](https://github.com/tokio-rs/axum), [tonic](https://github.com/hyperium/tonic), [serde](https://serde.rs/) |
| Rust storage/bus | [sqlx](https://github.com/launchbadge/sqlx), [official ClickHouse Rust client](https://github.com/ClickHouse/clickhouse-rs), [NATS Rust](https://github.com/nats-io/nats.rs) |
| Stores | [PostgreSQL](https://www.postgresql.org/docs/), [ClickHouse](https://clickhouse.com/docs), [NATS JetStream](https://docs.nats.io/nats-concepts/jetstream) |
| Python workflow | [uv](https://docs.astral.sh/uv/), [Polars](https://docs.pola.rs/), [DuckDB](https://duckdb.org/docs/), [Arrow/PyArrow](https://arrow.apache.org/docs/python/) |
| Models | [XGBoost](https://xgboost.readthedocs.io/), [LightGBM](https://lightgbm.readthedocs.io/), [scikit-learn](https://scikit-learn.org/stable/) |
| Model experiments | [Optuna](https://optuna.org/), [MLflow](https://mlflow.org/docs/latest/), [SHAP](https://shap.readthedocs.io/) |
| Graph algorithms | [python-igraph](https://github.com/igraph/python-igraph), [leidenalg](https://github.com/vtraag/leidenalg) |
| Later graph ML | [PyTorch](https://pytorch.org/), [PyTorch Geometric](https://pytorch-geometric.readthedocs.io/) |
| Local LLM runtime | [llama.cpp](https://github.com/ggml-org/llama.cpp), [Ollama](https://docs.ollama.com/) |
| Starting local model | [Official Qwen3-14B model card](https://huggingface.co/Qwen/Qwen3-14B) |
| Optional cloud LLM | [SiliconFlow documentation](https://docs.siliconflow.com/en/) |
| Optional training host | [Vast.ai documentation](https://docs.vast.ai/) |
| Operations | [OpenTelemetry](https://opentelemetry.io/docs/), [Prometheus](https://prometheus.io/docs/introduction/overview/), [Docker Compose](https://docs.docker.com/compose/) |
| Final UI verification | [Playwright](https://playwright.dev/docs/intro) |

## 4. Reference-only finance projects

- [Microsoft Qlib](https://github.com/microsoft/qlib)
- [FinRL](https://github.com/AI4Finance-Foundation/FinRL)
- [FinGPT](https://github.com/AI4Finance-Foundation/FinGPT)
- [FinRobot](https://github.com/AI4Finance-Foundation/FinRobot)

These are not required runtime dependencies. Their broader architecture does not replace a protocol-accurate Solana event simulator.

## 5. Implementation source-lock requirements

Record for each external protocol/model/dataset artifact:

```text
source URL
repository commit or release/model revision
retrieved-at UTC
content SHA-256
license/data-use conditions
supported network/program IDs and schema version
which decoder/model/feature version consumes it
```

If a primary interface differs from this pack, preserve raw capture, freeze affected derived quotes/policies, document the actual difference and update the adapter contract. Do not silently guess from stale documentation.

