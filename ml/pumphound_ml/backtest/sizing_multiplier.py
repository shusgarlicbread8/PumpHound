"""Walk-forward validation of the confidence-scaled position-sizing band.

`services/core-api/src/paper_loop.rs::confidence_scaled_atomic` scales every non-control-flow
book's entry size by `edge = shadow_momentum_probability - shadow_adverse_move_probability`,
mapped through a fixed band (min_multiplier=0.4, max_multiplier=1.8, edge_at_max=0.3). That band
was chosen by reasoning (fractional-Kelly in spirit, deliberately narrow so one bad shadow
reading can't zero out or blow up exposure), never fitted to data. This module is the harness to
check that reasoning against what actually happened.

Two questions, in order of importance:
  1. Is `edge` even predictive of realized return here? If it isn't, sizing up on higher edge is
     actively harmful and no amount of band-retuning fixes that — the fix would be upstream, in
     the shadow model itself.
  2. Given the current band, would a different (min, max, edge_at_max) have done better
     out-of-sample under this codebase's own walk-forward discipline (reuses
     `pumphound_ml.evaluation.walk_forward.walk_forward_folds`, the same expanding-window
     train/validation/test split used for the ML retraining pipeline elsewhere in this repo)?

Sample size discipline: this product's own UI already asserts "Strategy status UNPROVEN unless
closed_trades >= 30" (services/core-api/src/cockpit.rs). This harness applies the same bar to
itself — a fold or objective computed from under 30 closed lots is reported but flagged
INSUFFICIENT_EVIDENCE rather than treated as a real result.

Run as a script: PUMPHOUND_POSTGRES_DSN=... python -m pumphound_ml.backtest.sizing_multiplier
"""

from __future__ import annotations

import os
import statistics
from dataclasses import dataclass, field
from typing import Any

from pumphound_ml.evaluation.walk_forward import walk_forward_folds

# The live constants (services/core-api/src/paper_loop.rs::SIZE_SCALING_*). Kept as a literal
# copy rather than a cross-language import — there is no Rust/Python bridge for constants in
# this repo, so if the live values change this comparison baseline must be updated by hand.
CURRENT_MIN_MULTIPLIER = 0.4
CURRENT_MAX_MULTIPLIER = 1.8
CURRENT_EDGE_AT_MAX = 0.3

MIN_LOTS_FOR_EVIDENCE = 30

