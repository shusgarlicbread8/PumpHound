# Features, Labels and Models

## Objective

The model layer estimates path-dependent outcomes and calibrated uncertainty. It does not emit an unqualified `BUY`. Its outputs are immutable inputs to a separately versioned paper-trading policy.

Initial models are tabular and interpretable: logistic regression, XGBoost and LightGBM. Neural temporal or graph models enter only after the event store, labels and boosted-tree baselines are trustworthy. The exact initial names, units, formulas and label thresholds are defined in [the feature/label dictionary](10-feature-and-label-dictionary.md).

## 1. Observation frames

Create feature rows at deterministic cutoffs measured from the decoded creation event:

```text
5s, 15s, 30s, 60s, 120s, 300s
```

If source latency prevents a truthful 5-second row, mark it unavailable; never backfill it with facts observed later. Every row carries:

- launch/mint/market identity;
- cutoff slot and event time;
- maximum source observation time;
- commitment/canonicality state;
- feature schema version;
- missingness mask and data-quality score;
- quote mint and normalization context.

Train distinct horizon models or include observation-window identity explicitly. Do not pretend a five-minute model is a launch-time model.

## 2. Feature families

### 2.1 Launch and market state

- launchpad/program/instruction version;
- initial reserves, supply, quote asset and curve parameters;
- curve progress and estimated amount remaining to completion;
- executable buy/sell quotes at standard notional sizes;
- market capitalization and liquidity estimates with formula version;
- migration status and pool age;
- metadata availability, mutability indicators and duplicate fingerprints;
- decoded protocol flags such as Mayhem/cashback when applicable.

### 2.2 Flow and microstructure

- buys, sells, unique buyers/sellers, active wallets;
- quote volume, net flow, buy/sell imbalance;
- buyer arrival rate and its acceleration;
- inter-arrival-time quantiles and burstiness;
- order-size quantiles, concentration and entropy;
- repeat-buyer ratio, buyer retention and wallet churn;
- sell acceleration and largest seller fraction;
- executable price returns and realized volatility;
- maximum adverse/favourable path so far;
- price impact at configured notionals;
- creator/cluster flow share;
- transaction failure rate and observed ingestion lag.

### 2.3 Holder and concentration state

- top 1/5/10 non-program holder shares;
- Herfindahl-Hirschman and Gini measures;
- holder concentration after collapsing high-confidence clusters;
- number of independently funded clusters;
- supply held by creator-linked wallets;
- fresh-wallet share and wallet-age distribution;
- funded-immediately-before-launch share;
- retained versus already-sold early-buyer supply.

Program vaults, bonding curves, pools, fee accounts and burned/locked accounts must be identified before holder statistics are calculated.

### 2.4 Wallet graph and historical intelligence

- early-wallet reputation posterior mean and uncertainty;
- creator reputation posterior mean and uncertainty;
- weighted recurring-cohort count;
- largest behavioural cluster share;
- common-funder and creator-link evidence;
- co-buy/co-sell graph density, modularity and centralization;
- cluster historical outcomes at the exact cutoff;
- smart-wallet retention and suspicious-wallet sell pressure;
- funding-source diversity and hub-adjusted diversity;
- graph coverage/missing-history indicators.

### 2.5 Execution and data quality

- estimated all-in entry and exit costs by notional;
- slippage curve and depth;
- priority fee regime estimate;
- quote staleness and source lag;
- processed/confirmed/finalized status;
- reorg/rollback or gap-recovery flags;
- provider degradation state;
- whether an enrichment was available at the cutoff.

Missingness is a feature, but provider failure must not masquerade as favourable market structure.

## 3. Label catalog

Every label has a code implementation, version, horizon and required observation quality.

### 3.1 Graduation and survival

- `graduated_by_1h`, `graduated_by_6h`, `graduated_by_24h`.
- `time_to_graduation_seconds`, right-censored when not observed.
- `active_at_1h`, `active_at_6h`, `active_at_24h` under a versioned activity threshold.

Use decoded completion/migration events as the canonical graduation signal. Do not infer graduation only from a price or volume threshold.

### 3.2 First-passage outcomes

From the executable entry quote at the decision cutoff, calculate which barrier occurs first after full modeled round-trip cost:

- `up_20_before_down_10`;
- `up_50_before_down_20`;
- `up_100_before_down_30`;
- parameterized `(take_profit, stop_loss, horizon)` labels.

If both barriers appear inside a resolution gap and ordering cannot be established, mark the label ambiguous rather than choosing the profitable path.

### 3.3 Severe-collapse and rug-like outcomes

Use neutral, observable terminology in stored data. `severe_collapse_v1` is true when all are met:

