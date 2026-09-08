# Wallet Intelligence and Predictive Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the working live terminal and paper desk into a point-in-time wallet intelligence and calibrated prediction product.

**Architecture:** Factual edges originate from canonical events; Python builds bounded historical projections and versioned cohorts. Shared feature contracts feed offline datasets and live scoring; predictions are persisted before deterministic policies consume them.

**Tech Stack:** PostgreSQL, ClickHouse, Polars, DuckDB, PyArrow, igraph/Leiden, scikit-learn, XGBoost, LightGBM, Optuna, MLflow, Sigma.js/Graphology.

**Spec:** [Graph](../spec/04-wallet-graph-and-intent.md), [features/models](../spec/05-features-labels-and-models.md), [domain](../contracts/01-domain-and-event-contracts.md), [storage](../contracts/02-storage-schema.sql).

## Global Constraints

- Observed facts, derived relationships, model predictions and LLM interpretation remain distinct.
- Every historical feature respects economic and knowledge-time cutoffs.
- Wallet clusters are behavioural hypotheses, not proof of ownership.
- V1 contains no transaction signer or live-money submission path.
- The live terminal and paper desk remain usable throughout this phase.

---

### Task 12 — Historical jobs and factual wallet graph

**Files:** `ml/pumphound_ml/extraction/{history,transfers,coverage}.py`, `ml/pumphound_ml/graph/{facts,identities}.py`, `services/ml-worker/main.py`, `services/core-api/src/routes/{wallets,creators,graph}.rs`, `db/postgres/003_graph.sql`, `pyproject.toml`.

**Consumes:** canonical events and Helius archival/history responses through bounded jobs.

**Produces:** `build_facts(events, knowledge_cutoff) -> GraphFactBatch`, wallet/creator profiles and history coverage.

- [ ] Start the Python worker only now, with a persisted job queue/cursor and cancellable tasks.
- [ ] Convert creation, trade and external transfer events into typed factual nodes/edges; link every edge to evidence.
- [ ] Resolve SPL token-account owners at the relevant slot; do not treat an associated token account as an independent person-wallet.
- [ ] Bootstrap targeted holder snapshots with slot context and buffered account/transfer updates; mark partial history honestly as specified in the protocol data spec.
- [ ] Implement targeted creator/early-buyer history fetch with per-wallet/date/credit budgets, checkpointing and cache.
- [ ] Store coverage boundaries and observed time; label unknown/unavailable history explicitly.
- [ ] Surface profile and history job progress on token/creator/wallet screens.

**Smoke evidence:** a live launch links to its creator and buyers; clicking one wallet shows its captured transactions and exact coverage range.

### Task 13 — Bounded funding ancestry and hub handling

**Files:** `ml/pumphound_ml/graph/{funding,hubs,labels}.py`, `apps/desktop/src/features/graph/{FundingPaths,GraphLegend}.tsx`.

**Consumes:** factual transfers, label registry and point-in-time wallet balances.

**Produces:** `trace_funding(wallet, cutoff, budget) -> FundingPathResult` with evidence, score components and truncation reasons.

- [ ] Traverse up to three hops, 100 relevant transfers per wallet and a 30-day default lookback; expose all three as job configuration.
- [ ] Rank materiality by initial-capital share; exclude known protocol/rent/settlement flows.
- [ ] Down-weight high-degree and known exchange/bridge nodes; do not infer common ownership through hubs alone.
- [ ] Store competing funders and uncertainty when provenance cannot identify a single primary source.
- [ ] Render paths with asset/amount/time-gap labels and explicit stopped-at-hub/budget markers.

Example scoring component:

```python
def hub_weight(distinct_recipients: int) -> float:
    import math
    return 1.0 / (1.0 + math.log1p(max(0, distinct_recipients)))
```

This is a down-weighting feature, not a calibrated ownership probability. A known exchange/bridge path receives no common-control inference solely from this feature.

**Smoke evidence:** one direct funding chain is inspectable; common exchange withdrawals are visibly discounted.

### Task 14 — Recurring cohorts, reputation and intent features

**Files:** `ml/pumphound_ml/graph/{cooccurrence,communities,lineage,reputation,intent}.py`, `ml/configs/graph-v1.yaml`, `services/core-api/src/routes/clusters.rs`.

**Consumes:** early-buy/sell windows, funding evidence, historical launch outcomes and label registry.

**Produces:** versioned derived edges, cluster memberships/lineage, reputation snapshots and evidence-scored intent hypotheses.