QUERY = """
SELECT
  bd.decision_id::text AS decision_id,
  bd.decision_at,
  (bd.exact_inputs->'position_sizing'->>'edge')::float8 AS edge,
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
  AND bd.exact_inputs->'position_sizing'->>'multiplier' IS NOT NULL
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

WSOL_MINT = "So11111111111111111111111111111111111111112"


@dataclass
class Lot:
    decision_id: str
    decision_at: Any
    edge: float
    return_fraction: float


@dataclass
class BandResult:
    min_multiplier: float
    max_multiplier: float
    edge_at_max: float
    n: int
    mean_scaled_return: float | None = None
    sharpe_like: float | None = None
    evidence: str = "INSUFFICIENT_EVIDENCE"


def fetch_lots(dsn: str) -> list[Lot]:
    import psycopg

    lots: list[Lot] = []
    with psycopg.connect(dsn) as conn, conn.cursor() as cur:
        cur.execute(QUERY, (WSOL_MINT,))
        for decision_id, decision_at, edge, cost_atomic, pnl_atomic in cur.fetchall():
            if edge is None or cost_atomic is None or pnl_atomic is None:
                continue
            cost = float(cost_atomic)
            if cost <= 0:
                continue
            lots.append(
                Lot(
                    decision_id=decision_id,
                    decision_at=decision_at,
                    edge=float(edge),
                    return_fraction=float(pnl_atomic) / cost,
                )
            )
    return lots


def edge_predictiveness(lots: list[Lot]) -> dict[str, Any]:
    """Quintile breakdown + Spearman correlation: does higher edge actually mean higher return?"""
    if len(lots) < MIN_LOTS_FOR_EVIDENCE:
        return {"evidence": "INSUFFICIENT_EVIDENCE", "n": len(lots)}
    ranked = sorted(lots, key=lambda lot: lot.edge)
    n = len(ranked)
    edge_ranks = {lot.decision_id: i for i, lot in enumerate(ranked)}
    by_return = sorted(lots, key=lambda lot: lot.return_fraction)
    return_ranks = {lot.decision_id: i for i, lot in enumerate(by_return)}
    d_sq = sum(
        (edge_ranks[lot.decision_id] - return_ranks[lot.decision_id]) ** 2 for lot in lots
    )
    spearman = 1 - (6 * d_sq) / (n * (n**2 - 1)) if n > 1 else 0.0
    quintile_size = max(1, n // 5)
    quintiles = []
    for q in range(5):
        start = q * quintile_size
        end = n if q == 4 else min(n, start + quintile_size)
        chunk = ranked[start:end]
        if not chunk:
            continue
        quintiles.append(
            {
                "quintile": q + 1,
                "n": len(chunk),
                "edge_range": (round(chunk[0].edge, 4), round(chunk[-1].edge, 4)),
                "mean_return": round(statistics.fmean(x.return_fraction for x in chunk), 4),
            }
        )
    return {
        "evidence": "OK",
        "n": n,
        "spearman_edge_vs_return": round(spearman, 4),
        "quintiles": quintiles,
    }


def multiplier_for(edge: float, min_mult: float, max_mult: float, edge_at_max: float) -> float:
    t = max(0.0, min(1.0, edge / edge_at_max)) if edge_at_max else 0.0
    return min_mult + t * (max_mult - min_mult)


def evaluate_band(
    lots: list[Lot], min_mult: float, max_mult: float, edge_at_max: float
) -> BandResult:
    if len(lots) < MIN_LOTS_FOR_EVIDENCE:
        return BandResult(min_mult, max_mult, edge_at_max, len(lots))
    scaled = [
        multiplier_for(lot.edge, min_mult, max_mult, edge_at_max) * lot.return_fraction
        for lot in lots
    ]
    mean = statistics.fmean(scaled)
    stdev = statistics.pstdev(scaled) if len(scaled) > 1 else 0.0
    sharpe = mean / stdev if stdev > 1e-12 else None
    return BandResult(
        min_mult,
        max_mult,
        edge_at_max,
        len(lots),
        mean_scaled_return=round(mean, 6),
        sharpe_like=round(sharpe, 4) if sharpe is not None else None,
        evidence="OK",
    )


def grid_search_best_band(
    train_lots: list[Lot],
    min_mult_grid: list[float] = (0.2, 0.3, 0.4, 0.5),
    max_mult_grid: list[float] = (1.4, 1.6, 1.8, 2.0, 2.2),
    edge_at_max_grid: list[float] = (0.15, 0.2, 0.3, 0.4),
) -> BandResult | None:
    best: BandResult | None = None
    for min_mult in min_mult_grid:
        for max_mult in max_mult_grid:
            if max_mult <= min_mult:
                continue
            for edge_at_max in edge_at_max_grid:
                result = evaluate_band(train_lots, min_mult, max_mult, edge_at_max)
                if result.sharpe_like is None:
                    continue
                if best is None or result.sharpe_like > best.sharpe_like:
                    best = result
    return best


def run_walk_forward(lots: list[Lot]) -> dict[str, Any]:
    rows = [
        {"decision_id": lot.decision_id, "cutoff_time": lot.decision_at.isoformat()}
        for lot in lots
    ]
    folds = walk_forward_folds(rows, n_folds=2, id_key="decision_id", time_key="cutoff_time")
    by_id = {lot.decision_id: lot for lot in lots}
    fold_reports = []
    for fold in folds:
        train = [by_id[i] for i in fold.train_ids if i in by_id]
        test = [by_id[i] for i in fold.test_ids if i in by_id]
        if not test:
            test = [by_id[i] for i in fold.validation_ids if i in by_id]
        current = evaluate_band(
            test, CURRENT_MIN_MULTIPLIER, CURRENT_MAX_MULTIPLIER, CURRENT_EDGE_AT_MAX
        )
        best_on_train = grid_search_best_band(train)
        best_out_of_sample = (
            evaluate_band(
                test,
                best_on_train.min_multiplier,
                best_on_train.max_multiplier,
                best_on_train.edge_at_max,
            )
            if best_on_train is not None
            else None
        )
        fold_reports.append(
            {
                "fold_id": fold.fold_id,
                "train_n": len(train),
                "test_n": len(test),
                "current_band_oos": current,
                "train_selected_band": best_on_train,
                "train_selected_band_oos": best_out_of_sample,
            }
        )
    return {"folds": fold_reports}


def main() -> None:
    dsn = os.environ.get("PUMPHOUND_POSTGRES_DSN")
    if not dsn:
        raise SystemExit("PUMPHOUND_POSTGRES_DSN_required")
    lots = fetch_lots(dsn)
    print(f"closed_sized_lots={len(lots)}")
    predictiveness = edge_predictiveness(lots)
    print("\n=== Is shadow edge predictive of realized return at all? ===")
    print(predictiveness)
    full_sample_current = evaluate_band(
        lots, CURRENT_MIN_MULTIPLIER, CURRENT_MAX_MULTIPLIER, CURRENT_EDGE_AT_MAX
    )
    print("\n=== Current live band, full sample (not out-of-sample; sanity check only) ===")
    print(full_sample_current)
    print("\n=== Walk-forward: current band vs. train-selected band, evaluated out-of-sample ===")
    report = run_walk_forward(lots)
    for fold in report["folds"]:
        print(f"\n-- fold {fold['fold_id']} (train_n={fold['train_n']}, test_n={fold['test_n']}) --")
        print("current_band_oos:", fold["current_band_oos"])
        print("train_selected_band:", fold["train_selected_band"])
        print("train_selected_band_oos:", fold["train_selected_band_oos"])


if __name__ == "__main__":
    main()