1. the token had a minimum executable depth/volume threshold before peak;
2. the modeled executable exit value falls at least 80% from its prior executable peak within the horizon; and
3. it remains below 40% of that peak for a subsequent 15 minutes. If that persistence window is not fully observed, the label is censored, not assumed positive.

Separate labels capture:

- creator or creator-linked net selling above a threshold;
- liquidity removal or pool drainage where mechanically possible;
- migration failure/anomaly;
- supply concentration followed by coordinated distribution;
- metadata/social disappearance when independently observed.

The UI may summarize a calibrated combination as “rug-like risk,” but the model card must state the operational definition. A legal accusation is never inferred from price collapse alone.

### 3.4 Return path and exit labels

- net return at 30s, 1m, 5m, 15m, 1h, 6h and 24h;
- maximum favourable excursion and maximum adverse excursion;
- time to peak and time to trough;
- exit hazard in the next 30s/1m/5m;
- counterfactual return for each supported deterministic exit policy;
- actual paper-policy exit and reason.

All returns use executable quotes, simulated latency and complete modeled costs—not chart mid-prices.

## 4. Leakage barriers

These are build constraints, not optional test ideas:

- A feature query must include `available_from_slot <= cutoff_slot`.
- Creator, wallet and cluster histories are snapshot-as-of values.
- Later-finalized labels never enter earlier features.
- Splits are chronological and purged around boundaries.
- Creators, cluster families and near-duplicate launches are group-aware so identity leakage is measured.
- Hyperparameter search sees training folds only.
- Calibration uses a later validation slice; the final evaluation slice stays untouched.
- Provider enrichments are replayed with historical availability or excluded from honest latency claims.

## 5. Baselines and model ladder

Build in this order:

1. Always-negative/base-rate classifier.
2. Hand-written graduation and momentum heuristics.
3. Regularized logistic regression.
4. XGBoost.
5. LightGBM.
6. Calibrated ensemble only if it beats the best member out of sample.
7. Survival model for time-to-event.
8. Temporal convolution/sequence model only when event histories justify it.
9. Graph embeddings or GNN only after static graph features demonstrate incremental value.

Do not add reinforcement learning in the initial product. First prove calibrated outcomes plus an explicit policy.

## 6. Training and evaluation protocol

Use expanding-window walk-forward evaluation:

```text
train [start..T1] -> validate [T1..T2] -> test [T2..T3]
train [start..T2] -> validate [T2..T3] -> test [T3..T4]
```

Report by observation window, launchpad, quote asset, liquidity band and market regime:

- PR-AUC and ROC-AUC, with PR-AUC primary for rare outcomes;
- log loss and Brier score;
- calibration curve and expected calibration error;
- precision/recall at top-k and at policy threshold;
- confusion matrix at chosen operating points;
- net expected value, profit factor and drawdown for downstream policy;
- bootstrap confidence intervals grouped by day and cluster family;
- performance after removing the top 1%, top five and top three profitable trades;
- incremental lift of graph features over market-only features.

Accuracy alone is not an acceptance metric.

## 7. Model registry contract

Each promoted model bundle contains:

- native model artifact and optional ONNX export;
- exact feature schema and ordered feature list;
- label definition/version;
- training/validation/test time ranges;
- source and decoder versions;
- hyperparameters and library versions;
- calibration transform;
- metrics and subgroup report;
- SHAP/global explanation sample;
- data-quality requirements;
- content hash and human promotion note.

The live scorer refuses a model when feature schema, label contract or normalization context is incompatible.

## 8. Live scoring outputs

At each cutoff emit calibrated probabilities and distributions such as:

```json
{
  "graduated_by_6h": {"p": 0.78},
  "severe_collapse_1h": {"p": 0.11},
  "up_50_before_down_20_15m": {"p": 0.61},
  "exit_hazard_60s": {"p": 0.18},
  "net_return_15m": {"p10": -0.23, "p50": 0.08, "p90": 0.71},
  "data_quality": 0.93
}
```

Also emit top supporting and opposing feature contributions. An explanation generated after the prediction is linked to, but never modifies, the stored prediction.

## 9. Policy boundary

The deterministic paper policy consumes model outputs, costs, risk state and explicit thresholds:

```text
paper_enter only if
  data_quality >= minimum
  AND collapse_probability <= maximum
  AND expected_net_edge >= minimum
  AND independent_cluster_count >= minimum
  AND estimated_round_trip_cost <= maximum
  AND all portfolio risk gates pass
```

Model version and policy version are stored separately on every decision. This makes it possible to replay one model with several policies without retraining or rewriting history.
