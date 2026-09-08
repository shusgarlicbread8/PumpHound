# PumpHound Intelligence V1 — locked implementation spec

Evidence-first. Do not improvise this order. Do not enable any `LIVE_PAPER_*` mode until M15 after M10–M14 pass. Do not add live-money execution.

Thesis: persistent wallet/creator/graph intelligence + time-safe features + calibrated models + autonomous **paper** + AI interpretation. Not “an LLM picks a coin.”

SiliconFlow / any LLM sits **beside** the arithmetic. It never sets `action`.

Mode stays `LIVE_WATCH` until M15 explicitly flips it.

## Locked stack (unchanged)

Tauri 2, React 19, Vite, TanStack, Zustand, Tailwind, Radix, Lightweight Charts, Sigma.js. Rust/Tokio/Axum/sqlx/clickhouse/async-nats. Python 3.12/uv/Polars/XGBoost/SHAP. Postgres 17, ClickHouse 26.x, NATS JetStream. No Electron, Next.js, MongoDB, Neo4j, Kafka, K8s, LangChain, vector DBs.

Atomics and slots are decimal strings in JSON. Unknown stays unknown. `MODEL NOT TRAINED` until a registry champion exists with walk-forward evidence. Heuristic values are labelled `HEURISTIC`.

## Sequential gates

| Gate | Name | Stop condition |
|---|---|---|
| M10 | Data truth | Replay of raw archive reproduces event count, IDs, reserves, volumes, graph facts, final hash |
| M11 | Intelligence graph | Wallet/creator profiles + funding queue + cohorts + evidence-backed derived edges served by API |
| M12 | Dataset factory | Temporal snapshots + objective labels + Parquet lake + chronological splits. No random shuffle |
| M13 | Models V1 | Train only on M12 output. No promotion on train metrics. Live UI stays withheld until champion |
| M14 | Replay lab | Event-order replay, protocol quote, versioned fees, latency, rejected fills, candidate outcomes |
| M15 | Autonomous paper | Durable ledger + risk + exit. Then and only then a `LIVE_PAPER_*` mode |
| M16 | Intelligence terminal | Radar/token/wallet/creator/graph show real session intelligence; probabilities withheld if untrained |
| M17 | Cross-launchpad | Capture LaunchLab/Meteora early; simulate one protocol at a time |
| M18 | Investigators | Structured facts in, hypotheses+citations out, no execution authority |
| M19 | Evaluation | Walk-forward, ablation (graph/creator/wallet/LLM), F01–F30 |

## M10 files

- `crates/domain/src/ids.rs` — `economic_event_key(signature, ix, inner, kind)`
- `crates/storage/src/archive.rs` — write `data/archive/raw/date={YYYY-MM-DD}/{signature}.json`
- `crates/storage` — `canonical_event_keys` idempotent insert
- `services/collector/src/main.rs` — archive every raw tx; skip duplicate economic keys
- `services/collector/src/bin/verify_archive.rs` — re-decode archive/fixture, print hash
- `just verify-archive`
- `tests/python/test_archive_replay.py` and/or Rust unit test: fixture decoded twice → identical hash
- `GET /api/v1/system/status` — `archive`, `unique_events`, `decode_rate`, `duplicate_skips`

Raw JSON must retain: signature, slot, tx_index, instruction indexes, block_time, received_at, commitment, program_id, account keys, instruction data, pre/post balances, token balances, logs, source, decoder_version, payload hash.

## M11 files

Postgres `db/postgres/002_intelligence.sql` (apply to running DB; compose init only runs on empty volume):

- `wallet_profiles`, `creator_profiles`, `funding_queue`, `funding_edges`, `wallet_cohorts`, `cohort_members`, `derived_edges`

Derived edges are **not** ownership. Columns: score, evidence_count, first/last observed, algorithm_version, supporting_signatures.

Funding: priority queue only (creator=100, first 5 buyers=100, 6–10=80). Cache Helius `funded-by` / history. If unpaid/unavailable → `status=unknown`, never invent a funder.

P&L fields that lack complete cost basis must be `null` with `pnl_status=INCOMPLETE`.

## M12 files

- `ml/pumphound_ml/extraction/dataset.py` — launches from create **or** first trade per mint; paths join on **mint**
- `ml/configs/dataset-live.yaml` — `events_uri: data/journal/events.jsonl`, lake `data/lake/protocol=pump/date=…`
- Observation windows: 5,10,15,30,60,120,300
- Labels: graduation (migrate/complete only), barrier momentum, collapse paths, censored if horizon unseen
- Chronological splits only (`ml/pumphound_ml/evaluation/splits.py`)
- `just dataset ml/configs/dataset-live.yaml`

Do not train a live champion from a single-session journal.

## M13 files

Existing `ml/pumphound_ml/training/*`, `serving/*`, `registry/*`. Promotion never automatic. `GET /models` and token predictions return `MODEL NOT TRAINED` + `familiarity: withheld` until a registry row with `status=champion` exists.

## M14 files

- `ml/pumphound_ml/replay/runner.py` — arrival = decision_time + inference_ms + network_ms; fill at **arrival** state
- `crates/paper-core/src/fees.rs` — versioned `FeeSchedule` (curve 125 bps split, not a magic 1.25% constant with no version)
- Candidate outcomes for accepted **and** rejected decisions

## M15 files

Existing paper ledger tables. Crash recovery = reload portfolios from Postgres. Autopilot remains off while `PUMPHOUND_MODE=LIVE_WATCH`.

## M16 files

- Radar: creator, clusters, organic buyers, heuristic opportunity; model columns show withheld
- Token: Intelligence tab (familiarity, completeness, SHAP placeholder = withheld)
- Wallet / creator pages: session counts, `P&L INCOMPLETE`, activity table, funding unknown
- Graph: click edge → supporting signature

## M17–M20

Capture stubs and investigator schema only in this pass. No Raydium maths reuse. Brand ≠ protocol.

## Acceptance this slice must prove

1. `just verify-archive` (or equivalent cargo/pytest) passes on `fixtures/chain/sample_logs.json`.
2. Live collector writes archive files and skips duplicate event ids.
3. `GET /launches` includes creator; `GET /wallets/{addr}` and `/creators/{addr}` use observed facts.
4. `just dataset` on the replay fixture writes Parquet + manifest with chronological splits.
5. Desktop TypeScript builds. Radar does not display invented graduation %.
6. Mode remains `LIVE_WATCH`.
