"""Post-Cost Dataset Factory.

Turns every realized `EXECUTION-EDGE-V1` entry (buy) into a leakage-safe, failed-entry-aware
labeled row, using real Execution Twin output — not the legacy decision-time simulator, and not
a single sampled landing draw. This is the first deliverable of Alpha Research Core: it answers
nothing about edge by itself, it only produces the dataset Loss Filter / Tail Ranker / Exit
Hazard v2 train against.

Design decisions, and why:

  * Entry rows only (`side='buy'`). An exit's "post-cost outcome" is a different question (did
    the exit policy capture the move that was there); conflating the two in one row would hide
    which side of the trade a label is actually describing.
  * Failed entries are labeled, not dropped. A landed-but-failed transaction still paid base +
    priority fees; a never-landed one paid nothing (`docs/EXECUTION_TWIN.md`). Either way
    `position_taken=False` and the outcome is exactly the fee drag, nothing else — no price-path
    exposure to fabricate, no exclusion that would bias the dataset toward filled trades only.
  * Forward outcome labels (return / MFE / MAE / path-order binaries) come from
    `market_snapshots_buffer` in ClickHouse — real subsequently-observed ticks, not a
    re-simulation. They describe entry-side post-cost outcome only: fees and slippage already
    paid at entry are baked in (return is measured from `execution_price`, the price the twin
    actually filled at, not the decision or arrival price); a full round-trip cost (a further,
    twin-simulated exit at each horizon) is out of scope for this pass and is flagged as such in
    every row's `net_return_basis` field, not silently assumed.
  * Scenario robustness (FAST/BASE/SLOW/STRESSED) is entry-cost robustness only, via the new
    `/execution/replay/{trace_id}` `scenario` parameter and its historical-state resolver
    (`Stores::market_snapshot_at_or_after`). It answers "does this entry's cost profile hold up
    if the transaction had landed slower under worse fees", not "is the whole trade still
    profitable" — that would need a twin-simulated exit per scenario, likewise out of scope here.
  * Every row carries snapshot_id / cutoff_slot / trace_id / simulator_version /
    landing_model_version / fee_model_version / feature_version so any row is reproducible later
    even as those versions change.

Run: PUMPHOUND_POSTGRES_DSN=... PUMPHOUND_CLICKHOUSE_URL=... \
     python -m pumphound_ml.postcost.dataset_factory [--out PATH] [--core-api-url URL]
"""

from __future__ import annotations

import argparse
import json
import os
import statistics
import sys
from dataclasses import asdict, dataclass, field
from datetime import datetime, timezone
from decimal import Decimal
from typing import Any

FEE_MODEL_VERSION = "onchain-fee-config-2026-09-09"
LANDING_MODEL_VERSION = "landing-v1-prior"
DATASET_VERSION = "post-cost-dataset-v1"
HORIZONS_SECONDS = (30, 60, 120, 300)
SCENARIOS = ("FAST", "SLOW", "STRESSED")
WSOL_MINT = "So11111111111111111111111111111111111111112"

ENTRY_QUERY = """
SELECT
  r.result_id::text, r.intent_id::text, r.trace_id::text, i.decision_id::text,
  i.experiment_id, i.strategy_version, i.simulator_version, i.mint, i.market,
  i.decision_slot, i.decision_at, i.market_snapshot_id,
  r.success, r.failure_class,
  r.arrival_slot, r.landed_slot, r.arrival_delay_ms,
  r.network_cost_charged_lamports, r.protocol_fee_quote, r.creator_fee_quote,
  r.lp_fee_quote, r.cashback_fee_quote,
  r.price_impact_bps, r.adverse_move_bps,
  r.requested_quote, r.filled_quote, r.filled_base,
  r.decision_price_e9, r.arrival_price_e9, r.execution_price_e9,
  r.decision_snapshot_id, r.arrival_snapshot_id,
  r.created_at,
  d.feature_snapshot_id
FROM pumphound.execution_results r
JOIN pumphound.execution_intents i ON i.intent_id = r.intent_id
JOIN pumphound.decisions d ON d.decision_id = i.decision_id
WHERE i.side = 'buy'
  AND NOT EXISTS (
    SELECT 1 FROM pumphound.entity_integrity_quarantines iq
    WHERE iq.status = 'ACTIVE' AND iq.entity_type = 'LAUNCH'
      AND iq.entity_id IN (SELECT l.launch_id::text FROM pumphound.launches l WHERE l.mint = i.mint)
  )
ORDER BY i.decision_at
"""


