# Tools, Accounts, Costs and Expansion Boundaries

## 1. What the coding agent should install and use

This is a selected stack, not a request to install every quantitative-finance repository mentioned in the earlier discussion. Pin compatible minor versions when the corresponding product slice is built; the major versions below are the design targets.

| Area | Selected tools | Role |
|---|---|---|
| Desktop | Tauri 2, React 19, TypeScript, Vite | packaged Mac terminal |
| UI behaviour | TanStack Router/Query/Table, Zustand, Radix, Tailwind | navigation, data cache, virtualized tables, accessible controls |
| Financial charts | TradingView Lightweight Charts | candles, volume and equity curves |
| Graph rendering | Sigma.js, Graphology, worker-based layout | interactive wallet network |
| Rust runtime | Tokio, Axum, tonic/prost, serde, async-nats | stream/API/event transport |
| Storage clients | sqlx, official ClickHouse `clickhouse` crate | typed DB access |
| Protocol decoding | official Pump IDLs/SDK references, Borsh-compatible decode | instructions/events/account state |
| Research | Python 3.12, uv, Polars, DuckDB, PyArrow | extraction, joins, Parquet |
| Models | scikit-learn, XGBoost, LightGBM | baselines, probabilities, calibration |
| Experiments | Optuna, MLflow, SHAP | bounded tuning, registry, explanations |
| Graph analysis | python-igraph, leidenalg | sparse projections and communities |
| Databases | PostgreSQL 17, ClickHouse 26.x | transactional control/ledger and analytical tape |
| Bus | NATS JetStream 2.x | durable bounded live transport |
| Local AI | llama.cpp or Ollama; Qwen3 family | optional evidence summaries |
| Operations | Docker Compose, OpenTelemetry, Prometheus | runtime/metrics/recovery |
| Later experiments | PyTorch, PyTorch Geometric | only after tabular graph-feature lift is established |

Use the official Rust client package named `clickhouse`, not the unrelated client package sometimes called `clickhouse-rs`. Keep all dependency names explicit in manifests.

## 2. Account and API checklist

| Provider | Required for v1? | What to obtain | Implementation path and failure behaviour |
|---|---|---|---|
| Solana RPC/WebSocket | Yes, at least one source | read endpoint; public fallback can be keyless | program logs/accounts/transactions; partial/lagged coverage visible |
| Helius | Recommended, not mandatory for first GUI | server-side key; confirm mainnet LaserStream and history entitlements separately | primary stream + bounded history; fallback to configured RPC |
| Birdeye | No | API key + exact REST/WebSocket entitlements | independent enrichment/cross-check; omitted if unavailable |
| Jupiter | No | key from developer portal | read-only `/swap/v2/build` quote/instruction response; never execute/submit |
| Pyth Hermes | No | current access key/endpoint if required | SOL/USD reference; fallback provider or quote-native values |
| DexScreener | No | public API access under current limits | pair/market cross-check, never launch truth |
| SiliconFlow | No | key and allowed model IDs | optional cloud explanations; local/deterministic fallback |
| Vast.ai | No | account only when GPU experiments are justified | disposable training, no production state/credentials |
| Jito | Not used for submission in v1 | no account needed for the paper product | documentation informs optional execution-cost scenarios only |

No wallet, broker account, exchange deposit or funded mainnet key is required to build and operate v1.

### Configuration names

The application config must recognize these names; keys are secret references, not webview values:

```text
PUMPHOUND_MODE
PUMPHOUND_DATA_DIR
PUMPHOUND_API_BIND
PUMPHOUND_POSTGRES_DSN_REF
PUMPHOUND_CLICKHOUSE_URL
PUMPHOUND_CLICKHOUSE_CREDENTIAL_REF
PUMPHOUND_NATS_URL
PUMPHOUND_NATS_CREDENTIAL_REF
PUMPHOUND_SOLANA_RPC_URL_REF
PUMPHOUND_SOLANA_WS_URL_REF
PUMPHOUND_HELIUS_KEY_REF
PUMPHOUND_LASERSTREAM_ENDPOINT
PUMPHOUND_BIRDEYE_KEY_REF
PUMPHOUND_JUPITER_KEY_REF
PUMPHOUND_PYTH_KEY_REF
PUMPHOUND_AI_BASE_URL
PUMPHOUND_AI_MODEL
PUMPHOUND_AI_KEY_REF
PUMPHOUND_MONTHLY_PROVIDER_BUDGET_USD
```

`*_REF` resolves through the OS credential store locally or server-injected secrets remotely. The start script must reject missing required values with a named error and show the applicable fallback. No prefilled production credentials belong in `.env.example`.

## 3. Costs and procurement decisions

