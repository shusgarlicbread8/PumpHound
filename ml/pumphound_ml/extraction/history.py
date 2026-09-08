"""Targeted creator/early-buyer history at a knowledge cutoff."""

from __future__ import annotations

from collections import defaultdict
from datetime import datetime
from typing import Iterable

from pumphound_ml.clock import event_visible
from pumphound_ml.extraction.coverage import coverage_report
from pumphound_ml.types import ChainEvent


def extract_wallet_history(
    events: Iterable[ChainEvent],
    knowledge_cutoff: datetime,
    cutoff_slot: int,
    wallets: Iterable[str] | None = None,
) -> dict[str, dict]:
    visible = [e for e in events if event_visible(e, cutoff_slot, knowledge_cutoff)]
    wanted = set(wallets or [])
    trades: dict[str, list[dict]] = defaultdict(list)
    creates: dict[str, list[dict]] = defaultdict(list)
    for event in visible:
        p = event.payload
        if event.payload_kind == "trade.executed":
            trader = str(p.get("trader") or "")
            if wanted and trader not in wanted:
                continue
            trades[trader].append(
                {
                    "event_id": event.event_id,
                    "slot": event.slot,
                    "side": p.get("side"),
                    "mint": p.get("mint"),
                    "launch_id": p.get("launch_id"),
                    "quote_amount_atomic": str(p.get("quote_amount_atomic") or "0"),
                    "known_at": event.known_at.isoformat(),
                }
            )
        elif event.payload_kind == "launch.created":
            creator = str(p.get("creator") or "")
            if wanted and creator not in wanted:
                continue
            creates[creator].append(
                {
                    "event_id": event.event_id,
                    "launch_id": p.get("launch_id"),
                    "mint": p.get("mint"),
                    "slot": event.slot,
                }
            )
    cov = coverage_report(visible, wanted or set(trades) | set(creates))
    out: dict[str, dict] = {}
    subjects = wanted or set(trades) | set(creates) | set(cov)
    for wallet in subjects:
        row = cov.get(wallet)
        out[wallet] = {
            "wallet": wallet,
            "trades": trades.get(wallet, []),
            "launches_created": creates.get(wallet, []),
            "complete": bool(row.complete_for_scope) if row else False,
            "coverage_reasons": list(row.reasons) if row else ["no_observed_history"],
            "first_slot": row.first_slot if row else None,
            "last_slot": row.last_slot if row else None,
        }
    return out
