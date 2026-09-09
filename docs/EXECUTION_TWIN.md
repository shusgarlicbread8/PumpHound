# Execution Twin

Paper-only. There is no signer, no wallet, no mainnet submission path. Real transaction
*construction* exists solely so the simulator can be checked against the real protocol and so
predictions can be calibrated against what the chain did next.

Simulator version: `execution-twin-v1.0.0` (`crates/execution-twin/src/contract.rs`).

## Why it exists

The legacy paper simulator (`crates/paper-core/src/quote.rs`, `legacy-paper-v1`) filled every
order one second after the decision, at a constant-product price with a flat 1.25% fee, from the
*decision-time* market state. That overstates any strategy in four ways: no failed attempts, no
network fees, no adverse selection between decision and arrival, and a decorative mark for open
positions. The prior headline (+1.86%, one ~5.5 SOL winner) was produced by that simulator and is
not evidence of edge until it survives realistic execution.

The twin replaces `decision → 1 s → approximate quote → fill` with:

```
decision → ExecutionIntent → PumpExecutionBuilder (exact ix/accounts/CU budget/slippage)
        → NetworkCosts (base + CU limit × CU price + Jito scenario)
        → LandingModel sample (slots / expire)
        → arrival-state exact quote (never decision state)
        → slippage check → fill or FailureClass (fees still charged when the tx landed)
        → ExecutionResult (integer lamports/atomics, full cost attribution)
        → paper ledger (fill + network-cost movement) + trade_trace_events
```

Legacy books are frozen on `legacy-paper-v1`; only `EXECUTION-EDGE-V1` books route through the
twin. Each book's simulator is recorded on every order/fill (`paper_orders.simulator_version`).

## Components

| Component | File | Version / source |
|---|---|---|
| Contracts (`ExecutionIntent`, `ExecutionResult`, `FailureClass`, `JitoScenario`) | `crates/execution-twin/src/contract.rs` | `execution-twin-v1.0.0` |
| Pump / PumpSwap math (integer, exact rounding, market-cap fee tiers, creator/cashback/LP fees) | `pump_math.rs` | transcribed from `@pump-fun/pump-sdk@1.36.0`, `@pump-fun/pump-swap-sdk@1.19.0` |
| On-chain fee schedule (`FeeConfig`, `Global`) | `fee_config.rs` + `vendor/pump-idl/onchain-fee-config.snapshot.json` | `onchain-fee-config-2026-09-09` |
| IDLs | `vendor/pump-idl/*.json`, `vendor/pump-idl/SOURCE.lock` | pump-public-docs `9c82f61cb711b044a17f770ab8ce9f9bdf78f333` |
| PDA / ATA derivation (no Solana SDK) | `pda.rs` | tested against known mainnet PDAs |
| Transaction builder (legacy message, compute-budget ixs, ATA create, buy/sell) | `builder.rs` | `pump-execution-builder-v1`, synthetic identity |
| Compute budget + priority fee + Jito | `network.rs` | `compute-budget-v1-static`, `jito-auction-v1-percentile`; 20% CU safety margin |
| Landing / failure sampling | `landing.rs` | `landing-v1-prior` (empirical prior, seeded, deterministic) |
| Arrival-state execution | `twin.rs` | — |
| Executable liquidation mark | `mark.rs` | — |
| Fee oracle (Helius `getPriorityFeeEstimate`, Jito tip floor) | `services/core-api/src/execution_twin_bridge.rs` | persisted to `fee_environment_observations` |
| Ledger bridge / settlement / traces | `execution_twin_bridge.rs`, `paper_ledger.rs` | — |
| API (`/execution/status`, `/fidelity`, `/traces/{id}`, `/replay/{id}`) | `execution_routes.rs` | — |
| Cold-market rehydration for stranded exits | `market_rehydrate.rs` | on-chain `BondingCurve` account read |

## Modeled costs (every one is stored per result)

- Base fee: 5,000 lamports × required signatures.
- Priority fee: **requested CU limit** × CU price (µ-lamports). Never actual CU consumed.
  CU limit = estimated CU × (1 + 20% margin). Estimates are static per instruction kind
  (`compute-budget-v1-static`); Surfpool profiling can replace them.
- CU price policy: `P50` (default), `P75`, `P95` of the observed distribution. Oracle stale or
  down → labelled fallback (`fallback:static-v1`, `is_fallback=true`, +25 bps uncertainty).
- Jito scenario: `NO_JITO` (default for paper), `CONSERVATIVE` (p25), `MEDIAN` (p50),
  `AGGRESSIVE` (p95) of the observed tip distribution. Tips are modeled, never sent.
- Protocol fees: exact per-tier protocol + creator (or cashback) + LP fee from the vendored
  on-chain schedule, applied with the SDK's rounding.
- Failed attempts: landed-but-failed classes charge base + priority (+ tip if landed via Jito);
  never-landed classes charge nothing. No swap output is ever credited on failure.

## Failure classes

`STALE_BLOCKHASH, SLIPPAGE_EXCEEDED, INSUFFICIENT_OUTPUT, ACCOUNT_STATE_CHANGED,
COMPUTE_EXCEEDED, ACCOUNT_CONTENTION, SIMULATION_FAILURE, LANDING_TIMEOUT,
UNSUPPORTED_MARKET_STATE, DATA_STALE, UNKNOWN`. Fee-exempt mayhem-mode trades and graduated
curves are `UNSUPPORTED_MARKET_STATE` (refused, not approximated).

## Separate concepts, kept separate

- `adverse_move_bps`: arrival price vs decision price (adverse selection while waiting).
- `price_impact_bps`: execution price vs arrival mid (our own footprint).
- `liquidation_impact_bps` (marks): theoretical mid × size vs executable liquidation proceeds.

## Durable entities (migration `017_execution_twin.sql`)

`execution_intents`, `execution_results`, `execution_cost_components`,
`fee_environment_observations`, `landing_observations`, `trade_trace_events`,
`execution_replays`; plus `trace_id / intent_id / simulator_version` on `paper_orders` and
`result_id / network_cost_lamports / simulator_version` on `paper_fills`. Replays never
overwrite results.

## Verification

```
just execution-twin-test        # exact math, PDAs, fees, landing, replay determinism,
                                # golden parity vs 96 archived mainnet trades (GATE 1)
just execution-twin-surfpool    # GATE 14: canonical BUY executes in a Surfpool mainnet fork
                                # and matches the twin quote (needs `surfpool start`)
just execution-fidelity         # attempts / fills / failure classes / cost per book
just execution-replay <trace> '{"landing_delay_slots":1,"priority_fee_policy":"P95"}'
```

Surfpool is offline verification. It is never on the live path; when it is not configured
`/execution/status` reports `OFFLINE_VERIFICATION_DEGRADED`, not blocked.

## Known limitations (simulation still differs from mainnet)

- Landing probabilities are an empirical **prior** (`landing-v1-prior`), conditioned only on
  congestion class × Jito scenario. Shadow observations (`landing_observations`) are being
  collected to calibrate it; no calibration has been applied yet.
- CU estimates are static per instruction kind; no per-transaction profiling in the live path.
- Account contention and leader schedule are not modeled beyond the congestion class.
- PumpSwap: canonical pools only; no Raydium/Meteora.
- The Jito model consumes public tip-floor percentiles; it does not model the auction itself.
- Executable marks use current hot state (or an on-chain read); they are not a full order-book
  sweep of what other sellers would do at the same time.
- Positions in PumpSwap pools that have gone cold cannot be rehydrated yet and stay open with an
  explicit reason (`rehydrate_unsupported_pumpswap`).
