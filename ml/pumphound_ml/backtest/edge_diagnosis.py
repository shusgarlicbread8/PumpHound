"""Diagnose GATE 9 (confidence-sizing inversion): is `edge = momentum - adverse` mis-signed?

`sizing_multiplier.py` established that the live edge formula is negatively correlated with
realized return. This module asks a narrower, more actionable question: is the *sign* on the
adverse term the problem, or is the whole formula noise? It recomputes edge from the raw
`shadow_momentum_probability` / `shadow_adverse_move_probability` already stored in
`decisions.exact_inputs->'position_sizing'` (no live re-run needed) under a few candidate
definitions, and runs the same walk-forward discipline `sizing_multiplier.py` uses.

Hypothesis under test, motivated by a cross-check against ground-truth labels: in
`outcome_labels`, launches whose `adverse_move_2m_v1` label fired (a 10%+ dip within 2 minutes)
went on to hit `expected_mfe_5m_v1` (a 10%+ pump within 5 minutes) at ~49% vs ~25% for launches
that never dipped — i.e. short-term volatility/drawdown in this heavy-tailed asset class is a
precursor to the big moves the whole platform's P&L depends on, not purely a risk to avoid.
`adverse_probability` may be doing the same job in reverse in the live sizing formula: subtracting
it penalizes exactly the trades most likely to be the rare big winner.

This is diagnostic, not a promotion decision. GATE 9 requires walk-forward evidence that a
candidate formula is non-worse out-of-sample before it goes anywhere near live sizing; this
script produces that evidence, it does not act on it.

Run: PUMPHOUND_POSTGRES_DSN=... python -m pumphound_ml.backtest.edge_diagnosis
"""

from __future__ import annotations

import os
import statistics
from dataclasses import dataclass

from pumphound_ml.backtest.sizing_multiplier import (
    CURRENT_EDGE_AT_MAX,
    CURRENT_MAX_MULTIPLIER,
    CURRENT_MIN_MULTIPLIER,
    MIN_LOTS_FOR_EVIDENCE,
    WSOL_MINT,
    multiplier_for,
)
from pumphound_ml.evaluation.walk_forward import walk_forward_folds

QUERY = """
SELECT
  bd.decision_id::text AS decision_id,
  bd.decision_at,
  (bd.exact_inputs->'position_sizing'->>'shadow_momentum_probability')::float8 AS momentum,
  (bd.exact_inputs->'position_sizing'->>'shadow_adverse_move_probability')::float8 AS adverse,
  c.allocated_cost_atomic::numeric AS cost_atomic,
  c.realized_pnl_atomic::numeric AS realized_pnl_atomic
FROM pumphound.lot_consumptions c
JOIN pumphound.acquisition_lots l ON l.lot_id = c.lot_id
JOIN pumphound.paper_fills bf ON bf.fill_id = l.fill_id
JOIN pumphound.paper_orders bo ON bo.order_id = bf.order_id
JOIN pumphound.decisions bd ON bd.decision_id = bo.decision_id
JOIN pumphound.paper_positions pos
  ON pos.simulation_run_id = l.simulation_run_id
 AND pos.base_mint = l.base_mint
WHERE l.quote_mint = %s
  AND bd.exact_inputs->'position_sizing'->>'shadow_momentum_probability' IS NOT NULL
  AND c.allocated_cost_atomic > 0
  AND NOT EXISTS (
    SELECT 1 FROM pumphound.entity_integrity_quarantines iq
    WHERE iq.status='ACTIVE' AND (
      (iq.entity_type='LOT' AND iq.entity_id=l.lot_id::text)
      OR (iq.entity_type='LAUNCH' AND iq.entity_id=bd.launch_id::text)
      OR (iq.entity_type='POSITION' AND iq.entity_id=pos.position_id::text)
      OR (iq.entity_type='LAUNCH' AND iq.entity_id=pos.launch_id::text)
    )
  )
ORDER BY bd.decision_at
"""


@dataclass
class RawLot:
    decision_id: str
    decision_at: object
    momentum: float
    adverse: float
    return_fraction: float


EDGE_VARIANTS = {
    "current_momentum_minus_adverse": lambda m, a: m - a,
    "momentum_only": lambda m, a: m,
    "adverse_only_as_signal": lambda m, a: a,
    "momentum_plus_adverse": lambda m, a: m + a,
}


