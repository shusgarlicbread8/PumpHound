"""CLI: python -m pumphound_ml.extraction.dataset <config>"""

from __future__ import annotations

import argparse
import json
import math
import sys
from collections import defaultdict
from datetime import datetime
from pathlib import Path

import polars as pl

from pumphound_ml.clock import cutoff_from_creation, parse_cutoff
from pumphound_ml.evaluation.splits import chronological_splits
from pumphound_ml.extraction.sources import load_canonical_sources
from pumphound_ml.features.extract import (
    event_matches_launch,
    extract_features,
    ordered_feature_names,
)
from pumphound_ml.io import (
    dump_json,
    load_yaml,
    sha256_canonical,
    sha256_file,
)
from pumphound_ml.labels.barriers import first_passage
from pumphound_ml.labels.collapse import severe_collapse
from pumphound_ml.labels.graduation import graduation_labels
from pumphound_ml.labels.paths import exit_risk_label, short_horizon_labels
from pumphound_ml.types import (
    OBSERVATION_WINDOWS,
    ChainEvent,
    parse_atomic,
    to_plain,
)

FEATURE_ID_SCHEMA = {
    "launch_id": pl.String,
    "mint": pl.String,
    "creator": pl.String,
    "cluster_family": pl.String,
    "observation_window_seconds": pl.Int64,
    "cutoff_slot": pl.Int64,
    "cutoff_time": pl.String,
    "feature_snapshot_id": pl.String,
    "quality_grade": pl.String,
    "split": pl.String,
}
LABEL_SCHEMA = {
    "launch_id": pl.String,
    "feature_snapshot_id": pl.String,
    "cutoff_slot": pl.Int64,
    "observation_window_seconds": pl.Int64,
    "label": pl.String,
    "value": pl.Float64,
    "status": pl.String,
    "horizon_seconds": pl.Int64,
    "matured_at": pl.String,
    "evidence_ids": pl.List(pl.String),
    "attributes": pl.String,
    "creator": pl.String,
    "mint": pl.String,
}


def _stable_float(
    value: object,
    *,
    column: str,
    rejected: dict[str, dict[str, int]],
) -> float | None:
    if value is None:
        return None
    reason = None
    if isinstance(value, bool):
        reason = "boolean_not_numeric_signal"
    else:
        try:
            number = float(value)
        except (TypeError, ValueError, OverflowError):
            reason = "invalid_numeric"
        else:
            if math.isfinite(number):
                return number
            reason = "non_finite"
    per_column = rejected.setdefault(column, {})
    per_column[reason] = per_column.get(reason, 0) + 1
    return None


def _feature_frame(
    rows: list[dict], names: list[str]
) -> tuple[pl.DataFrame, dict[str, object]]:
    rejected: dict[str, dict[str, int]] = {}
    normalized = []
    for row in rows:
        stable = {
            "launch_id": str(row["launch_id"]),
            "mint": str(row["mint"]) if row.get("mint") is not None else None,
            "creator": str(row.get("creator") or ""),
            "cluster_family": (
                str(row["cluster_family"])
                if row.get("cluster_family") is not None
                else None
            ),
            "observation_window_seconds": int(row["observation_window_seconds"]),
            "cutoff_slot": int(row["cutoff_slot"]),
            "cutoff_time": str(row["cutoff_time"]),
            "feature_snapshot_id": str(row["feature_snapshot_id"]),
            "quality_grade": str(row["quality_grade"]),
            "split": str(row["split"]) if row.get("split") is not None else None,
        }
        stable.update(
            {
                name: _stable_float(row.get(name), column=name, rejected=rejected)
                for name in names
            }
        )
        normalized.append(stable)
    schema = {**FEATURE_ID_SCHEMA, **dict.fromkeys(names, pl.Float64)}
    frame = pl.DataFrame(normalized, schema=schema, strict=True)
    return frame, {
        "rejected_feature_values": rejected,
        "rejected_feature_value_count": sum(
            sum(reasons.values()) for reasons in rejected.values()
        ),
    }


