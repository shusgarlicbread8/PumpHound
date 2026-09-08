# Runbook: paper ledger divergence

## Signal

- `just verify-ledger <portfolio_id>` non-zero.
- Spendable cash negative, reservation > cash, or lot consumption > acquisition.
- Fill exists without matching `ledger_movements` / `acquisition_lots`.
- Fork correction without a new `simulation_run`.

## Safety

1. Freeze **new entries** on that portfolio. Keep exits able to flatten if the engine supports it; otherwise halt the run.
2. Do not `UPDATE`/`DELETE` ledger rows. Append a `paper_corrections` record and a new run.
3. This is simulation integrity, not an exchange dispute. Still treat it as a stop-the-line bug.

## Recovery

1. Capture `GET /paper/portfolios/{id}/ledger` and the verify-ledger output.
2. Recompute balances: `SUM(signed_atomic)` minus open reservations, per mint.
3. Identify the first violating `order_id` / `fill_id`. Check quote expiry, arrival-before-decision, and double-counted fees.
4. If a processed fork retracted the underlying chain event, start `kind=canonical_reconciliation` — do not rewrite the original fill.
5. Patch the paper engine (rounding, rent, quote mint). Re-replay the same manifest; require `ledger_balanced` and zero lot/reservation violations.
6. Audited breaker reset only after the bug fix, never to “make the number green.”

## Done when

- New run reconciles to zero.
- Original broken run remains readable.
- Policy/version IDs on decisions unchanged.
