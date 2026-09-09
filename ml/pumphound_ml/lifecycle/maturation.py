"""Idempotent, due-time-gated label maturation for live feature snapshots."""

from __future__ import annotations

import uuid
from collections.abc import Callable, Iterable
from dataclasses import dataclass
from datetime import UTC, datetime, timedelta
from typing import Any

from pumphound_ml.io import sha256_canonical
from pumphound_ml.types import parse_ts

TARGET_SPECS: dict[str, tuple[int, str]] = {
    "return_next_60s_v1": (60, "return"),
    "down_20_within_60s_v1": (60, "crash20"),
    "return_next_2m_v1": (120, "return"),
    "max_upside_next_2m_v1": (120, "mfe"),
    "max_drawdown_next_2m_v1": (120, "mae"),
    "short_momentum_2m_v1": (120, "momentum"),
    "adverse_move_2m_v1": (120, "adverse"),
    "down_30_within_2m_v1": (120, "crash30"),
    "return_next_5m_v1": (300, "return"),
    "expected_mfe_5m_v1": (300, "mfe_hit"),
}

SCHEDULE_SQL = """
INSERT INTO pumphound.outcome_labels (
    outcome_label_id,
    feature_snapshot_id,
    launch_id,
    target_version,
    horizon_seconds,
    maturity_due_at,
    status,
    source_cutoff,
    payload
)
SELECT
    fs.feature_snapshot_id || ':' || spec.target_version,
    fs.feature_snapshot_id,
    fs.launch_id,
    spec.target_version,
    spec.horizon_seconds,
    fs.knowledge_cutoff + make_interval(secs => spec.horizon_seconds),
    'PENDING',
    fs.knowledge_cutoff,
    jsonb_build_object(
        'feature_schema_version', fs.schema_version,
        'exact_feature_snapshot_id', fs.feature_snapshot_id
    )
FROM pumphound.feature_snapshots AS fs
JOIN pumphound.simulation_runs AS run
  ON run.simulation_run_id = fs.simulation_run_id
CROSS JOIN (VALUES
    ('return_next_60s_v1', 60),
    ('down_20_within_60s_v1', 60),
    ('return_next_2m_v1', 120),
    ('max_upside_next_2m_v1', 120),
    ('max_drawdown_next_2m_v1', 120),
    ('short_momentum_2m_v1', 120),
    ('adverse_move_2m_v1', 120),
    ('down_30_within_2m_v1', 120),
    ('return_next_5m_v1', 300),
    ('expected_mfe_5m_v1', 300)
) AS spec(target_version, horizon_seconds)
WHERE fs.knowledge_cutoff <= %s
  AND fs.knowledge_cutoff >= %s
  AND run.manifest->>'experiment_id' = 'ALPHA-LEARN'
ON CONFLICT (feature_snapshot_id, target_version) DO NOTHING
"""

# Scheduling is idempotent (ON CONFLICT DO NOTHING) and runs every 30 s. Without a lower bound it
# re-derived 10 candidate rows for every feature snapshot ever written (72k snapshots -> 720k
# conflict checks per run) and was the single largest steady-state Postgres CPU consumer,
# starving the collector's inserts. Snapshots older than this window were scheduled by an
# earlier run; the one-off backfill path (`schedule_lookback=None`) still covers full history.
SCHEDULE_LOOKBACK_SECONDS = 6 * 3600

CLAIM_DUE_SQL = """
SELECT
    ol.outcome_label_id,
    ol.feature_snapshot_id,
    ol.launch_id,
    ol.target_version,
    ol.horizon_seconds,
    ol.maturity_due_at,
    ol.source_cutoff,
    fs.values
FROM pumphound.outcome_labels AS ol
JOIN pumphound.feature_snapshots AS fs
  ON fs.feature_snapshot_id = ol.feature_snapshot_id
WHERE ol.status = 'PENDING'
  AND ol.maturity_due_at <= %s
ORDER BY ol.maturity_due_at, ol.outcome_label_id
FOR UPDATE OF ol SKIP LOCKED
LIMIT %s
"""

TAPE_SQL = """
SELECT knowledge_cutoff, values
FROM pumphound.feature_snapshots
WHERE launch_id = %s
  AND knowledge_cutoff >= %s
  AND knowledge_cutoff <= %s
ORDER BY knowledge_cutoff, feature_snapshot_id
"""

MATURE_SQL = """
UPDATE pumphound.outcome_labels
SET matured_at = %s,
    value = %s,
    status = %s,
    source_hash = %s,
    payload = payload || %s
WHERE outcome_label_id = %s
  AND status = 'PENDING'
  AND maturity_due_at <= %s
"""