def _label_frame(rows: list[dict]) -> tuple[pl.DataFrame, dict[str, object]]:
    rejected: dict[str, dict[str, int]] = {}
    normalized = []
    for row in rows:
        attributes = row.get("attributes") or {}
        normalized.append(
            {
                "launch_id": str(row["launch_id"]),
                "feature_snapshot_id": (
                    str(row["feature_snapshot_id"])
                    if row.get("feature_snapshot_id") is not None
                    else None
                ),
                "cutoff_slot": int(row["cutoff_slot"]),
                "observation_window_seconds": int(row["observation_window_seconds"]),
                "label": str(row["label"]),
                "value": _stable_float(
                    row.get("value"), column="value", rejected=rejected
                ),
                "status": str(row["status"]),
                "horizon_seconds": int(row["horizon_seconds"]),
                "matured_at": (
                    str(row["matured_at"])
                    if row.get("matured_at") is not None
                    else None
                ),
                "evidence_ids": [
                    str(value) for value in (row.get("evidence_ids") or [])
                ],
                "attributes": json.dumps(
                    attributes, sort_keys=True, separators=(",", ":"), default=str
                ),
                "creator": str(row.get("creator") or ""),
                "mint": str(row["mint"]) if row.get("mint") is not None else None,
            }
        )
    frame = pl.DataFrame(normalized, schema=LABEL_SCHEMA, strict=True)
    return frame, {
        "rejected_label_values": rejected,
        "rejected_label_value_count": sum(
            sum(reasons.values()) for reasons in rejected.values()
        ),
    }


def _creator_from_trade(payload: dict) -> str:
    raw = payload.get("raw_event") or {}
    if isinstance(raw, dict) and raw.get("creator"):
        return str(raw.get("creator"))
    return str(payload.get("creator") or payload.get("trader") or "")


def _launches(events: list[ChainEvent]) -> list[dict]:
    out: list[dict] = []
    created_mints: set[str] = set()
    for event in events:
        if event.payload_kind != "launch.created":
            continue
        p = event.payload
        mint = str(p.get("mint") or "")
        if mint:
            created_mints.add(mint)
        out.append(
            {
                "launch_id": str(p.get("launch_id") or mint or ""),
                "mint": mint,
                "creator": str(p.get("creator") or ""),
                "creation_time": event.event_time,
                "creation_slot": event.slot,
                "quote_decimals": int(p.get("quote_decimals") or 9),
            }
        )

    first_trade_by_mint: dict[str, ChainEvent] = {}
    for event in events:
        if event.payload_kind != "trade.executed":
            continue
        mint = str(event.payload.get("mint") or "")
        if not mint or mint in created_mints:
            continue
        if mint not in first_trade_by_mint:
            first_trade_by_mint[mint] = event

    for mint, event in first_trade_by_mint.items():
        p = event.payload
        out.append(
            {
                "launch_id": mint,
                "mint": mint,
                "creator": _creator_from_trade(p),
                "creation_time": event.event_time,
                "creation_slot": event.slot,
                "quote_decimals": int(p.get("quote_decimals") or 9),
            }
        )
    return out


def _index_events(events: list[ChainEvent]) -> dict[str, list[ChainEvent]]:
    indexed: dict[str, list[ChainEvent]] = defaultdict(list)
    for event in events:
        payload = event.payload
        keys = {
            str(value)
            for value in (payload.get("launch_id"), payload.get("mint"))
            if value
        }
        for key in keys:
            indexed[key].append(event)
    return indexed


def _launch_events(
    indexed: dict[str, list[ChainEvent]], launch_id: str, mint: str
) -> list[ChainEvent]:
    by_id: dict[str, ChainEvent] = {}
    for key in {launch_id, mint}:
        for event in indexed.get(key, []):
            by_id[event.event_id] = event
    return sorted(by_id.values(), key=lambda event: (*event.sort_key(), event.event_id))


def _path_for_launch(
    events: list[ChainEvent],
    launch_id: str,
    mint: str | None = None,
) -> list[tuple[datetime, float, float | None]]:
    path: list[tuple[datetime, float, float | None]] = []
    cum_quote = 0
    last_ret = 0.0
    first_px = None
    for event in events:
        if event.payload_kind != "trade.executed":
            continue
        if not event_matches_launch(event.payload, launch_id, mint=mint):
            continue
        q = parse_atomic(event.payload.get("quote_amount_atomic"))
        b = parse_atomic(event.payload.get("base_amount_atomic"))
        dec = int(event.payload.get("quote_decimals") or 9)
        if b <= 0:
            continue
        px = (q / float(10**dec)) / (b / 1e6)
        if first_px is None:
            first_px = px
        last_ret = px / first_px - 1.0 if first_px else 0.0
        cum_quote += q
        path.append((event.event_time, 1.0 + last_ret, None))
    return path