def _num(value: Any) -> float:
    if value is None:
        return 0.0
    if isinstance(value, Decimal):
        return float(value)
    return float(value)


@dataclass
class ForwardLabels:
    net_return_30s: float | None = None
    net_return_60s: float | None = None
    net_return_120s: float | None = None
    net_return_300s: float | None = None
    net_mfe_30s: float | None = None
    net_mfe_120s: float | None = None
    net_mfe_300s: float | None = None
    net_mae_30s: float | None = None
    net_mae_120s: float | None = None
    net_mae_300s: float | None = None
    down20_before_up20: bool | None = None
    down40_before_up30: bool | None = None
    up20_before_down10: bool | None = None
    up50_before_down20: bool | None = None
    up100_before_down30: bool | None = None
    forward_snapshots_observed: int = 0
    forward_window_censored: bool = False


def price_of(row: dict[str, Any]) -> float:
    """Same units as `execution_results.execution_price_e9`
    (`pump_math::mid_price_e9 = quote_reserve * 1_000_000 * 1_000_000_000 / base_reserve` —
    the extra 1e6 corrects for the base token's 6 decimals vs quote's 9). Comparing a raw
    quote/base ratio straight to `*_price_e9` without this factor makes every forward price
    look ~1e6x smaller than entry, i.e. every entry looks like it went to zero — caught by
    spot-checking an early run where every successful row showed net_return_300s ~= -0.999999."""
    base = _num(row["effective_base_atomic"])
    quote = _num(row["effective_quote_atomic"])
    if base <= 0:
        return 0.0
    return quote * 1_000_000.0 * 1_000_000_000.0 / base


def fetch_forward_path(ch_client: Any, mint: str, from_ts: datetime, horizon_s: int) -> list[dict[str, Any]]:
    query = """
        SELECT slot, observed_at, effective_base_atomic, effective_quote_atomic
        FROM market_snapshots_buffer
        WHERE mint = {mint:String} AND observed_at >= {from_ts:DateTime64(6)}
          AND observed_at <= {to_ts:DateTime64(6)}
        ORDER BY observed_at ASC
    """
    to_ts = from_ts.timestamp() + horizon_s
    result = ch_client.query(
        query,
        parameters={
            "mint": mint,
            "from_ts": from_ts,
            "to_ts": datetime.fromtimestamp(to_ts, tz=timezone.utc),
        },
    )
    cols = result.column_names
    rows = [dict(zip(cols, row)) for row in result.result_rows]
    # clickhouse_connect returns naive datetimes for DateTime64; the column is UTC by convention
    # everywhere else in this pipeline (collector writes DateTime64(6,'UTC')), so stamp it rather
    # than silently comparing naive-vs-aware against entry_time below.
    for row in rows:
        if row["observed_at"].tzinfo is None:
            row["observed_at"] = row["observed_at"].replace(tzinfo=timezone.utc)
    return rows


