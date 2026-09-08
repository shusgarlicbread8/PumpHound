# Wallet Graph and Intent Inference

## Purpose

PumpHound's durable advantage is not another token screener. It is a point-in-time graph that can answer:

- Which wallets repeatedly appear together across launches?
- Which apparently independent buyers share funding or coordinated timing?
- What did the system know about this creator, wallet, or cluster at the decision time?
- Which observed facts support a risk or opportunity hypothesis?
- How did the inferred structure change after later transactions arrived?

The graph stores facts separately from hypotheses. A transfer is a fact. “These wallets are controlled by one actor” is a versioned hypothesis with evidence and uncertainty.

## 1. Graph layers

### 1.1 Immutable factual layer

Node types:

| Node | Identity | Examples of attributes |
|---|---|---|
| `Wallet` | Solana public key | first/last seen, program owner, label status |
| `Token` | mint public key | decimals, metadata hash, quote mint, creation slot |
| `Launch` | launch program + mint + creation signature | launchpad, creator, initial parameters |
| `Market` | bonding curve or pool account | venue, base mint, quote mint, migration state |
| `Transaction` | signature | slot, index, status, fee payer |
| `FundingSource` | wallet or labelled source | exchange, bridge, distributor, unknown |
| `IdentityLabel` | versioned label record | source, confidence, valid-time interval |

Factual edge types:

| Edge | From → to | Required evidence |
|---|---|---|
| `CREATED` | wallet → launch | decoded create instruction/event |
| `ISSUED` | launch → token | decoded creation event |
| `TRADED_ON` | token → market | decoded market creation/migration |
| `BOUGHT` / `SOLD` | wallet → token | decoded successful trade |
| `TRANSFERRED_TO` | wallet → wallet | successful native/SPL transfer |
| `FUNDED_BY` | wallet → funding source | earliest material inbound transfer under a versioned rule |
| `MIGRATED_TO` | launch → market | decoded completion/migration event |
| `PAID_FEE_TO` | transaction/wallet → wallet | decoded protocol/creator fee |
| `HELD_AT` | wallet → token | point-in-time balance snapshot, not a timeless edge |

Every fact stores `event_id`, signature, slot, intra-slot ordering, observed time, source, canonicality, decoder version, and raw-record pointer. Never overwrite or silently merge facts.

### 1.2 Derived relationship layer

Derived edges are recomputable products and must include `algorithm_version`, `feature_window`, `computed_at`, `valid_from_slot`, `valid_to_slot`, score, and supporting fact IDs.

- `CO_BOUGHT`: repeated entry into the same launches within a bounded time window.
- `CO_SOLD`: repeated exit within a bounded time window.
- `SHARES_FUNDER`: same non-hub upstream funder within the configured crawl depth.
- `TEMPORALLY_SIMILAR`: statistically unusual timing similarity.
- `SIZE_SIMILAR`: similar quote-normalized order fractions across repeated launches.
- `STRATEGY_SIMILAR`: similar launch selection, holding time, and exit pattern.
- `CREATOR_LINKED`: evidence connects a buyer or cluster to the launch creator.
- `CLUSTER_MEMBER`: membership in a versioned clustering run.

Do not create a relationship from one coincidence. Persist the underlying counts, expected counts, base rates, and confidence.

## 2. Identity safety rules

1. A cluster is a behavioural cohort, not proof of common ownership.
2. Do not expose a real-world identity unless the label comes from an allowed, cited source.
3. Do not infer identity from names embedded in untrusted token metadata.
4. Never destructively merge wallet nodes. A cluster membership record may change without changing wallet identity.
5. Exchange, bridge, launchpad, router, fee and high-degree service wallets are hubs. Down-weight or exclude them from common-funder evidence.
6. Show “unknown” when history is incomplete. Missing history is not benign history.
7. Historical features must be computed only from facts available before the observation slot.

## 3. Funding graph

For a newly relevant wallet, run a bounded, cached ancestry search:

1. Read the earliest material inbound native/SPL transfers before its first relevant buy.
2. Ignore rent dust, fee rebates, obvious protocol flows and transfers below the configured materiality threshold.
3. Traverse at most three hops by default and stop at known hubs, high-degree nodes, age limits or an evidence budget.
4. Retain all plausible funders when no single source dominates.
5. Record path amounts, tokens, slots and time gaps.

Materiality is quote-aware. Express each transfer in native atomic units and, when available, a point-in-time USD value. Never compare raw token integers across decimals or quote assets.

Funding-link strength should combine:

- share of the destination wallet's initial capital;
- time between funding and first relevant trade;
- path depth;
- source-wallet hub penalty;
- recurrence across related wallets;
- whether funds split from one source shortly before a launch.

Use an inverse-degree penalty similar to inverse document frequency so that a shared CEX hot wallet contributes almost no identity evidence.

## 4. Coordination graph

For each launch, build early-buyer observations at 5, 15, 30, 60 and 300 seconds from the creation event. A wallet pair accumulates evidence when it repeatedly:

- buys the same token inside a narrow time band;
- appears at similar rank among early buyers;
- spends similar fractions of its available quote balance;
- receives funding from related sources;
- sells in the same narrow window;
- follows the same creator or creator cluster across launches.

For pair `(i, j)`, keep interpretable components rather than only one opaque score:

```text
coordination_score =
    w1 * normalized_co_buy_recurrence
  + w2 * timing_similarity
  + w3 * order_size_similarity
  + w4 * common_funding_strength
  + w5 * co_sell_recurrence
  + w6 * creator_overlap
  - w7 * popularity_expected_overlap
  - w8 * hub_exposure
```

Weights are model/version data, never UI constants. Require a minimum recurrence count before durable edges affect trade policy.

### Clustering

- Use connected components only for obvious, high-confidence funding families.
- Use Leiden community detection for weighted behavioural relationships.
- Run incremental local updates for the live UI and scheduled full recomputation for research.
- Store cluster lineage: created, split, merged, retired. Do not reuse cluster IDs after recomputation.
- Assign a stable `cluster_family_id` when successive versions have sufficient weighted membership overlap.

Validate communities against time-shuffled and launch-shuffled null models. A dense graph created merely by popular tokens must not be treated as coordination.

## 5. Creator intelligence

A creator profile is a point-in-time rollup of attributable launches:

- launches, completions, migrations and severe collapses;
- distribution of time to graduation, peak, collapse and inactivity;
- creator and creator-linked cluster sells;
- early supply concentration and subsequent distribution;
- fee/claim activity where decoded;
- repeat use of metadata, websites, social handles or infrastructure, tagged as weak evidence;
- recurring early-buyer clusters;
- realized executable outcomes for entrants at standard observation windows.

Use Bayesian shrinkage for rates. A creator with one successful launch is not assigned a 100% success probability. Store numerator, denominator, prior and posterior parameters so the UI can explain the estimate.

## 6. Wallet reputation

Compute reputation by observation cutoff and horizon, not as one all-time score. Components include:

- entry quality after modeled costs;
- exit quality and median holding time;
- graduation hit rate;
- severe-collapse exposure;
- creator-link and coordination risk;
- sample size and recency;
- realized versus unrealized outcomes;
- strategy-specific performance.

Use recency decay plus Bayesian shrinkage. Keep positive skill and suspicious coordination as separate axes; a profitable manipulator can be both high-skill and high-risk.

## 7. Intent hypotheses

The inference engine emits a distribution over explainable hypotheses, not a verdict:

| Hypothesis | Typical supporting evidence | Important counter-evidence |
|---|---|---|
| organic early buyer | independent funding, diverse history, no recurring cohort | newly funded seconds before launch |
| coordinated sniper cohort | repeat co-entry, tight timing, common funders | same behaviour explained by a public signal |
| creator-controlled demand | creator funding paths, recurring linked buyers, synchronized exits | indirect paths through hubs only |
| distributor/operator | fans funds out to many active buyers | known exchange/bridge behaviour |
| momentum/copy trader | consistently arrives after identifiable leaders | direct creator link |
| accumulator | repeated buys, retention, low immediate sell pressure | transfers to unobserved wallets |
| exit preparation | balance consolidation, approvals/routes, accelerating cluster sells | routine rebalancing history |
| arbitrage/MEV | atomic routes, short holding, venue price discrepancy | persistent directional position |

Each hypothesis record contains:

```json
{
  "subject_type": "wallet_cluster",
  "subject_id": "cluster-family-id",
  "hypothesis": "creator_controlled_demand",
  "probability": 0.72,
  "confidence_band": [0.58, 0.82],
  "as_of_slot": 123,
  "model_version": "intent-2026-09-01",
  "supporting_evidence_ids": ["evt:...", "edge:..."],
  "counter_evidence_ids": ["edge:..."],
  "data_gaps": ["funding history stops at known exchange"]
}
```

The GUI must render supporting evidence, counter-evidence and gaps beside every intent claim. The probability example is a schema illustration for a future calibrated intent model, not a value to display from initial heuristics. Until calibration exists, store/render an `evidence_score` and `calibrated=false`, not an ownership probability.

## 8. Live computation budgets

- Update immediate per-launch statistics with every ordered trade.
- Update local pair relationships within 250 ms p95 after the event is committed to the live state.
- Defer expensive historical funding crawls; surface their status as pending.
- Cache wallet history and invalidate by new slot ranges.
- Limit live graph rendering to a ranked neighbourhood (default 300 nodes / 800 edges); query deeper layers on demand.
- Never delay market ingestion while graph enrichment is slow.

## 9. Graph acceptance scenarios

1. Eight wallets funded through a known exchange must not become a high-confidence ownership cluster solely because of that source.
2. Five wallets funded by one unlabelled wallet, buying three of the same launches within two seconds and co-selling twice, must produce an evidence-rich coordination hypothesis.
3. Replaying from an earlier slot must exclude later wallet reputation and cluster membership.
4. A cluster split after recomputation must retain lineage and must not rewrite old predictions.
5. Clicking an edge in the GUI must lead to its transactions/signatures and the algorithm version that created it.