def _observed_until(
    events: list[ChainEvent],
    launch_id: str,
    mint: str | None = None,
) -> datetime | None:
    times = [
        e.event_time
        for e in events
        if event_matches_launch(e.payload, launch_id, mint=mint)
    ]
    return max(times) if times else None


def _relative_path(
    path: list[tuple[datetime, float, float | None]],
    cutoff: datetime,
) -> list[tuple[datetime, float]]:
    entry = None
    for ts, value, _ in path:
        if ts <= cutoff:
            entry = value
        elif entry is None:
            entry = value
            break
    if entry in (None, 0):
        return []
    return [(ts, value / entry - 1.0) for ts, value, _ in path if ts > cutoff]


def _collapse_usd_context(
    events: list[ChainEvent],
    launch_id: str,
    mint: str,
    cutoff: datetime,
    path: list[tuple[datetime, float, float | None]],
) -> tuple[float | None, float | None]:
    """Use only point-in-time USD fields supplied by the historical corpus."""
    horizon = cutoff.timestamp() + 3600
    horizon_path = [
        (ts, value)
        for ts, value, _ in path
        if cutoff.timestamp() <= ts.timestamp() <= horizon
    ]
    if not horizon_path:
        return None, None
    peak_time = max(horizon_path, key=lambda item: item[1])[0]
    volume_usd = 0.0
    peak_exit_usd = None
    saw_usd_volume = False
    usd_volume_complete = True
    for event in events:
        if event.payload_kind != "trade.executed":
            continue
        if not event_matches_launch(event.payload, launch_id, mint=mint):
            continue
        if event.event_time > peak_time:
            continue
        payload = event.payload
        quote_usd = payload.get("quote_amount_usd")
        if quote_usd is None:
            quote_price_usd = payload.get("quote_usd_price")
            if quote_price_usd is not None:
                quote = parse_atomic(payload.get("quote_amount_atomic"))
                decimals = int(payload.get("quote_decimals") or 9)
                quote_usd = (quote / float(10**decimals)) * float(quote_price_usd)
        if quote_usd is not None:
            volume_usd += float(quote_usd)
            saw_usd_volume = True
        else:
            usd_volume_complete = False
        if event.event_time == peak_time:
            value = payload.get("executable_exit_usd")
            if value is not None:
                peak_exit_usd = float(value)
    return (
        volume_usd if saw_usd_volume and usd_volume_complete else None
    ), peak_exit_usd


def _lake_partition(cfg: dict, knowledge_end: datetime) -> Path | None:
    lake_uri = cfg.get("lake_uri")
    if not lake_uri:
        return None
    date = cfg.get("lake_date") or cfg.get("date")
    if date is None:
        date = knowledge_end.date().isoformat()
    else:
        date = str(date).split("T", 1)[0]
    root = Path(str(lake_uri))
    if "date=" in root.as_posix():
        return root
    return root / f"date={date}"


def _write_lake(
    cfg: dict, knowledge_end: datetime, feature_df: pl.DataFrame, label_df: pl.DataFrame
) -> str | None:
    part = _lake_partition(cfg, knowledge_end)
    if part is None:
        return None
    part.mkdir(parents=True, exist_ok=True)
    feature_df.write_parquet(part / "snapshots.parquet")
    label_df.write_parquet(part / "outcomes.parquet")
    return str(part)


