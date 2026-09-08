"""History coverage boundaries. Missing history is unknown, not benign."""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime
from typing import Iterable

from pumphound_ml.types import ChainEvent


@dataclass
class WalletCoverage:
    wallet: str
    first_slot: int | None
    last_slot: int | None
    first_known_at: datetime | None
    last_known_at: datetime | None
    event_count: int
    complete_for_scope: bool
    reasons: list[str] = field(default_factory=list)


def coverage_report(
    events: Iterable[ChainEvent],
    wallets: Iterable[str] | None = None,
    *,
    required_lookback_slot: int | None = None,
) -> dict[str, WalletCoverage]:
    wanted = set(wallets or [])
    seen: dict[str, list[ChainEvent]] = {}
    for event in events:
        refs: set[str] = set()
        p = event.payload
        for key in ("trader", "creator", "fee_payer", "source_owner", "destination_owner"):
            if p.get(key):
                refs.add(str(p[key]))
        if wanted and not (refs & wanted):
            continue
        for wallet in refs:
            if wanted and wallet not in wanted:
                continue
            seen.setdefault(wallet, []).append(event)

    report: dict[str, WalletCoverage] = {}
    for wallet, rows in seen.items():
        rows.sort(key=lambda e: e.sort_key())
        first, last = rows[0], rows[-1]
        complete = True
        reasons: list[str] = []
        if required_lookback_slot is not None and first.slot > required_lookback_slot:
            complete = False
            reasons.append("history_starts_after_required_lookback")
        if any(not e.transaction_succeeded for e in rows):
            reasons.append("failed_transactions_present_as_evidence_only")
        report[wallet] = WalletCoverage(
            wallet=wallet,
            first_slot=first.slot,
            last_slot=last.slot,
            first_known_at=first.known_at,
            last_known_at=last.known_at,
            event_count=len(rows),
            complete_for_scope=complete,
            reasons=reasons,
        )
    for wallet in wanted:
        if wallet not in report:
            report[wallet] = WalletCoverage(
                wallet=wallet,
                first_slot=None,
                last_slot=None,
                first_known_at=None,
                last_known_at=None,
                event_count=0,
                complete_for_scope=False,
                reasons=["no_observed_history"],
            )
    return report


def coverage_fraction(report: dict[str, WalletCoverage]) -> float | None:
    if not report:
        return None
    ok = sum(1 for row in report.values() if row.complete_for_scope)
    return ok / len(report)
