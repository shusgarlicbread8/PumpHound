"""Capacity / shared-liquidity notes. Strategies do not share unlimited depth."""

from __future__ import annotations

from typing import Any


def capacity_report(
    trades: list[dict[str, Any]],
    *,
    max_order_reserve_fraction: float = 0.005,
    max_position_reserve_fraction: float = 0.01,
) -> dict[str, Any]:
    breaches = [
        t
        for t in trades
        if float(t.get("order_reserve_fraction") or 0.0) > max_order_reserve_fraction
        or float(t.get("position_reserve_fraction") or 0.0) > max_position_reserve_fraction
    ]
    return {
        "n": len(trades),
        "reserve_fraction_breaches": len(breaches),
        "max_order_reserve_fraction": max_order_reserve_fraction,
        "max_position_reserve_fraction": max_position_reserve_fraction,
        "shared_liquidity": "standalone_runs_only",
    }