def compute_forward_labels(path: list[dict[str, Any]], entry_price: float, now: datetime, entry_time: datetime) -> ForwardLabels:
    labels = ForwardLabels(forward_snapshots_observed=len(path))
    if entry_price <= 0 or not path:
        labels.forward_window_censored = (now - entry_time).total_seconds() < HORIZONS_SECONDS[-1]
        return labels
    max_observed_s = (path[-1]["observed_at"] - entry_time).total_seconds()
    labels.forward_window_censored = max_observed_s < HORIZONS_SECONDS[-1] and (
        now - entry_time
    ).total_seconds() < HORIZONS_SECONDS[-1]

    def prices_within(seconds: int) -> list[float]:
        return [
            price_of(row) / entry_price - 1.0
            for row in path
            if (row["observed_at"] - entry_time).total_seconds() <= seconds
        ]

    for s in HORIZONS_SECONDS:
        rets = prices_within(s)
        if not rets:
            continue
        setattr(labels, f"net_return_{s}s", rets[-1])
        if s in (30, 120, 300):
            setattr(labels, f"net_mfe_{s}s", max(rets))
            setattr(labels, f"net_mae_{s}s", min(rets))

    ordered = prices_within(HORIZONS_SECONDS[-1])
    if ordered:

        def first_cross(down_pct: float, up_pct: float) -> bool | None:
            for r in ordered:
                if r <= -down_pct:
                    return True
                if r >= up_pct:
                    return False
            return None

        labels.down20_before_up20 = first_cross(0.20, 0.20)
        labels.down40_before_up30 = first_cross(0.40, 0.30)

        def first_cross_up(up_pct: float, down_pct: float) -> bool | None:
            for r in ordered:
                if r >= up_pct:
                    return True
                if r <= -down_pct:
                    return False
            return None

        labels.up20_before_down10 = first_cross_up(0.20, 0.10)
        labels.up50_before_down20 = first_cross_up(0.50, 0.20)
        labels.up100_before_down30 = first_cross_up(1.00, 0.30)
    return labels


def entry_cost_bps(row: dict[str, Any]) -> float:
    requested = _num(row["requested_quote"])
    if requested <= 0:
        return 0.0
    fee_lamports = (
        _num(row["network_cost_charged_lamports"])
        + _num(row["protocol_fee_quote"])
        + _num(row["creator_fee_quote"])
        + _num(row["lp_fee_quote"])
        + _num(row["cashback_fee_quote"])
    )
    return fee_lamports / requested * 10_000.0


def entry_cost_bps_from_replay_result(result: dict[str, Any]) -> float:
    """Same cost-bps definition as `entry_cost_bps`, but for a `/execution/replay` response
    body's `result` object, which nests fees under `protocol_fees` and network cost under a
    flat `network_cost_charged_lamports` rather than the flat Postgres row column layout."""
    requested = _num(result.get("requested_quote"))
    if requested <= 0:
        return 0.0
    fees = result.get("protocol_fees") or {}
    fee_lamports = (
        _num(result.get("network_cost_charged_lamports"))
        + _num(fees.get("protocol_fee_quote"))
        + _num(fees.get("creator_fee_quote"))
        + _num(fees.get("lp_fee_quote"))
        + _num(fees.get("cashback_fee_quote"))
    )
    return fee_lamports / requested * 10_000.0


def fetch_scenario_robustness(http_client: Any, core_api_url: str, trace_id: str) -> dict[str, Any]:
    out: dict[str, Any] = {}
    for scenario in SCENARIOS:
        try:
            resp = http_client.post(
                f"{core_api_url}/api/v1/execution/replay/{trace_id}",
                json={"scenario": scenario},
                timeout=10.0,
            )
            body = resp.json()
        except Exception as exc:  # noqa: BLE001 - degrade to unresolved, never fail the row
            out[scenario] = {"resolved": False, "reason": str(exc)}
            continue
        result = body.get("result", {})
        out[scenario] = {
            "resolved": bool(body.get("historical_state_resolved")),
            "success": result.get("success"),
            "failure_class": result.get("failure_class"),
            "entry_cost_bps": entry_cost_bps_from_replay_result(result)
            if result.get("success")
            else None,
        }
    return out