- [ ] Build sparse wallet/launch incidence observations; do not allocate a dense all-wallet pair matrix.
- [ ] Require at least three shared launches before a persistent co-buy edge affects the initial policy; retain lower-count evidence for display.
- [ ] Compute popularity-adjusted recurrence, timing/size similarity and co-sell components.
- [ ] Use weighted Leiden for behavioural communities and high-confidence connected components only for appropriate funding families.
- [ ] Persist cluster version/family/parent lineage; preserve prior membership snapshots.
- [ ] Compute Beta-binomial rate estimates and recency-weighted outcome statistics with sample counts. Keep skill and suspiciousness separate.
- [ ] Emit scored hypotheses with counter-evidence; expose a probability only after a calibrated intent model exists.

```python
def posterior_rate(successes: int, failures: int, alpha: float, beta: float) -> float:
    return (successes + alpha) / (successes + failures + alpha + beta)
```

The prior is fit on training-period base rates and versioned. Do not choose a prior using held-out outcomes.

**Smoke evidence:** a recurring cohort appears with three or more source launches; a historical as-of request returns the older membership/reputation state.

### Task 15 — Interactive graph and wallet/creator workbench

**Files:** `apps/desktop/src/features/graph/{GraphPage,GraphCanvas,GraphToolbar,NodeInspector,EdgeInspector,TimelineScrubber,ClusterPanel}.tsx`, `apps/desktop/src/features/wallet/{WalletPage,CreatorPage,ReputationPanel}.tsx`.

**Consumes:** bounded graph/profile APIs and `graph:{subject_id}` deltas.

**Produces:** interactive evidence-first wallet intelligence terminal.

- [ ] Render wallet, token, creator, market and hub types with the UX legend and edge styles.
- [ ] Support expand one hop, hide hubs, minimum evidence, relationship filters, cluster collapse and pin selection.
- [ ] Run graph layout in a web worker; cap initial view at 300 nodes/800 edges and show hidden counts.
- [ ] Add time scrubbing that queries a historical snapshot, rather than recolouring today's graph.
- [ ] Make node/edge inspectors show facts, score components, counter-evidence and exact source transactions.
- [ ] Link wallet/creator histories, reputation distributions and observed trades back to token detail and paper decisions.

**Milestone 4 outcome:** the user can investigate whether apparent buyer diversity survives wallet/funding/cohort analysis, with auditable uncertainty.

### Task 16 — Shared point-in-time feature engine

**Files:** `crates/feature-core/src/{schema,market,holders,graph,quality,clock}.rs`, `ml/pumphound_ml/features/{extract,join,availability}.py`, `schemas/json/feature-schema-v1.json`, `ml/configs/features-v1.yaml`.

**Consumes:** canonical market state, graph/reputation versions, provider observation times.

**Produces:** FeatureVector at 5/15/30/60/120/300 seconds and immutable dependency manifests.

- [ ] Declare every feature's name, units, formula, window, missingness rule and earliest possible availability in the schema.
- [ ] Compute online market features from the shared Rust reducer; offline extraction replays that reducer or consumes its snapshots, rather than independently reimplementing financial math.
- [ ] Join graph/history features by cutoff slot and knowledge time; reject future dependencies.
- [ ] Exclude program vaults and duplicate settlement flows from holder/buyer features.
- [ ] Save each feature vector and dependency IDs before scoring; include source coverage and unknown indicators.
- [ ] Add the feature inspector to the token screen, including units, current values and as-of timestamp.

**Smoke evidence:** compare one online feature snapshot with replay at the same cutoff; adding a later wallet label does not alter the saved earlier row.

### Task 17 — Matured labels and chronological datasets

**Files:** `ml/pumphound_ml/labels/{graduation,barriers,collapse,paths,censoring}.py`, `ml/pumphound_ml/extraction/dataset.py`, `ml/pumphound_ml/evaluation/splits.py`, `ml/configs/dataset-v1.yaml`.

**Consumes:** feature snapshots and later finalized, execution-aware market paths.

**Produces:** Parquet dataset, label/split/coverage manifests and `just dataset <config>`.

