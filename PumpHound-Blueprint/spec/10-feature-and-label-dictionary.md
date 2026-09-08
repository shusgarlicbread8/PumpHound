# Initial Feature and Label Dictionary

This dictionary fixes the first production schema. Add features through versioned schema changes, not opportunistic fields in a model notebook. All calculations are point-in-time and exclude failed/orphaned economic events.

## 1. Common notation

For an observation cutoff `t`, window `W` contains successful, known trades in `(t-W, t]`. `B` and `S` are quote-volume totals for buys and sells. `U_B` is the set of buyer owners, not token accounts. `q_i` is quote amount after decimal normalization; `p_i` is the executable price at a pinned notional. Reserve/ledger arithmetic remains integer; normalized ML features may be finite floating point.

Division by zero produces null plus a missingness indicator unless the dictionary explicitly defines zero. No winsorization or scaling parameter is learned outside the training interval.

## 2. Market and behaviour features

| Feature name | Formula / computation | Windows | Missingness |
|---|---|---|---|
| `age_seconds` | cutoff time − creation time | current | null if creation unknown |
| `buy_count_W`, `sell_count_W` | successful side counts | 5/15/30/60/120/300s | zero only when window coverage complete |
| `buy_quote_W`, `sell_quote_W` | sums of side quote amount | same | null on incomplete economic coverage |
| `buy_sell_imbalance_W` | `(B-S)/(B+S)` | 30/60/300s | null when total volume zero |
| `unique_buyers_W` | distinct trader owner wallets | 15/30/60/300s | null if owner mapping incomplete |
| `buyer_arrival_rate_W` | newly observed buyers in W / W seconds | 5/15/30/60s | null on incomplete window |
| `buyer_arrival_acceleration` | current arrival rate − prior equal-window rate | 15/30s | null until both windows exist |
| `repeat_buyer_ratio_W` | buyers with at least two buys in W / unique buyers | 30/60/300s | null if no buyers |
| `buyer_retention_W` | buyers still holding positive attributable inventory / buyers | 60/300s | lower-bound/unknown if transfers unobserved |
| `wallet_churn_W` | sellers who fully exited attributable position / starting holders | 60/300s | null if starting balance coverage absent |
| `buy_size_p10/p50/p90_W` | quote-amount quantiles | 30/60/300s | null if no trades |
| `buy_size_cv_W` | order-size standard deviation / mean | 30/60s | null if fewer than two observations |
| `interarrival_cv_W` | inter-trade-gap standard deviation / mean | 30/60s | null if fewer than three trades |
| `sell_acceleration_W` | current sell quote/sec − previous equal-window sell quote/sec | 15/30/60s | null if either window incomplete |
| `largest_seller_share_W` | maximum wallet sell quote / total sell quote | 30/60/300s | null if no sells |
| `return_W` | current executable price / executable price W ago − 1 | 15/30/60/300s | null if quote unavailable at either endpoint |
| `realized_volatility_W` | sqrt(sum of squared log executable-price increments) | 30/60/300s | null if gaps/order unresolved |
| `net_creator_flow_W` | creator-linked buy quote − sell quote, link as-of cutoff | 30/60/300s | null if graph required but absent |
| `round_trip_cost_fraction` | entry cash − immediate modeled exit cash, divided by entry cash | current/notional | null if either route unsupported |
| `entry_impact_bps` | execution-average versus pretrade marginal price, same units | current/notional | null if state unknown |
| `curve_progress` | completed fraction under the actual protocol's reserve/completion definition | current | never a fixed SOL threshold across configs |

`curve_progress` must retain a formula ID and inputs. It is a display metric, not proof of graduation. Completion/migration labels use decoded events.

## 3. Concentration and graph features