def robustness_summary(scenario_rows: dict[str, Any], base_cost_bps: float) -> dict[str, Any]:
    resolved_costs = [
        row["entry_cost_bps"]
        for row in scenario_rows.values()
        if row.get("resolved") and row.get("success") and row.get("entry_cost_bps") is not None
    ]
    if not resolved_costs:
        return {
            "expected_entry_cost_bps": base_cost_bps,
            "worst_reasonable_entry_cost_bps": base_cost_bps,
            "execution_robustness": "UNRESOLVED",
        }
    all_costs = [base_cost_bps, *resolved_costs]
    worst = max(all_costs)
    return {
        "expected_entry_cost_bps": statistics.fmean(all_costs),
        "worst_reasonable_entry_cost_bps": worst,
        # Cost-robustness only (see module docstring) — not a claim about round-trip P&L.
        "execution_robustness": "ROBUST" if worst - base_cost_bps < 50 else "FRAGILE",
    }


def build_row(pg_row: dict[str, Any], ch_client: Any, http_client: Any | None, core_api_url: str) -> dict[str, Any]:
    success = bool(pg_row["success"])
    cost_bps = entry_cost_bps(pg_row)
    now = datetime.now(timezone.utc)
    if success:
        # Raw execution_price_e9, matching price_of()'s scale exactly — a ratio computation
        # only needs both sides in the SAME units, not human-readable SOL-per-token.
        entry_price = _num(pg_row["execution_price_e9"])
        entry_time = pg_row["created_at"]
        path = fetch_forward_path(ch_client, pg_row["mint"], entry_time, HORIZONS_SECONDS[-1])
        forward = compute_forward_labels(path, entry_price, now, entry_time)
        position_taken = True
    else:
        # No swap output is ever credited on failure (docs/EXECUTION_TWIN.md): the only real
        # outcome is the fee drag, applied uniformly across every horizon. No fabricated price
        # exposure for a position that was never actually opened.
        guaranteed_return = -cost_bps / 10_000.0
        forward = ForwardLabels(
            net_return_30s=guaranteed_return,
            net_return_60s=guaranteed_return,
            net_return_120s=guaranteed_return,
            net_return_300s=guaranteed_return,
            net_mfe_30s=guaranteed_return,
            net_mfe_120s=guaranteed_return,
            net_mfe_300s=guaranteed_return,
            net_mae_30s=guaranteed_return,
            net_mae_120s=guaranteed_return,
            net_mae_300s=guaranteed_return,
        )
        position_taken = False

    scenario_rows: dict[str, Any] = {}
    robustness: dict[str, Any] = {"execution_robustness": "NOT_ATTEMPTED"}
    if http_client is not None:
        scenario_rows = fetch_scenario_robustness(http_client, core_api_url, pg_row["trace_id"])
        robustness = robustness_summary(scenario_rows, cost_bps)

    return {
        "result_id": pg_row["result_id"],
        "intent_id": pg_row["intent_id"],
        "trace_id": pg_row["trace_id"],
        "decision_id": pg_row["decision_id"],
        "experiment_id": pg_row["experiment_id"],
        "strategy_version": pg_row["strategy_version"],
        "mint": pg_row["mint"],
        "market": pg_row["market"],
        "decision_at": pg_row["decision_at"].isoformat(),
        "decision_slot": int(pg_row["decision_slot"]),
        "entry_success": success,
        "entry_failure_class": pg_row["failure_class"],
        "position_taken": position_taken,
        "entry_execution_cost_bps": cost_bps,
        "entry_price_impact_bps": pg_row["price_impact_bps"],
        "entry_adverse_selection_bps": pg_row["adverse_move_bps"],
        "net_return_basis": "entry_cost_only_no_simulated_exit",
        **{k: v for k, v in asdict(forward).items()},
        "scenario_replays": scenario_rows,
        **robustness,
        "reproducibility": {
            "snapshot_id": pg_row["decision_snapshot_id"],
            "cutoff_slot": int(pg_row["decision_slot"]),
            "trace_id": pg_row["trace_id"],
            "simulator_version": pg_row["simulator_version"],
            "fee_model_version": FEE_MODEL_VERSION,
            "landing_model_version": LANDING_MODEL_VERSION,
            "feature_version": pg_row["feature_snapshot_id"],
        },
    }