LABEL_LIFECYCLE_SQL = """
INSERT INTO pumphound.candidate_lifecycle_events (
    lifecycle_event_id,
    idempotency_key,
    experiment_id,
    launch_id,
    feature_snapshot_id,
    state,
    reason_code,
    retryable,
    chain_creation_at,
    first_observed_at,
    snapshot_at,
    occurred_at,
    payload
)
SELECT
    %s,
    'label-matured:' || ol.outcome_label_id,
    'ALPHA-LEARN',
    ol.launch_id,
    ol.feature_snapshot_id,
    'LABEL_MATURED',
    %s,
    FALSE,
    launch.creation_time,
    launch.known_at,
    fs.computed_at,
    %s,
    %s
FROM pumphound.outcome_labels AS ol
JOIN pumphound.launches AS launch ON launch.launch_id = ol.launch_id
JOIN pumphound.feature_snapshots AS fs
  ON fs.feature_snapshot_id = ol.feature_snapshot_id
WHERE ol.outcome_label_id = %s
  AND ol.status = 'MATURED'
ON CONFLICT (idempotency_key) DO NOTHING
"""


@dataclass(frozen=True)
class LabelOutcome:
    value: float | None
    status: str
    source_hash: str
    payload: dict[str, Any]


def derive_label(
    target_version: str,
    *,
    source_cutoff: datetime,
    maturity_due_at: datetime,
    observed_until: datetime | None,
    tape: Iterable[tuple[datetime, float]],
) -> LabelOutcome:
    """Derive a target only after its complete horizon has been observed."""
    if observed_until is None or observed_until < maturity_due_at:
        raise ValueError("label_not_due_or_tape_incomplete")
    spec = TARGET_SPECS.get(target_version)
    if spec is None:
        raise ValueError(f"unsupported_target:{target_version}")
    horizon, kind = spec
    expected_due = source_cutoff + timedelta(seconds=horizon)
    if maturity_due_at != expected_due:
        raise ValueError("maturity_due_mismatch")

    points = sorted(
        {
            timestamp: float(price)
            for timestamp, price in tape
            if source_cutoff <= timestamp <= maturity_due_at and float(price) > 0
        }.items()
    )
    source_hash = sha256_canonical(
        [(timestamp.isoformat(), price) for timestamp, price in points]
    )
    if not points:
        return LabelOutcome(
            value=None,
            status="WITHHELD",
            source_hash=source_hash,
            payload={"reason": "market_tape_missing", "point_count": 0},
        )
    entry = points[0][1]
    returns = [price / entry - 1.0 for _, price in points]
    if kind == "return":
        value = returns[-1]
    elif kind == "mfe":
        value = max(returns)
    elif kind == "mae":
        value = min(returns)
    elif kind == "momentum":
        value = float(returns[-1] > 0)
    elif kind == "adverse":
        value = float(min(returns) <= -0.10)
    elif kind == "mfe_hit":
        value = float(max(returns) >= 0.10)
    elif kind == "crash20":
        value = float(min(returns) <= -0.20)
    elif kind == "crash30":
        value = float(min(returns) <= -0.30)
    else:  # pragma: no cover - guarded by TARGET_SPECS
        raise ValueError(f"unsupported_label_kind:{kind}")
    return LabelOutcome(
        value=float(value),
        status="MATURED",
        source_hash=source_hash,
        payload={
            "point_count": len(points),
            "first_market_time": points[0][0].isoformat(),
            "last_market_time": points[-1][0].isoformat(),
            "definition": kind,
        },
    )


def _price_from_mapping(values: dict[str, Any]) -> float | None:
    for key in (
        "executable_price",
        "mark_price",
        "mid_price",
        "curve_price",
        "price",
    ):
        raw = values.get(key)
        try:
            price = float(raw)
        except (TypeError, ValueError):
            continue
        if price > 0:
            return price
    return None


def _tape_points(rows: Iterable[tuple[Any, Any]]) -> list[tuple[datetime, float]]:
    points: list[tuple[datetime, float]] = []
    for raw_time, raw_values in rows:
        values = raw_values if isinstance(raw_values, dict) else {}
        for item in values.get("market_tape") or values.get("tape") or []:
            if not isinstance(item, dict):
                continue
            timestamp = parse_ts(
                item.get("observed_at") or item.get("timestamp") or item.get("time")
            )
            price = _price_from_mapping(item)
            if timestamp is not None and price is not None:
                points.append((timestamp, price))
        timestamp = parse_ts(raw_time)
        price = _price_from_mapping(values)
        if timestamp is not None and price is not None:
            points.append((timestamp, price))
    return points