As checked on 7 September 2026, Helius lists Developer at $49/month, Business at $499/month and Professional at $999/month; its premium LaserStream section states mainnet gRPC is included with Business and higher, and lists data add-ons starting at $400/month. Confirm actual entitlement in the dashboard because plan cards and free/devnet access can be confusing. These are provider prices, not a recommended purchase. [Helius pricing](https://www.helius.dev/pricing)

The first build should use the available endpoint and measured fallback. Do not purchase a high-end stream to make an unfinished GUI appear fast.

For budgeting, use these **planning envelopes**, not vendor quotes:

| Operating profile | Planning envelope | Decision |
|---|---:|---|
| local product development | $0–$100/month data/API spend | RPC/limited history, existing Mac, local AI optional |
| always-on broad research | hundreds to $1,000+ per month | only after measuring stream volume/history cost and retention |
| temporary model training | hard per-job cap set by user | choose GPU/job duration from actual experiment needs |

Never automatically sign up, upgrade, rent or exceed a spending cap. Show estimated credits/bytes and job cost before a large backfill. Price each provider adapter separately.

Store two performance views:

```text
trading net P&L = gross trading result - execution costs
project net result = trading net P&L - data/API/hosting/model costs
```

A hypothetical 5% monthly result on a $1,000 bankroll is $50 before fixed infrastructure cost. That arithmetic is why data procurement matters; it is not a return forecast.

## 4. Local model choice on the Mac

Detect RAM and current free headroom before enabling AI. Start with one model request at a time and an 8k context budget, not the model's maximum context.

| Available machine profile | Starting choice | Operating rule |
|---|---|---|
| limited memory / 16–24 GB unified RAM | smaller Qwen3-class 4B/8B quantized model, or no LLM | preserve ingestion/DB headroom |
| 32–48 GB unified RAM | Qwen3-14B Q4-class quantization | one investigation at a time; measure memory/latency |
| 64 GB+ and spare headroom | compare a larger model only if grounding quality improves | no automatic upgrade by parameter count |

These are engineering starting points, not guaranteed fit/performance claims. Quantization, context cache and concurrent databases affect memory materially. The official Qwen3-14B model card lists Apache-2.0 and local runtime support; verify the exact downloaded model/quantizer artifact, hash and license. [Qwen3-14B model card](https://huggingface.co/Qwen/Qwen3-14B)

A finance-branded model is not automatically better at Solana evidence. Evaluate grounded claim quality using PumpHound evidence packs. Do not fine-tune a language model to replace deterministic prices or supervised risk models.

## 5. License and data-use inventory

The build must create `THIRD_PARTY_NOTICES.md` and `licenses.lock.json`, recording package/version/source/license and distributed components. Preserve notices and attribution required by the exact pinned version.

Special attention:

- Lightweight Charts has attribution/notice requirements in its official repository; render required chart attribution. [Official chart repository](https://github.com/tradingview/lightweight-charts)
- igraph and Leiden components have copyleft licensing; personal use is the current scope. Before distributing a product, review the pinned licenses and packaging obligations; process separation is not an automatic legal exemption. [python-igraph](https://github.com/igraph/python-igraph), [leidenalg](https://github.com/vtraag/leidenalg)
- Model weights, inference software, training datasets and quantized derivatives each have their own licenses.
- Public-chain facts do not grant unlimited rights to redistribute a provider's enriched labels, historical dataset or cached metadata images.
- Do not assume “free API” means unlimited requests or resale rights.

## 6. Earlier finance tools: what to reuse and what not to install

| Tool family | PumpHound decision |
|---|---|
| Qlib | use research/workflow ideas; not the microsecond/event-level AMM simulator |
| FinRL | reference environment patterns only; no RL in initial product |
| FinGPT | reference research only; no required finance-LLM training project |
| FinRobot | reference evidence/role separation, no dependency on its full architecture |
| vectorbt / backtesting.py | optional independent sanity comparisons for aggregate series; never paper-fill source of truth |
| NetworkX | optional small offline inspection; avoid a second live graph representation |
| PyTorch Geometric | later experiment against a proven market+graph tabular baseline |
| Neo4j/Kafka/Kubernetes | unnecessary services for this single-user first product |

## 7. Expansion beyond Pump.fun

The first completed product fully supports the two specified Pump venues. Expansion is capability-based and must not be advertised as implemented before an adapter exists.

| Candidate | Required work before support |
|---|---|
| Raydium pools/LaunchLab | official IDs/IDLs, launch/migration mapping, separate CPMM/CLMM/legacy venue math, fees, fixtures and coverage |
| Meteora launch/AMM products | official IDs/layouts, dynamic-fee/curve/bins or liquidity model, event decode and quote validation |
| other Solana launchpads | same capability checklist; no guessed program IDs from token symbols |
| centralized crypto venues | separate market-data/execution-cost adapter and exchange terms; wallet graph applies only to visible on-chain flows |
| India/UK stocks | separate broker/data adapter and market calendar/corporate-action layer; not a hidden v1 deliverable |

An adapter declares `discover_launches`, `decode_trades`, `reconstruct_state`, `quote_buy`, `quote_sell`, `track_migration`, `history_coverage`, `token_programs`, and `quote_mints`. The policy only operates when required capabilities are true.

Never reuse constant-product quote code for concentrated liquidity, bins or a different bonding curve merely because the UI shows similar prices.

## 8. Future live execution: separate authorization and specification

The architecture preserves a clean route to a future executor, but it is deliberately absent from v1. If explicitly requested after paper research, require:

1. independent out-of-sample and live-shadow evidence, confidence intervals, cost/capacity stress and regime coverage;
2. a separate signed-order schema, key-management threat model and transaction simulator;
3. human-approved hard wallet/notional/day limits and kill switch;
4. quote/route/program/account allowlists, slippage/deadline checks and exact transaction inspection;
5. blockhash expiry, duplicate-send/idempotency, landing/finality and reconciliation design;
6. a small isolated funded wallet, not a seed imported into the analysis desktop;
7. explicit user approval before any real submission.

Paper profitability is evidence for an experiment, not proof of live profitability. Do not implement this future boundary as an undocumented environment flag.

