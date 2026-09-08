"""Cost/friction sensitivity. Increasing friction must not improve a cost-adjusted metric by invention."""

from __future__ import annotations

from copy import deepcopy
from typing import Any


SCENARIOS = {
    "base": {"fee_mult": 1.0, "latency_ms": 1000, "fail_rate": 0.0, "slippage_bps": 0},
    "pessimistic": {"fee_mult": 1.5, "latency_ms": 2000, "fail_rate": 0.05, "slippage_bps": 50},
    "latency_stress": {"fee_mult": 1.0, "latency_ms": 4000, "fail_rate": 0.02, "slippage_bps": 25},
}


def apply_cost_scenario(trades: list[dict[str, Any]], scenario: str) -> list[dict[str, Any]]:
    cfg = SCENARIOS[scenario]
    out = []
    for trade in trades:
        row = deepcopy(trade)
        net = float(row.get("net_return") or 0.0)
        fees = float(row.get("fee_fraction") or 0.0) * cfg["fee_mult"]
        slip = cfg["slippage_bps"] / 10_000.0
        adj = net - (fees - float(row.get("fee_fraction") or 0.0)) - slip
        if cfg["fail_rate"] and str(row.get("id") or "")[-1:] in set("ab"):
            adj = -float(row.get("failed_fee_fraction") or 0.0)
            row["status"] = "SIMULATED_TX_FAILURE"
        row["net_return"] = adj
        row["cost_scenario"] = scenario
        out.append(row)
    return out


def sweep_costs(trades: list[dict[str, Any]]) -> dict[str, float]:
    return {name: sum(float(t["net_return"]) for t in apply_cost_scenario(trades, name)) for name in SCENARIOS}
