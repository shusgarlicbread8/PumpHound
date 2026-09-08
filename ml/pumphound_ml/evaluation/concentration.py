"""Top-winner removal. Negative results stay visible."""

from __future__ import annotations

from typing import Any


def remove_top_winners(trades: list[dict[str, Any]], *, value_key: str = "net_return") -> dict[str, float]:
    nets = sorted((float(t.get(value_key) or 0.0) for t in trades), reverse=True)
    total = sum(nets)
    n = len(nets)
    top1p = max(1, int(round(n * 0.01))) if n else 0

    def leftover(k: int) -> float:
        if not nets:
            return 0.0
        return sum(nets[k:])

    return {
        "n": float(n),
        "all": float(total),
        "minus_top_3": float(leftover(min(3, n))),
        "minus_top_5": float(leftover(min(5, n))),
        "minus_top_1_percent": float(leftover(min(top1p, n))),
    }
