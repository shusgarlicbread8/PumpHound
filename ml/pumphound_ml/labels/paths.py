"""Execution-aware path labels: net return, MFE/MAE, exit hazard."""

from __future__ import annotations

from collections.abc import Iterable
from datetime import datetime, timedelta

from pumphound_ml.labels.censoring import apply_censoring
from pumphound_ml.types import LabelRecord

RETURN_HORIZONS = (
    ("net_return_30s_v1", 30),
    ("net_return_1m_v1", 60),
    ("net_return_2m_v1", 120),
    ("net_return_5m_v1", 300),
    ("net_return_15m_v1", 900),
    ("net_return_1h_v1", 3600),
    ("net_return_6h_v1", 21600),
    ("net_return_24h_v1", 86400),
)


def _value_at(path: list[tuple[datetime, float]], when: datetime) -> float | None:
    last = None
    for ts, value in path:
        if ts <= when:
            last = value
        else:
            break
    return last


def path_labels(
    path: Iterable[tuple[datetime, float]],
    *,
    launch_id: str,
    cutoff: datetime,
    cutoff_slot: int,
    observation_window_seconds: int,
    observed_until: datetime | None,
    no_route: bool = False,
) -> list[LabelRecord]:
    series = [(ts, v) for ts, v in path if ts >= cutoff]
    out: list[LabelRecord] = []
    if no_route:
        for name, horizon in RETURN_HORIZONS:
            out.append(
                LabelRecord(
                    launch_id=launch_id,
                    cutoff_slot=cutoff_slot,
                    observation_window_seconds=observation_window_seconds,
                    label=name,
                    value=None,
                    status="censored",
                    horizon_seconds=horizon,
                    matured_at=None,
                    attributes={"reason": "no_route", "not_zero_fill": True},
                )
            )
        return out

    entry = _value_at(series, cutoff)
    if entry is None and series:
        entry = series[0][1]
    for name, horizon in RETURN_HORIZONS:
        target = cutoff + timedelta(seconds=horizon)
        decision = apply_censoring(
            observed_until=observed_until,
            required_until=target,
            positive=None,
        )
        value = None
        status = decision.status
        if observed_until and observed_until >= target and entry not in (None, 0):
            mark = _value_at(series, target)
            if mark is None:
                status = "censored"
            else:
                value = mark / entry - 1.0 if entry else None
                status = "matured"
        out.append(
            LabelRecord(
                launch_id=launch_id,
                cutoff_slot=cutoff_slot,
                observation_window_seconds=observation_window_seconds,
                label=name,
                value=value,
                status=status,
                horizon_seconds=horizon,
                matured_at=target if status == "matured" else None,
            )
        )

    after = [(ts, v) for ts, v in series if ts >= cutoff]
    if after and entry not in (None, 0):
        mfe = max(v / entry - 1.0 for _, v in after)
        mae = min(v / entry - 1.0 for _, v in after)
        peak_t = max(after, key=lambda x: x[1])[0]
        trough_t = min(after, key=lambda x: x[1])[0]
        out.append(
            LabelRecord(
                launch_id=launch_id,
                cutoff_slot=cutoff_slot,
                observation_window_seconds=observation_window_seconds,
                label="mfe_v1",
                value=mfe,
                status="matured",
                horizon_seconds=0,
                matured_at=peak_t,
            )
        )
        out.append(
            LabelRecord(
                launch_id=launch_id,
                cutoff_slot=cutoff_slot,
                observation_window_seconds=observation_window_seconds,
                label="mae_v1",
                value=mae,
                status="matured",
                horizon_seconds=0,
                matured_at=trough_t,
            )
        )
    return out