def build_dataset(config_path: str) -> dict:
    cfg = load_yaml(config_path)
    events, source_manifest = load_canonical_sources(cfg)
    windows = [
        int(w) for w in cfg.get("observation_windows_seconds") or OBSERVATION_WINDOWS
    ]
    knowledge_end = max(
        (e.known_at for e in events),
        default=parse_cutoff("2026-09-07T12:00:00.000000Z"),
    )
    launches = _launches(events)
    event_index = _index_events(events)
    names = ordered_feature_names()
    rows: list[dict] = []
    label_rows: list[dict] = []
    for launch in launches:
        lid = launch["launch_id"]
        mint = launch.get("mint") or ""
        scoped_events = _launch_events(event_index, lid, mint)
        path = _path_for_launch(scoped_events, lid, mint=mint)
        observed = _observed_until(scoped_events, lid, mint=mint)
        for window in windows:
            cutoff_slot, cutoff_time = cutoff_from_creation(
                launch["creation_time"],
                window,
                launch["creation_slot"],
            )
            # Do not use facts observed after the row cutoff.
            vec = extract_features(
                scoped_events,
                launch_id=lid,
                window_seconds=window,
                cutoff_slot=cutoff_slot,
                knowledge_cutoff=min(cutoff_time, knowledge_end),
                creation_time=launch["creation_time"],
                creation_slot=launch["creation_slot"],
                creator=launch["creator"],
                mint=mint or None,
            )
            row = {
                "launch_id": lid,
                "mint": mint or None,
                "creator": launch["creator"],
                "cluster_family": None,
                "observation_window_seconds": window,
                "cutoff_slot": cutoff_slot,
                "cutoff_time": cutoff_time.isoformat(),
                "feature_snapshot_id": vec.feature_snapshot_id,
                "quality_grade": vec.quality.grade.value
                if hasattr(vec.quality.grade, "value")
                else vec.quality.grade,
                "split": None,
            }
            for name in names:
                row[name] = vec.values.get(name)
            rows.append(row)

            grads = graduation_labels(
                scoped_events,
                launch_id=lid,
                cutoff=cutoff_time,
                cutoff_slot=cutoff_slot,
                observation_window_seconds=window,
                observed_until=observed,
                knowledge_cutoff=knowledge_end,
                mint=mint or None,
            )
            executable_path = _relative_path(path, cutoff_time)
            barrier = first_passage(
                executable_path,
                launch_id=lid,
                cutoff=cutoff_time,
                cutoff_slot=cutoff_slot,
                observation_window_seconds=window,
                take_profit=0.50,
                stop_loss=0.20,
                horizon_seconds=900,
                label="up_50_before_down_20_15m_v1",
                observed_until=observed,
            )
            barrier_25 = first_passage(
                executable_path,
                launch_id=lid,
                cutoff=cutoff_time,
                cutoff_slot=cutoff_slot,
                observation_window_seconds=window,
                take_profit=0.25,
                stop_loss=0.15,
                horizon_seconds=900,
                label="hit_+25_before_-15",
                observed_until=observed,
            )
            label_cfg = cfg.get("labels") or {}
            pre_peak_usd, peak_exit_usd = _collapse_usd_context(
                scoped_events,
                lid,
                mint,
                cutoff_time,
                path,
            )
            usd_available = pre_peak_usd is not None and peak_exit_usd is not None
            collapse = severe_collapse(
                path,
                launch_id=lid,
                cutoff=cutoff_time,
                cutoff_slot=cutoff_slot,
                observation_window_seconds=window,
                observed_until=observed,
                min_quote_volume_usd=float(
                    label_cfg.get("collapse_min_quote_volume_usd") or 5000
                ),
                peak_exit_quote_usd=float(
                    label_cfg.get("collapse_peak_exit_quote_usd") or 15
                ),
                pre_peak_quote_volume_usd=pre_peak_usd,
                peak_executable_exit_usd=peak_exit_usd,
                usd_available=usd_available,
            )
            exit_risk = exit_risk_label(
                executable_path,
                launch_id=lid,
                cutoff=cutoff_time,
                cutoff_slot=cutoff_slot,
                observation_window_seconds=window,
                observed_until=observed,
                drawdown=float(label_cfg.get("exit_risk_drawdown") or 0.20),
                recovery=float(label_cfg.get("exit_risk_recovery") or 0.10),
                horizon_seconds=int(label_cfg.get("exit_risk_horizon_seconds") or 60),
            )
            plus_20 = first_passage(
                executable_path,
                launch_id=lid,
                cutoff=cutoff_time,
                cutoff_slot=cutoff_slot,
                observation_window_seconds=window,
                take_profit=0.20,
                stop_loss=0.10,
                horizon_seconds=120,
                label="plus_20_before_minus_10_2m_v1",
                observed_until=observed,
            )
            plus_30 = first_passage(
                executable_path,
                launch_id=lid,
                cutoff=cutoff_time,
                cutoff_slot=cutoff_slot,
                observation_window_seconds=window,
                take_profit=0.30,
                stop_loss=0.15,
                horizon_seconds=120,
                label="plus_30_before_minus_15_2m_v1",
                observed_until=observed,
            )
            shorts = short_horizon_labels(
                executable_path,
                launch_id=lid,
                cutoff=cutoff_time,
                cutoff_slot=cutoff_slot,
                observation_window_seconds=window,
                observed_until=observed,
            )
            for lab in (
                list(grads)
                + [barrier, barrier_25, collapse, exit_risk, plus_20, plus_30]
                + list(shorts)
            ):
                if cfg.get("exclude_ambiguous_labels") and lab.status == "ambiguous":
                    continue
                label_rows.append(
                    to_plain(lab)
                    | {
                        "creator": launch["creator"],
                        "mint": mint or None,
                        "feature_snapshot_id": vec.feature_snapshot_id,
                    }
                )

    if not rows:
        raise ValueError("dataset contains no feature rows")

    feature_df, feature_normalization = _feature_frame(rows, names)
    label_df, label_normalization = _label_frame(label_rows)
    split_assign = chronological_splits(
        [
            {
                "launch_id": r["launch_id"],
                "cutoff_time": r["cutoff_time"],
                "creator": r["creator"],
            }
            for r in rows
        ],
        cfg.get("split") or {},
    )
    feature_df = feature_df.with_columns(
        pl.col("launch_id").replace_strict(split_assign, default="train").alias("split")
    )

    out_dir = Path(cfg.get("output_uri") or "artifacts/datasets/dataset-v1")
    out_dir.mkdir(parents=True, exist_ok=True)
    feat_path = out_dir / "features.parquet"
    lab_path = out_dir / "labels.parquet"
    feature_df.write_parquet(feat_path)
    label_df.write_parquet(lab_path)
    source_manifest_path = out_dir / "source-manifest.json"
    dump_json(source_manifest_path, source_manifest)
    lake_partition = _write_lake(cfg, knowledge_end, feature_df, label_df)
    manifest = {
        "dataset_version": cfg.get("dataset_version", "dataset-v1"),
        "feature_schema_version": cfg.get("feature_schema_version", "features-v1"),
        "row_count": feature_df.height,
        "label_count": label_df.height,
        "launch_count": len(launches),
        "windows": windows,
        "class_balance": _balances(label_df),
        "split_counts": feature_df.group_by("split").len().to_dicts()
        if "split" in feature_df.columns
        else [],
        "features_sha256": sha256_file(feat_path),
        "labels_sha256": sha256_file(lab_path),
        "events_uri": cfg.get("events_uri"),
        "event_sources": cfg.get("event_sources"),
        "source_manifest_uri": str(source_manifest_path),
        "source_manifest_sha256": sha256_file(source_manifest_path),
        "source_manifest_complete": source_manifest[
            "all_required_historical_sources_complete"
        ],
        "lake_uri": cfg.get("lake_uri"),
        "lake_partition": lake_partition,
        "include_dead_and_inactive_launches": True,
        "feature_availability": "bitemporal",
        "normalization_diagnostics": {
            **feature_normalization,
            **label_normalization,
        },
        "physical_schema": {
            "features": {name: str(dtype) for name, dtype in feature_df.schema.items()},
            "labels": {name: str(dtype) for name, dtype in label_df.schema.items()},
        },
        "promotion_eligible": bool(cfg.get("promotion_eligible", True)),
        "experimental_requirements": cfg.get("experimental_requirements") or {},
        "corpus_kind": cfg.get("corpus_kind", "historical"),
        "content_sha256": sha256_canonical(
            {"features": sha256_file(feat_path), "labels": sha256_file(lab_path)}
        ),
    }
    dump_json(out_dir / "manifest.json", manifest)
    from pumphound_ml.extraction.readiness import build_diagnostics

    diagnostics = build_diagnostics(out_dir)
    diagnostics_path = out_dir / "diagnostics.json"
    manifest["diagnostics_uri"] = str(diagnostics_path)
    manifest["diagnostics_sha256"] = sha256_file(diagnostics_path)
    manifest["readiness"] = diagnostics["status"]
    dump_json(out_dir / "manifest.json", manifest)
    return manifest


def _balances(label_df: pl.DataFrame) -> dict:
    if label_df.is_empty() or "label" not in label_df.columns:
        return {}
    out: dict[str, dict] = {}
    for name in label_df.get_column("label").unique().to_list():
        sub = label_df.filter(pl.col("label") == name)
        out[str(name)] = {
            "n": sub.height,
            "positive": int(sub.filter(pl.col("value") == 1).height)
            if "value" in sub.columns
            else 0,
            "censored": int(sub.filter(pl.col("status") == "censored").height)
            if "status" in sub.columns
            else 0,
        }
    return out


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Build a point-in-time PumpHound dataset"
    )
    parser.add_argument("config", help="dataset yaml config")
    args = parser.parse_args(argv)
    try:
        manifest = build_dataset(args.config)
    except (OSError, RuntimeError, TypeError, ValueError) as exc:
        print(str(exc), file=sys.stderr)
        return 2
    print(json.dumps(manifest, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
