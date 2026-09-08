"""Challengers stay shadow-only until an explicit promotion with a reason."""

from __future__ import annotations

from datetime import datetime, timezone
from typing import Any

ALLOWED = ("candidate", "shadow", "champion", "retired", "rejected")


def can_promote(metrics: dict[str, Any], *, train_only: bool = False) -> tuple[bool, str]:
    if train_only or metrics.get("split") == "train":
        return False, "NO_PROMOTION_ON_TRAIN_METRICS"
    if not metrics.get("held_out_report"):
        return False, "held_out_report_required"
    if not metrics.get("compatible_feature_schema"):
        return False, "incompatible_feature_schema"
    if not metrics.get("reason"):
        return False, "explicit_reason_required"
    return True, "ok"


def transition(
    current: str,
    target: str,
    metrics: dict[str, Any],
    *,
    reason: str,
) -> dict[str, Any]:
    if target not in ALLOWED or current not in ALLOWED:
        raise ValueError("invalid registry status")
    ok, code = can_promote({**metrics, "reason": reason}, train_only=False)
    if target == "champion" and not ok:
        raise ValueError(code)
    return {
        "from": current,
        "to": target,
        "reason": reason,
        "at": datetime.now(timezone.utc).isoformat(),
        "automatic": False,
    }