def short_horizon_labels(
    path: Iterable[tuple[datetime, float]],
    *,
    launch_id: str,
    cutoff: datetime,
    cutoff_slot: int,
    observation_window_seconds: int,
    observed_until: datetime | None,
) -> list[LabelRecord]:
    """Labels that mature in minutes from a live or historical price path."""
    series = [(ts, value) for ts, value in path if ts >= cutoff]
    out: list[LabelRecord] = []
    windows = (
        ("return_next_30s_v1", 30, "return"),
        ("return_next_60s_v1", 60, "return"),
        ("return_next_2m_v1", 120, "return"),
        ("return_next_5m_v1", 300, "return"),
        ("max_upside_next_2m_v1", 120, "mfe"),
        ("max_drawdown_next_2m_v1", 120, "mae"),
        ("short_momentum_2m_v1", 120, "momentum"),
        ("adverse_move_2m_v1", 120, "adverse"),
        ("expected_mfe_5m_v1", 300, "mfe_hit"),
        ("down_20_within_60s_v1", 60, "crash20"),
        ("down_30_within_2m_v1", 120, "crash30"),
    )
    if not series:
        for name, horizon, _kind in windows:
            out.append(
                LabelRecord(
                    launch_id=launch_id,
                    cutoff_slot=cutoff_slot,
                    observation_window_seconds=observation_window_seconds,
                    label=name,
                    value=None,
                    status="censored",
                    horizon_seconds=horizon,
                    matured_at=None,
                    attributes={"reason": "empty_path"},
                )
            )
        return out

    entry = series[0][1]
    for name, horizon, kind in windows:
        deadline = cutoff + timedelta(seconds=horizon)
        decision = apply_censoring(
            observed_until=observed_until,
            required_until=deadline,
            positive=None,
        )
        window = [(ts, value) for ts, value in series if ts <= deadline]
        if not window or observed_until is None or observed_until < deadline:
            out.append(
                LabelRecord(
                    launch_id=launch_id,
                    cutoff_slot=cutoff_slot,
                    observation_window_seconds=observation_window_seconds,
                    label=name,
                    value=None,
                    status="censored",
                    horizon_seconds=horizon,
                    matured_at=None,
                )
            )
            continue
        returns = [
            (value / entry - 1.0) if entry not in (None, 0) else None for _, value in window
        ]
        returns = [value for value in returns if value is not None]
        if not returns:
            status = "censored"
            value = None
        elif kind == "return":
            value = float(returns[-1])
            status = "matured"
        elif kind == "mfe":
            value = float(max(returns))
            status = "matured"
        elif kind == "mae":
            value = float(min(returns))
            status = "matured"
        elif kind == "momentum":
            value = 1.0 if returns[-1] > 0 else 0.0
            status = "positive" if value == 1.0 else "negative"
        elif kind == "adverse":
            value = 1.0 if min(returns) <= -0.10 else 0.0
            status = "positive" if value == 1.0 else "negative"
        elif kind == "mfe_hit":
            value = 1.0 if max(returns) >= 0.10 else 0.0
            status = "positive" if value == 1.0 else "negative"
        elif kind == "crash20":
            value = 1.0 if min(returns) <= -0.20 else 0.0
            status = "positive" if value == 1.0 else "negative"
        elif kind == "crash30":
            value = 1.0 if min(returns) <= -0.30 else 0.0
            status = "positive" if value == 1.0 else "negative"
        else:
            value = None
            status = decision.status
        out.append(
            LabelRecord(
                launch_id=launch_id,
                cutoff_slot=cutoff_slot,
                observation_window_seconds=observation_window_seconds,
                label=name,
                value=value,
                status=status,
                horizon_seconds=horizon,
                matured_at=deadline if status in {"matured", "positive", "negative"} else None,
            )
        )
    return out


def exit_risk_label(
    path: Iterable[tuple[datetime, float]],
    *,
    launch_id: str,
    cutoff: datetime,
    cutoff_slot: int,
    observation_window_seconds: int,
    observed_until: datetime | None,
    drawdown: float = 0.20,
    recovery: float = 0.10,
    horizon_seconds: int = 60,
) -> LabelRecord:
    """Objective exit-risk: drawdown barrier is reached before recovery barrier."""
    deadline = cutoff + timedelta(seconds=horizon_seconds)
    hit_down: datetime | None = None
    hit_up: datetime | None = None
    for ts, value in path:
        if ts <= cutoff or ts > deadline:
            continue
        if value <= -abs(drawdown) and hit_down is None:
            hit_down = ts
        if value >= abs(recovery) and hit_up is None:
            hit_up = ts
        if hit_down is not None or hit_up is not None:
            break
    positive = None
    if hit_down is not None and (hit_up is None or hit_down < hit_up):
        positive = True
    elif hit_up is not None and (hit_down is None or hit_up < hit_down) or observed_until is not None and observed_until >= deadline:
        positive = False
    decision = apply_censoring(
        observed_until=observed_until,
        required_until=deadline,
        positive=positive,
    )
    return LabelRecord(
        launch_id=launch_id,
        cutoff_slot=cutoff_slot,
        observation_window_seconds=observation_window_seconds,
        label="exit_hazard_60s_v1",
        value=decision.value,
        status=decision.status,
        horizon_seconds=horizon_seconds,
        matured_at=deadline if decision.status in {"positive", "negative"} else None,
        attributes={
            "definition": "drawdown_before_recovery",
            "drawdown": drawdown,
            "recovery": recovery,
            "hit_down": hit_down.isoformat() if hit_down else None,
            "hit_up": hit_up.isoformat() if hit_up else None,
        },
    )
