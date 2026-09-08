"""Feature-family ablations derived from real schema columns and observed values."""

from __future__ import annotations

import math
from typing import Any

ABLATION_NAMES = (
    "market",
    "market_curve",
    "market_creator",
    "market_wallet",
    "market_holders",
    "market_graph",
    "all",
)

CURVE = {
    "age_seconds",
    "curve_progress",
    "round_trip_cost_fraction",
    "entry_impact_bps",
}
CREATOR = {
    "net_creator_flow_30",
    "net_creator_flow_60",
    "net_creator_flow_300",
    "creator_linked_supply_share",
    "creator_graduation_posterior",
    "creator_collapse_posterior",
}
WALLET = {
    "buyer_retention_60",
    "buyer_retention_300",
    "wallet_churn_60",
    "wallet_churn_300",
    "fresh_wallet_share",
    "immediate_funding_share",
    "smart_wallet_retention",
    "wallet_skill_posterior_mean",
    "wallet_skill_effective_sample",
    "history_coverage_fraction",
}
HOLDERS = {
    "holder_top_1_share",
    "holder_top_5_share",
    "holder_top_10_share",
    "holder_hhi",
    "cluster_hhi",
    "independent_cluster_count",
}
GRAPH = {
    "funding_diversity",
    "cohort_recurrence",
    "co_buy_density",
    "weighted_degree_concentration",
    "cohort_modularity",
}


def _observed(rows: list[dict[str, Any]], name: str) -> bool:
    for row in rows:
        value = row.get(name)
        try:
            if value is not None and math.isfinite(float(value)):
                return True
        except (TypeError, ValueError):
            continue
    return False


def feature_ablation_sets(
    feature_names: list[str],
    training_rows: list[dict[str, Any]],
) -> dict[str, dict[str, Any]]:
    """Return progressive market+family sets; unavailable means no observed signal."""
    real = [name for name in feature_names if _observed(training_rows, name)]
    family_owned = CURVE | CREATOR | WALLET | HOLDERS | GRAPH
    market = [name for name in real if name not in family_owned]
    additions = {
        "market": set(),
        "market_curve": CURVE,
        "market_creator": CREATOR,
        "market_wallet": WALLET,
        "market_holders": HOLDERS,
        "market_graph": GRAPH,
    }
    result: dict[str, dict[str, Any]] = {}
    for ablation in ABLATION_NAMES:
        if ablation == "all":
            names = real
            available = bool(names)
            reason = None if available else "no_observed_features"
        else:
            extra = [name for name in real if name in additions[ablation]]
            names = market + extra
            available = bool(market) and (ablation == "market" or bool(extra))
            reason = (
                None
                if available
                else (
                    "base_market_unavailable"
                    if not market
                    else f"{ablation.removeprefix('market_')}_family_unavailable"
                )
            )
        result[ablation] = {
            "available": available,
            "feature_names": names if available else [],
            "reason": reason,
        }
    return result
