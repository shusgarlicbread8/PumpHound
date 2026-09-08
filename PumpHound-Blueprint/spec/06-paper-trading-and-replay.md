# Paper Trading and Deterministic Replay

## Purpose

Paper trading is a first-class live product, not a chart-side toy. It must answer whether a signal remains valuable after observation delay, decision delay, execution latency, protocol fees, network fees, price impact, failed transactions and portfolio constraints.

There is no real signer, private-key input or transaction submission path in v1.

## 1. Supported entry paths

- Manual paper order from a token page.
- Alert-to-paper order requiring a user click.
- Automated paper policy operating only in `LIVE_PAPER_HEURISTIC`,
  `LIVE_PAPER_SHADOW_ML`, `LIVE_PAPER_ML`, or `REPLAY` mode.
- Counterfactual shadow policies that record decisions without consuming the primary paper portfolio.

All paths generate the same immutable decision, order, fill and ledger records.

## 2. Decision clock

For a candidate event at source time `t0`:

```text
t0 source event
t1 observed by collector
t2 feature snapshot complete
t3 prediction complete
t4 policy decision created
t5 simulated transaction reaches market
t6 fill or rejection determined
```

Store every timestamp and slot. The simulator may use recorded live delays, fixed scenario delays or sampled empirical delays. It must never fill against a market state earlier than `t5`.

## 3. Quote and fill engine

### 3.1 Bonding curve

Use the vendored official IDL and decoded on-chain state to reproduce the applicable instruction/fee version. Compute integer arithmetic in native atomic units with the same rounding direction as the program. Support each observed quote mint explicitly.

### 3.2 PumpSwap

Derive effective reserves using pool token balances plus the decoded virtual quote-reserve state where applicable. Calculate a route-specific executable quote and fee components at the simulated arrival slot.

### 3.3 Jupiter comparison

For migrated tokens, optionally call Jupiter Swap V2 `/swap/v2/build` without signing or submission. Store the returned route/quote as an external comparison. The internal simulator remains reproducible if Jupiter is unavailable.

### 3.4 Fill outcomes

A paper swap produces one of:

- `FILLED`: exact simulated atomic input/output and costs;
- `REJECTED_SLIPPAGE`: output below configured minimum;
- `REJECTED_LIQUIDITY`: no executable route/depth;
- `REJECTED_STALE_STATE`: quote exceeds age budget;
- `SIMULATED_TX_FAILURE`: sampled or replay-observed failure class;
- `RISK_REJECTED`: portfolio gate failed before simulated submission.

Do not grant partial fills to an atomic swap. If an order is split deliberately, each child swap is separately atomic.

## 4. Cost model

Store each cost component independently:

- launchpad/AMM protocol fee;
- creator fee;
- LP fee;
- network base fee;
- priority fee;
- optional modeled Jito tip for future execution research;
- price impact;
- slippage between decision quote and arrival-state execution;
- failed-transaction fee where applicable;
- quote/USD conversion uncertainty.

Protocol fee schedules are versioned inputs sourced from decoded events/on-chain configuration or an explicit dated configuration. Never bake one current percentage into historical replay.

## 5. Ledger and positions

Use atomic integers for token, SOL and SPL quote quantities. Each fill writes:

- portfolio and strategy ID;
- order/fill/decision ID;
- base mint and quote mint;
- base and quote atomic amounts;
- every fee component and asset;
- acquisition lot IDs;
- executable USD valuation source and timestamp when available;
- realized/unrealized P&L calculations and version.

Maintain append-only cash movements and acquisition lots. Derived balances must reconcile to the sum of movements. Selling consumes lots according to the configured accounting method, default FIFO, while strategy analytics also track position-level average cost.

Never silently convert a missing USD price to zero. Native-quote P&L remains valid even if consolidated USD P&L is temporarily unknown.

## 6. Default research risk policy

Defaults are editable research settings, not investment advice:

| Control | Default for a $1,000 paper bankroll |
|---|---:|
| standard initial order notional | $10–$20 equivalent |
| maximum single-token cost basis | 5% |
| maximum simultaneous gross exposure | 15% |
| maximum creator/cluster-family exposure | 7.5% |
| daily realized + marked loss breaker | 5% |
| strategy peak-to-trough breaker | 15% |
| stale quote rejection | configurable by venue; strict by default |
| averaging down | disabled |
| leverage/borrow | unavailable |

Breakers prevent new entries; an explicit exit policy continues managing open paper positions. A user reset creates a new policy state record with a reason rather than deleting the breach.

## 7. Exit engine

Entry and exit are evaluated independently. Supported versioned exits:

- fixed stop and take-profit;
- staged profit-taking;
- trailing stop based on executable value;
- time stop;
- graduation/migration state;
- model exit-hazard threshold;
- graph event such as creator-linked sell acceleration;
- liquidity or data-quality emergency exit;
- investigator stance as human-readable context only, never an automatic policy input in v1.

An exit decision stores all candidate reasons and the winning precedence rule. Use a deterministic precedence table so replay gives the same result.

## 8. Deterministic replay

Replay consumes the same normalized event contracts as live mode. It controls a virtual clock and publishes events through the same reducer. Required controls:

- date/slot range, token or saved incident;
- 0.25×, 1×, 10×, 100× and maximum throughput;
- pause, single-event and single-slot step;
- deterministic seed for sampled latencies/failures;
- checkpoint and resume;
- compare multiple model/policy versions from the same event stream.

The replay manifest pins raw partitions, IDLs, decoders, feature schemas, models, policies, fee schedules and random seed. Its result contains a content hash.

## 9. Live shadow evaluation

Every eligible live launch receives shadow predictions even if no paper trade occurs. Preserve:

- eligibility/gate result;
- model outputs;
- policy decision;
- hypothetical quote at decision and arrival;
- actual future labels when mature;
- reason for abstention.

This prevents selection bias caused by retaining only exciting trades.

## 10. Paper analytics

Report gross and net results and attribute the difference to costs. At minimum:

- net return, expectancy/trade and turnover;
- win rate, profit factor, Sharpe and Sortino with suitable caveats;
- maximum drawdown and time under water;
- maximum favourable/adverse excursion;
- entry and exit slippage distributions;
- rejection/failure rates;
- result by model, policy, launchpad, quote mint, regime and graph-risk band;
- top-trade concentration;
- result with top three, top five and top 1% winners removed;
- bootstrap confidence intervals grouped by time and wallet cluster.

## 11. Simulator calibration

When live observation data are available, compare simulated quotes with the next observed successful trades and external quote-only routes. Update cost/latency distributions through versioned calibration runs. Never revise completed paper fills in place; recompute a new counterfactual run.

## 12. Amount semantics, settlement and failure costs

For every venue, define these quantities explicitly:

```text
gross_input = reserve_input + fees deducted from input
net_output = reserve_output - fees deducted from output
cash_change(input_asset) = -gross_input - separately charged costs in that asset
cash_change(output_asset) = +net_output - separately charged costs in that asset
```

The quote and fill include both gross and reserve amounts. A fee included in gross input must not be debited a second time as an additional cash cost. Ledger category rows may split the gross debit into trade input and fee rows, but their sum must equal the gross debit plus genuinely separate costs. Fees paid in SOL are not silently subtracted from USDC output.

The wrapped-SOL mint can identify a SOL quote while settlement is native SOL under the protocol. Keep settlement mode explicit. Model associated-token-account creation/rent when the simulated account does not exist, distinguish rent lock from expense, and model a refund only when an account is actually closed in that simulation. Do not charge account creation on every trade.

Reject unsupported Token-2022 extensions before quoting; transfer fees/hooks/permanent delegates cannot be treated as ordinary SPL transfers. Track token owner/program, mint/freeze authorities and extension flags as separate evidence, not a blanket “safe” badge.

Failed **landed** transactions can still pay network/priority costs, whereas dropped/non-landed transactions generally do not pay an on-chain transaction fee. Venue swaps revert atomically on failure. Tip/bundle costs are scenario-specific and must not be charged indiscriminately on every failed attempt. Preserve failure classification and cost assumptions.

## 13. Counterfactual market-impact limitations

Paper trading cannot know how real competitors would respond to an order that was never sent. Support two labelled modes:

1. `bounded_shadow` (default): quote against observed arrival-state liquidity, charge each order's price impact, limit order/position size to configured reserve fractions, and include a conservative self-impact haircut when valuing an open position. The observed external market path remains immutable. This is an approximation, not a claim to reproduce the alternate chain.
2. `impact_overlay` (research): each simulation branch has its own reserve overlay. Apply hypothetical orders to that overlay and replay observed external input intentions through it where possible. Recomputed external outputs may differ; record divergence and stop the branch if instruction constraints become unknowable. Never overwrite the canonical observed tape.

Do not reset hypothetical costs/position impact by blindly marking the position at the unadjusted chart price. Do not grant several simultaneous strategies unlimited independent use of the same pool liquidity; compare standalone runs separately from a shared-capital/shared-capacity portfolio simulation.

Default maximum single-order notional is 0.5% of effective quote reserves and total position notional 1%, subject also to much tighter bankroll limits. These are conservative starting limits to stress, not guarantees of fill realism. Curves near completion additionally respect real token inventory and migration constraints.

When a market migrates between decision and simulated arrival, reject the old quote and request an explicitly new venue quote. Do not silently grant the old curve price on the new pool.

## 14. Unavailable marks and emergency exits

If a quote becomes unavailable, record `UNMARKABLE` and its reason. Report last-known value with age, plus a conservative stress-equity view; new-entry gates must not ignore this exposure. An attempted emergency exit can be rejected or remain unfilled. Never manufacture an executable zero-price sale or a guaranteed stop-loss fill.

Stops and take-profit stages use net executable position return relative to allocated cost basis, not a candle print. Staged sizes are fractions of the original position capped by remaining quantity. A migration, halted source or liquidity collapse may prevent an intended exit.

## 15. Acceptance scenarios

1. A 500 ms configured latency cannot fill at the decision-time reserve state when intervening trades changed it.
2. A minimum-output breach rejects the entire atomic swap.
3. Replaying the same manifest and seed yields byte-equivalent decisions and ledger totals.
4. SOL-quoted and USDC-quoted markets reconcile independently.
5. Provider loss produces an explicit data-quality/risk rejection rather than a favourable zero-flow feature.
6. A top-winner removal report is visible beside headline P&L.