class PostgresLabelMaturer:
    def __init__(
        self,
        dsn: str,
        *,
        connect: Callable[[str], Any] | None = None,
        json_adapter: Callable[[dict[str, Any]], Any] | None = None,
    ) -> None:
        if not dsn:
            raise ValueError("PUMPHOUND_POSTGRES_DSN_required")
        if connect is None:
            import psycopg

            connect = psycopg.connect
        if json_adapter is None:
            from psycopg.types.json import Jsonb

            json_adapter = Jsonb
        self.dsn = dsn
        self.connect = connect
        self.json_adapter = json_adapter

    def run(
        self,
        *,
        now: datetime | None = None,
        limit: int = 500,
        grace_period: timedelta = timedelta(seconds=60),
    ) -> dict[str, int]:
        now = now or datetime.now(UTC)
        scheduled = 0
        matured = 0
        withheld = 0
        tape_cache: dict[
            tuple[Any, Any], tuple[list[tuple[datetime, float]], datetime | None]
        ] = {}
        with (
            self.connect(self.dsn) as connection,
            connection.transaction(),
            connection.cursor() as cursor,
        ):
            cursor.execute(
                SCHEDULE_SQL,
                (now, now - timedelta(seconds=SCHEDULE_LOOKBACK_SECONDS)),
            )
            scheduled = max(int(cursor.rowcount or 0), 0)
            while matured + withheld < limit:
                chunk_limit = min(2000, limit - (matured + withheld))
                cursor.execute(CLAIM_DUE_SQL, (now, int(chunk_limit)))
                due_rows = list(cursor.fetchall())
                if not due_rows:
                    break
                updated_this_chunk = 0
                for row in due_rows:
                    (
                        label_id,
                        feature_snapshot_id,
                        launch_id,
                        target,
                        _horizon,
                        maturity_due,
                        source_cutoff,
                        source_values,
                    ) = row
                    cache_key = (launch_id, source_cutoff)
                    if cache_key not in tape_cache:
                        cursor.execute(TAPE_SQL, (launch_id, source_cutoff, now))
                        tape_rows = [(source_cutoff, source_values), *cursor.fetchall()]
                        tape_points = _tape_points(tape_rows)
                        coverage_times = [
                            timestamp
                            for raw_time, _values in tape_rows
                            if (timestamp := parse_ts(raw_time)) is not None
                        ]
                        coverage_times.extend(
                            timestamp for timestamp, _price in tape_points
                        )
                        observed_until = max(coverage_times) if coverage_times else None
                        tape_cache[cache_key] = (tape_points, observed_until)
                    else:
                        tape_points, observed_until = tape_cache[cache_key]

                    if observed_until is None or observed_until < maturity_due:
                        if now < maturity_due + grace_period:
                            continue
                        source_hash = sha256_canonical(
                            [
                                (
                                    "observed_until",
                                    (
                                        observed_until.isoformat()
                                        if observed_until
                                        else None
                                    ),
                                ),
                                ("point_count", len(tape_points)),
                            ]
                        )
                        outcome = LabelOutcome(
                            value=None,
                            status="WITHHELD",
                            source_hash=source_hash,
                            payload={
                                "reason": "coverage_incomplete",
                                "observed_until": (
                                    observed_until.isoformat()
                                    if observed_until
                                    else None
                                ),
                                "maturity_due_at": (
                                    maturity_due.isoformat()
                                    if hasattr(maturity_due, "isoformat")
                                    else str(maturity_due)
                                ),
                                "point_count": len(tape_points),
                            },
                        )
                    else:
                        outcome = derive_label(
                            str(target),
                            source_cutoff=source_cutoff,
                            maturity_due_at=maturity_due,
                            observed_until=observed_until,
                            tape=tape_points,
                        )
                    payload = {
                        **outcome.payload,
                        "exact_feature_snapshot_id": str(feature_snapshot_id),
                    }
                    cursor.execute(
                        MATURE_SQL,
                        (
                            now,
                            outcome.value,
                            outcome.status,
                            outcome.source_hash,
                            self.json_adapter(payload),
                            label_id,
                            now,
                        ),
                    )
                    if cursor.rowcount:
                        updated_this_chunk += 1
                        matured += outcome.status == "MATURED"
                        withheld += outcome.status == "WITHHELD"
                        if outcome.status == "MATURED":
                            cursor.execute(
                                LABEL_LIFECYCLE_SQL,
                                (
                                    uuid.uuid4(),
                                    str(target),
                                    now,
                                    self.json_adapter(
                                        {
                                            "outcome_label_id": str(label_id),
                                            "target_version": str(target),
                                            "value": outcome.value,
                                        }
                                    ),
                                    label_id,
                                ),
                            )
                if updated_this_chunk == 0:
                    break
        return {
            "scheduled": scheduled,
            "matured": matured,
            "withheld": withheld,
        }