| Feature | Definition |
|---|---|
| `holder_top_k_share` | sum of top k eligible holder balances / circulating eligible supply, for k=1/5/10 |
| `holder_hhi` | sum of squared eligible holder supply shares |
| `cluster_hhi` | same calculation after versioned high-confidence cluster aggregation |
| `independent_cluster_count` | high-confidence clusters + unclustered wallets, with an uncertainty/coverage field |
| `creator_linked_supply_share` | supply attributable to creator/linked cohort / eligible supply, with link confidence threshold |
| `fresh_wallet_share` | early buyers first observed within 24h / early buyers; “first observed” is not asserted creation time |
| `immediate_funding_share` | early buyers materially funded within 5 minutes before first buy / covered early buyers |
| `funding_diversity` | exp(Shannon entropy of material funding-source shares), with hub-adjusted variant |
| `cohort_recurrence` | count of prior shared launches passing recurrence/timing rules, excluding current outcome |
| `co_buy_density` | observed durable behavioural edges / possible edges inside the selected buyer graph |
| `weighted_degree_concentration` | largest weighted degree / sum of degrees |
| `cohort_modularity` | modularity of the pinned weighted community run |
| `smart_wallet_retention` | reputation-weighted retained early inventory / reputation-weighted bought inventory |
| `wallet_skill_posterior_mean` | weighted mean of point-in-time reputation posterior values |
| `wallet_skill_effective_sample` | effective sample size of that reputation evidence |
| `creator_graduation_posterior` | shrunk graduation rate based only on matured prior launches |
| `creator_collapse_posterior` | shrunk severe-collapse rate under the pinned label definition |
| `history_coverage_fraction` | relevant wallets with required history range / relevant wallets |

Holder inventory must account for visible transfers and token-program semantics. If a complete holder snapshot is unavailable, label calculations as covered-subset estimates and prevent exact full-supply claims.

## 4. Pair-score starting configuration

Begin with non-probabilistic evidence scores, all in `[0,1]`:

```text
S = 0.25 * recurrence
  + 0.20 * timing_similarity
  + 0.10 * size_similarity
  + 0.20 * common_funding
  + 0.15 * co_sell_recurrence
  + 0.10 * creator_overlap
```

Compute recurrence against launch-popularity/activity-adjusted expected overlap; apply hub penalties before aggregation. Initial durable-edge threshold is `S >= 0.65` and at least three distinct prior shared launches. These are inspectable engineering defaults, not trained ownership probabilities. Tune only on training partitions and retain component scores in the UI.

Timing similarity starts as `exp(-median_absolute_entry_gap_seconds / 2)` over shared launches. Size similarity compares quote-normalized fractions of available capital, not raw token quantity. If balance history is missing, size similarity is unavailable and the edge cannot receive the full component's weight by silently renormalizing away missing evidence.

## 5. Label parameters and maturation

| Label | Positive condition | Matures no earlier than |
|---|---|---|
| `graduated_by_6h_v1` | canonical decoded graduation ≤ cutoff + 6h | cutoff + 6h + finality/coverage buffer |
| `up_50_before_down_20_15m_v1` | net executable position value first reaches +50% before −20% within 15m | terminal barrier or full 15m, with ordering resolved |
| `severe_collapse_1h_v1` | prior qualifying executable peak falls ≥80% during 1h and stays below 40% of peak for a subsequent 15m | cutoff + up to 1h + 15m + buffer |
| `net_return_15m_v1` | actual modeled executable exit at +15m net of all scenario costs | +15m + coverage/finality buffer |
| `exit_hazard_60s_v1` | specified adverse exit event within next 60s, conditioned on survival so far | +60s + coverage/finality buffer |

Initial severe-collapse qualifying threshold: at least $5,000 of reconstructed quote volume before the peak and an executable $15 exit quote at the peak. Store these thresholds in the label manifest; exclude/censor cases lacking USD valuation rather than pretending the threshold was satisfied. They are research definitions to compare, not universal descriptions of fraud.

If an observation stops before the required persistence window, the label is censored. A quiet market is not automatically a collapse and a missing route is not automatically a zero-valued fill; record the distinct no-liquidity/no-route outcome and use a declared conservative valuation scenario.

## 6. Data-quality policy and model availability

- Quality A: full required program/event scope in the window, exact order, fresh state and required graph/history coverage.
- Quality B: core market state exact/fresh, optional enrichment missing; any required model feature must still satisfy its own contract.
- Quality C: provisional ordering, stale state or incomplete required scope; display/watch only by default.
- Unavailable: incompatible decoder or no trustworthy state; no quote/prediction-based entry.

Quality is not evidence of token safety. It describes reliability of the input data, separately from market risk.