- [ ] Implement exact label definitions from the model spec; separate severe collapse from allegations of fraud.
- [ ] Use first-passage ordering and censor ambiguous gaps or incomplete horizons.
- [ ] Generate execution-aware net path labels for fixed notionals and quote currencies; pin fee and latency scenarios.
- [ ] Keep all eligible launches, including dead/illiquid/no-trade outcomes and abstentions; do not construct the dataset from surviving API listings only.
- [ ] Build time-ordered train/validation/calibration/test partitions with purge/embargo and creator/cluster group diagnostics.
- [ ] Write dataset manifest with hashes, counts, class balance, missingness and observation-availability mode.

**Smoke evidence:** one graduating launch, one severe collapse and one censored launch have inspectable labels; the final held-out interval appears in no training input.

### Task 18 — Baselines, XGBoost and LightGBM training

**Files:** `ml/pumphound_ml/training/{baseline,xgboost_model,lightgbm_model,tune,calibrate}.py`, `ml/pumphound_ml/evaluation/{metrics,walk_forward,subgroups}.py`, `ml/configs/train-v1.yaml`.

**Consumes:** pinned dataset/split manifests.

**Produces:** candidate model bundles, calibration maps, evaluation reports and `just train <config>`.

Start from [the supplied training configuration](../contracts/05-training-config.yaml); map early-stopping parameters to the pinned library's supported constructor/callback API and record the resolved configuration.

- [ ] Train base-rate, heuristic and logistic baselines first in the same job pipeline.
- [ ] Fit XGBoost and LightGBM with early stopping and deterministic seeds; use a bounded Optuna budget, initially 30 trials per target/window.
- [ ] Calibrate on a separate later slice; choose sigmoid or isotonic based on validation sample sufficiency, not test performance.
- [ ] Report PR-AUC, log loss, Brier, reliability curves, top-k precision and subgroup performance; include base rates/sample counts.
- [ ] Compare market-only versus market+graph models to quantify actual incremental graph value.
- [ ] Save ordered feature schema, training ranges, dependencies, native artifact, hash and model card in MLflow plus the application registry.

No model can be promoted on its training metrics. If there are too few mature examples, the product remains fully functional with labelled heuristics and displays the required data-count progress.

**Smoke evidence:** a completed reproducible training run with a candidate artifact and held-out report; probabilities are bounded and calibrated status is truthful.

### Task 19 — Registry, live inference and Models screen

**Files:** `ml/pumphound_ml/{registry/store,serving/app,serving/predict}.py`, `services/core-api/src/{prediction,feature_scheduler}.rs`, `apps/desktop/src/features/models/{ModelsPage,ModelCard,CalibrationChart,FeatureImportance}.tsx`.

**Consumes:** FeatureVector and compatible registered model bundle.

**Produces:** persisted PredictionBundle, `/models` APIs and live model UI.

- [ ] Implement internal prediction endpoint with explicit model version, feature order/schema validation and timeout.
- [ ] Save prediction before publication and keep model version/target horizon attached to every visible number.
- [ ] Add candidate → shadow → champion transitions with audit reason and schema compatibility checks.
- [ ] Support atomic in-memory model swap and rollback; queued decisions retain the version they actually used.
- [ ] Render reliability, feature importance, coverage, drift, class balance and walk-forward results.
- [ ] On inference failure, emit abstention or named heuristic fallback; never silently re-use stale predictions.

**Smoke evidence:** a live launch receives a persisted shadow prediction and the UI opens its exact model card; a schema mismatch is rejected.

### Task 20 — Cost-aware model policy and learned exit experiment

**Files:** `crates/paper-core/src/{model_policy,exit_hazard}.rs`, `ml/pumphound_ml/training/survival.py`, `apps/desktop/src/features/paper/{DecisionInspector,PolicyComparison}.tsx`.

**Consumes:** immutable predictions, executable costs, graph gates and paper risk state.

**Produces:** versioned model-based paper policies, shadow comparison and exit-hazard experiment.

- [ ] Implement eligibility and expected-net-edge gates; use calibrated distributions or explicitly labelled conservative approximations.
- [ ] Do not add win probabilities from correlated models or multiply them as independent facts. Fit/calibrate any combined estimator out of sample.
- [ ] Run model policy alongside the heuristic policy on separate portfolios/runs.
- [ ] Train a separate time-to-event/exit-hazard model using right-censored positions and point-in-time held-position features.
- [ ] Keep fixed/time/trailing exit baselines and log every candidate exit reason and precedence.
- [ ] Expose model/policy/version/cost/evidence inputs on each accepted or rejected decision.

**Milestone 5 outcome:** calibrated model outputs can drive live paper decisions while heuristic and learned exit alternatives remain independently inspectable.