def fetch_raw_lots(dsn: str) -> list[RawLot]:
    import psycopg

    lots: list[RawLot] = []
    with psycopg.connect(dsn) as conn, conn.cursor() as cur:
        cur.execute(QUERY, (WSOL_MINT,))
        for decision_id, decision_at, momentum, adverse, cost_atomic, pnl_atomic in cur.fetchall():
            if momentum is None or adverse is None or cost_atomic is None or pnl_atomic is None:
                continue
            cost = float(cost_atomic)
            if cost <= 0:
                continue
            lots.append(
                RawLot(
                    decision_id=decision_id,
                    decision_at=decision_at,
                    momentum=float(momentum),
                    adverse=float(adverse),
                    return_fraction=float(pnl_atomic) / cost,
                )
            )
    return lots


def spearman(xs: list[float], ys: list[float]) -> float:
    n = len(xs)
    if n < 2:
        return 0.0
    rank_x = [0] * n
    rank_y = [0] * n
    for rank, i in enumerate(sorted(range(n), key=lambda i: xs[i])):
        rank_x[i] = rank
    for rank, i in enumerate(sorted(range(n), key=lambda i: ys[i])):
        rank_y[i] = rank
    d2 = sum((rank_x[i] - rank_y[i]) ** 2 for i in range(n))
    return 1 - (6 * d2) / (n * (n**2 - 1))


def evaluate_edge_scaled_return(
    lots: list[RawLot], edge_fn, min_mult: float, max_mult: float, edge_at_max: float
) -> dict:
    if len(lots) < MIN_LOTS_FOR_EVIDENCE:
        return {"n": len(lots), "evidence": "INSUFFICIENT_EVIDENCE"}
    edges = [edge_fn(lot.momentum, lot.adverse) for lot in lots]
    returns = [lot.return_fraction for lot in lots]
    rho = spearman(edges, returns)
    scaled = [
        multiplier_for(e, min_mult, max_mult, edge_at_max) * r for e, r in zip(edges, returns)
    ]
    mean = statistics.fmean(scaled)
    stdev = statistics.pstdev(scaled) if len(scaled) > 1 else 0.0
    return {
        "n": len(lots),
        "spearman_edge_vs_return": round(rho, 4),
        "mean_scaled_return": round(mean, 6),
        "sharpe_like": round(mean / stdev, 4) if stdev > 1e-12 else None,
        "evidence": "OK",
    }


def walk_forward_by_variant(lots: list[RawLot]) -> dict:
    rows = [{"decision_id": lot.decision_id, "cutoff_time": lot.decision_at.isoformat()} for lot in lots]
    folds = walk_forward_folds(rows, n_folds=2, id_key="decision_id", time_key="cutoff_time")
    by_id = {lot.decision_id: lot for lot in lots}
    report: dict[str, list[dict]] = {name: [] for name in EDGE_VARIANTS}
    for fold in folds:
        test = [by_id[i] for i in fold.test_ids if i in by_id] or [
            by_id[i] for i in fold.validation_ids if i in by_id
        ]
        for name, fn in EDGE_VARIANTS.items():
            result = evaluate_edge_scaled_return(
                test, fn, CURRENT_MIN_MULTIPLIER, CURRENT_MAX_MULTIPLIER, CURRENT_EDGE_AT_MAX
            )
            result["fold_id"] = fold.fold_id
            result["n_test"] = len(test)
            report[name].append(result)
    return report


def main() -> None:
    dsn = os.environ.get("PUMPHOUND_POSTGRES_DSN")
    if not dsn:
        raise SystemExit("PUMPHOUND_POSTGRES_DSN_required")
    lots = fetch_raw_lots(dsn)
    print(f"closed_lots_with_raw_momentum_adverse={len(lots)}")

    print("\n=== Full-sample: does each edge definition correlate with realized return? ===")
    for name, fn in EDGE_VARIANTS.items():
        result = evaluate_edge_scaled_return(
            lots, fn, CURRENT_MIN_MULTIPLIER, CURRENT_MAX_MULTIPLIER, CURRENT_EDGE_AT_MAX
        )
        print(f"{name:35s} {result}")

    print("\n=== Walk-forward (out-of-sample test folds only) ===")
    wf = walk_forward_by_variant(lots)
    for name, folds in wf.items():
        print(f"\n-- {name} --")
        for f in folds:
            print(f"  fold {f['fold_id']} (n_test={f['n_test']}): {f}")


if __name__ == "__main__":
    main()
