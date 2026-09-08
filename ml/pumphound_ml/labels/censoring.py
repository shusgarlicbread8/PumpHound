"""Right-censoring and ambiguous-gap handling."""

from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, timedelta


STATUS_POSITIVE = "positive"
STATUS_NEGATIVE = "negative"
STATUS_CENSORED = "censored"
STATUS_AMBIGUOUS = "ambiguous"


@dataclass
class CensorDecision:
    status: str
    value: int | float | None
    reason: str


def apply_censoring(
    *,
    observed_until: datetime | None,
    required_until: datetime,
    positive: bool | None,
    ambiguous: bool = False,
    coverage_complete: bool = True,
) -> CensorDecision:
    if ambiguous:
        return CensorDecision(STATUS_AMBIGUOUS, None, "ordering_unresolved")
    if not coverage_complete:
        return CensorDecision(STATUS_CENSORED, None, "incomplete_coverage")
    if observed_until is None or observed_until < required_until:
        return CensorDecision(STATUS_CENSORED, None, "horizon_not_observed")
    if positive is None:
        return CensorDecision(STATUS_CENSORED, None, "label_undefined")
    return CensorDecision(STATUS_POSITIVE if positive else STATUS_NEGATIVE, int(positive), "matured")


def required_until(cutoff: datetime, horizon_seconds: int, extra_buffer_seconds: int = 0) -> datetime:
    return cutoff + timedelta(seconds=horizon_seconds + extra_buffer_seconds)