def leakage_audit(rows: list[dict[str, Any]]) -> dict[str, Any]:
    issues: list[str] = []
    missing_feature_snapshot = sum(1 for r in rows if not r["reproducibility"]["feature_version"])
    if missing_feature_snapshot:
        issues.append(f"{missing_feature_snapshot} rows have no exact feature_snapshot_id join")
    censored = sum(1 for r in rows if r.get("forward_window_censored"))
    non_terminal_failures = sum(
        1 for r in rows if not r["entry_success"] and r["entry_failure_class"] is None
    )
    if non_terminal_failures:
        issues.append(f"{non_terminal_failures} failed rows carry no failure_class")
    return {
        "total_rows": len(rows),
        "censored_forward_window_rows": censored,
        "note": "censored rows have not yet reached their full 5-minute forward window as of "
        "generation time; they carry partial labels and forward_window_censored=true rather "
        "than a fabricated complete one.",
        "issues": issues,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", default="artifacts/datasets/post_cost_entries.jsonl")
    parser.add_argument("--core-api-url", default="http://127.0.0.1:8080")
    parser.add_argument(
        "--skip-scenarios",
        action="store_true",
        help="Skip FAST/SLOW/STRESSED replay calls (faster, entry-cost robustness omitted).",
    )
    args = parser.parse_args()

    dsn = os.environ.get("PUMPHOUND_POSTGRES_DSN")
    if not dsn:
        raise SystemExit("PUMPHOUND_POSTGRES_DSN_required")
    ch_url = os.environ.get("PUMPHOUND_CLICKHOUSE_URL", "http://127.0.0.1:8123")
    ch_user = os.environ.get("PUMPHOUND_CLICKHOUSE_USER", "default")
    ch_password = os.environ.get("PUMPHOUND_CLICKHOUSE_PASSWORD", "")

    import clickhouse_connect
    import httpx
    import psycopg
    from urllib.parse import urlparse

    parsed = urlparse(ch_url)
    ch_client = clickhouse_connect.get_client(
        host=parsed.hostname or "127.0.0.1",
        port=parsed.port or 8123,
        username=ch_user,
        password=ch_password,
        database="pumphound",
    )
    http_client = None if args.skip_scenarios else httpx.Client()

    with psycopg.connect(dsn) as conn, conn.cursor() as cur:
        cur.execute(ENTRY_QUERY)
        col_names = [d.name for d in cur.description]
        pg_rows = [dict(zip(col_names, row)) for row in cur.fetchall()]

    rows = [build_row(r, ch_client, http_client, args.core_api_url) for r in pg_rows]
    audit = leakage_audit(rows)

    os.makedirs(os.path.dirname(args.out) or ".", exist_ok=True)
    with open(args.out, "w") as fh:
        for row in rows:
            fh.write(json.dumps(row, default=str) + "\n")

    manifest = {
        "dataset_version": DATASET_VERSION,
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "row_count": len(rows),
        "entry_success_count": sum(1 for r in rows if r["entry_success"]),
        "entry_failure_count": sum(1 for r in rows if not r["entry_success"]),
        "scenarios_attempted": http_client is not None,
        "fee_model_version": FEE_MODEL_VERSION,
        "landing_model_version": LANDING_MODEL_VERSION,
        "leakage_audit": audit,
        "output_path": args.out,
    }
    manifest_path = args.out.replace(".jsonl", ".manifest.json")
    with open(manifest_path, "w") as fh:
        json.dump(manifest, fh, indent=2, default=str)

    print(json.dumps(manifest, indent=2, default=str))


if __name__ == "__main__":
    main()
