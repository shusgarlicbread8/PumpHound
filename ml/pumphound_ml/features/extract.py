"""Point-in-time market feature extraction. No invented prices."""

from __future__ import annotations

import math
from collections import defaultdict
from datetime import datetime, timedelta
from typing import Any, Iterable

from pumphound_ml.clock import event_visible, filter_events
from pumphound_ml.features.availability import ordered_feature_names as schema_names
from pumphound_ml.io import content_id
from pumphound_ml.types import (
    FEATURE_SCHEMA_VERSION,
    OBSERVATION_WINDOWS,
    ChainEvent,
    DataQuality,
    FeatureVector,
    ObservationPoint,
    QualityGrade,
    parse_atomic,
)

PROGRAM_OWNERS = frozenset({"bonding_curve", "pool", "fee", "vault", "burn", "lock"})


def ordered_feature_names() -> list[str]:
    try:
        return schema_names()
    except Exception:
        return [
            "age_seconds",
            "curve_progress",
            "round_trip_cost_fraction",
            "entry_impact_bps",
            "history_coverage_fraction",
        ]


def _norm_quote(atomic: int, decimals: int) -> float:
    return float(atomic) / float(10**decimals)


def _quantile(values: list[float], q: float) -> float | None:
    if not values:
        return None
    xs = sorted(values)
    if len(xs) == 1:
        return xs[0]
    pos = q * (len(xs) - 1)
    lo = int(math.floor(pos))
    hi = int(math.ceil(pos))
    if lo == hi:
        return xs[lo]
    return xs[lo] * (hi - pos) + xs[hi] * (pos - lo)


def _cv(values: list[float], min_n: int) -> float | None:
    if len(values) < min_n:
        return None
    mean = sum(values) / len(values)
    if mean == 0:
        return None
    var = sum((x - mean) ** 2 for x in values) / (len(values) - 1)
    return math.sqrt(var) / mean


def _window_rows(trades: list[dict], cutoff: datetime, window: int) -> list[dict]:
    start = cutoff - timedelta(seconds=window)
    return [t for t in trades if start < t["event_time"] <= cutoff]


def _put(values: dict[str, float | None], missing: dict[str, bool], name: str, value: float | None) -> None:
    values[name] = value
    missing[name] = value is None


def _payload_mint(payload: dict[str, Any]) -> str:
    return str(payload.get("mint") or "")


def resolve_launch_mint(
    events: Iterable[ChainEvent],
    launch_id: str,
    mint: str | None = None,
) -> str:
    if mint:
        return str(mint)
    for event in events:
        p = event.payload
        if event.payload_kind == "launch.created" and str(p.get("launch_id") or "") == launch_id:
            found = _payload_mint(p)
            if found:
                return found
        if str(p.get("launch_id") or "") == launch_id:
            found = _payload_mint(p)
            if found:
                return found
    return launch_id


def trade_belongs_to_launch(payload: dict[str, Any], launch_id: str, mint: str | None = None) -> bool:
    """Join trades on launch_id, or on mint when launch_id is missing (live journals)."""
    trade_lid = payload.get("launch_id")
    trade_mint = _payload_mint(payload)
    resolved_mint = str(mint or "")
    has_lid = trade_lid is not None and str(trade_lid) != ""
    if has_lid:
        if str(trade_lid) == launch_id:
            return True
        return bool(resolved_mint) and trade_mint == resolved_mint
    if resolved_mint:
        return trade_mint == resolved_mint
    return bool(trade_mint) and trade_mint == launch_id


def event_matches_launch(payload: dict[str, Any], launch_id: str, mint: str | None = None) -> bool:
    lid = payload.get("launch_id")
    event_mint = _payload_mint(payload)
    resolved_mint = str(mint or "")
    if lid is not None and str(lid) != "" and str(lid) == launch_id:
        return True
    if resolved_mint and event_mint == resolved_mint:
        return True
    return bool(event_mint) and event_mint == launch_id


def extract_launch_trades(
    events: Iterable[ChainEvent],
    launch_id: str,
    mint: str | None = None,
) -> list[dict]:
    rows: list[dict] = []
    for event in events:
        if event.payload_kind != "trade.executed":
            continue
        p = event.payload
        if not trade_belongs_to_launch(p, launch_id, mint=mint):
            continue
        rows.append(
            {
                "event_id": event.event_id,
                "event_time": event.event_time,
                "slot": event.slot,
                "known_at": event.known_at,
                "available_from_slot": event.available_from_slot,
                "side": str(p.get("side") or "buy").lower(),
                "trader": str(p.get("trader") or ""),
                "quote_atomic": parse_atomic(p.get("quote_amount_atomic")),
                "base_atomic": parse_atomic(p.get("base_amount_atomic")),
                "quote_mint": str(p.get("quote_mint") or ""),
                "quote_decimals": int(p.get("quote_decimals") or 9),
                "virtual_base": p.get("virtual_base_atomic"),
                "virtual_quote": p.get("virtual_quote_atomic"),
            }
        )
    rows.sort(key=lambda r: (r["slot"], r["event_time"]))
    return rows


def _executable_price(trade: dict) -> float | None:
    base = trade.get("base_atomic") or 0
    quote = trade.get("quote_atomic") or 0
    if base <= 0 or quote <= 0:
        return None
    return _norm_quote(quote, trade.get("quote_decimals") or 9) / _norm_quote(base, 6)


def _curve_progress(trade: dict | None, created: dict | None) -> float | None:
    if not trade and not created:
        return None
    src = trade or created or {}
    vq = src.get("virtual_quote")
    if vq is None and created:
        vq = created.get("virtual_quote_atomic")
    rq = src.get("real_quote_atomic") if created else None
    try:
        vq_i = int(str(vq)) if vq is not None else None
        init = int(str(created.get("virtual_quote_atomic"))) if created and created.get("virtual_quote_atomic") is not None else vq_i
        if vq_i is None or init is None or init <= 0:
            return None
        # Progress as quote reserve growth vs initial virtual quote; formula_id retained in attributes.
        return max(0.0, min(1.0, (vq_i - init) / float(init) if False else 0.0))
    except (TypeError, ValueError):
        return None


def extract_features(
    events: Iterable[ChainEvent],
    *,
    launch_id: str,
    window_seconds: int,
    cutoff_slot: int,
    knowledge_cutoff: datetime,
    creation_time: datetime | None = None,
    creation_slot: int | None = None,
    creator: str | None = None,
    mint: str | None = None,
    graph_features: dict[str, float | None] | None = None,
    holders: dict[str, int] | None = None,
    program_holders: set[str] | None = None,
    coverage_fraction: float | None = None,
    window_coverage_complete: bool = True,
    schema_version: str = FEATURE_SCHEMA_VERSION,
) -> FeatureVector:
    if window_seconds not in OBSERVATION_WINDOWS:
        raise ValueError(f"unsupported observation window {window_seconds}")
    resolved_mint = resolve_launch_mint(events, launch_id, mint)
    visible = filter_events(events, cutoff_slot, knowledge_cutoff)
    created = None
    for event in visible:
        if event.payload_kind != "launch.created":
            continue
        if not event_matches_launch(event.payload, launch_id, mint=resolved_mint):
            continue
        created = event.payload
        creation_time = creation_time or event.event_time
        creation_slot = creation_slot if creation_slot is not None else event.slot
        creator = creator or str(event.payload.get("creator") or "")
        resolved_mint = resolved_mint or _payload_mint(event.payload)
    trades = [
        t
        for t in extract_launch_trades(visible, launch_id, mint=resolved_mint)
        if t["slot"] <= cutoff_slot and t["known_at"] <= knowledge_cutoff
    ]
    values: dict[str, float | None] = {}
    missing: dict[str, bool] = {}
    deps = [t["event_id"] for t in trades]

    age = None
    if creation_time is not None:
        age = max(0.0, (knowledge_cutoff - creation_time).total_seconds())
    _put(values, missing, "age_seconds", age)

    last = trades[-1] if trades else None
    progress = None
    if last and created:
        try:
            vb0 = int(str(created.get("virtual_base_atomic") or "0"))
            vb1 = int(str(last.get("virtual_base") or created.get("virtual_base_atomic") or "0"))
            if vb0 > 0 and vb1 <= vb0:
                progress = (vb0 - vb1) / float(vb0)
        except (TypeError, ValueError):
            progress = None
    _put(values, missing, "curve_progress", progress)
    _put(values, missing, "round_trip_cost_fraction", None)
    _put(values, missing, "entry_impact_bps", None)

    def flow(window: int, suffix: bool = True) -> dict[str, float | None]:
        rows = _window_rows(trades, knowledge_cutoff, window)
        complete = window_coverage_complete
        buys = [r for r in rows if r["side"] == "buy"]
        sells = [r for r in rows if r["side"] == "sell"]
        buy_count = float(len(buys)) if complete else (float(len(buys)) if rows else None)
        sell_count = float(len(sells)) if complete else (float(len(sells)) if rows else None)
        if not complete:
            buy_q = None
            sell_q = None
        else:
            buy_q = sum(_norm_quote(r["quote_atomic"], r["quote_decimals"]) for r in buys)
            sell_q = sum(_norm_quote(r["quote_atomic"], r["quote_decimals"]) for r in sells)
        return {
            "buy_count": buy_count if complete else (0.0 if complete else None),
            "sell_count": sell_count if complete else (0.0 if complete else None),
            "buy_quote": buy_q,
            "sell_quote": sell_q,
            "buys": buys,
            "sells": sells,
            "rows": rows,
        }

    # Always emit the catalog windows so schema order is stable.
    cache: dict[int, dict[str, Any]] = {}
    for w in OBSERVATION_WINDOWS:
        cache[w] = flow(w)
        tag = str(w)
        stats = cache[w]
        _put(values, missing, f"buy_count_{tag}", 0.0 if window_coverage_complete else None if stats["buy_count"] is None else float(len(stats["buys"])))
        if window_coverage_complete:
            values[f"buy_count_{tag}"] = float(len(stats["buys"]))
            values[f"sell_count_{tag}"] = float(len(stats["sells"]))
            missing[f"buy_count_{tag}"] = False
            missing[f"sell_count_{tag}"] = False
        else:
            _put(values, missing, f"buy_count_{tag}", None)
            _put(values, missing, f"sell_count_{tag}", None)
        _put(values, missing, f"buy_quote_{tag}", stats["buy_quote"])
        _put(values, missing, f"sell_quote_{tag}", stats["sell_quote"])

    for w in (30, 60, 300):
        b, s = cache[w]["buy_quote"], cache[w]["sell_quote"]
        if b is None or s is None or (b + s) == 0:
            _put(values, missing, f"buy_sell_imbalance_{w}", None)
        else:
            _put(values, missing, f"buy_sell_imbalance_{w}", (b - s) / (b + s))

    first_seen: dict[str, datetime] = {}
    for t in trades:
        first_seen.setdefault(t["trader"], t["event_time"])

    for w in (15, 30, 60, 300):
        buys = cache[w]["buys"]
        owners = {r["trader"] for r in buys if r["trader"]}
        if not window_coverage_complete:
            _put(values, missing, f"unique_buyers_{w}", None)
        else:
            _put(values, missing, f"unique_buyers_{w}", float(len(owners)) if owners or buys else 0.0)

    for w in (5, 15, 30, 60):
        if not window_coverage_complete:
            _put(values, missing, f"buyer_arrival_rate_{w}", None)
            continue
        start = knowledge_cutoff - timedelta(seconds=w)
        new = {r["trader"] for r in cache[w]["buys"] if first_seen.get(r["trader"]) and start < first_seen[r["trader"]] <= knowledge_cutoff}
        _put(values, missing, f"buyer_arrival_rate_{w}", len(new) / float(w))

    for w in (15, 30):
        cur = values.get(f"buyer_arrival_rate_{w}")
        prev_rows = _window_rows(trades, knowledge_cutoff - timedelta(seconds=w), w)
        if cur is None or not window_coverage_complete:
            _put(values, missing, f"buyer_arrival_acceleration_{w}", None)
            continue
        prev_start = knowledge_cutoff - timedelta(seconds=2 * w)
        prev_end = knowledge_cutoff - timedelta(seconds=w)
        prev_new = {
            r["trader"]
            for r in prev_rows
            if first_seen.get(r["trader"]) and prev_start < first_seen[r["trader"]] <= prev_end
        }
        _put(values, missing, f"buyer_arrival_acceleration_{w}", cur - (len(prev_new) / float(w)))

    for w in (30, 60, 300):
        buys = cache[w]["buys"]
        counts: dict[str, int] = defaultdict(int)
        for r in buys:
            counts[r["trader"]] += 1
        if not counts:
            _put(values, missing, f"repeat_buyer_ratio_{w}", None)
        else:
            _put(values, missing, f"repeat_buyer_ratio_{w}", sum(1 for v in counts.values() if v >= 2) / len(counts))

    inventory: dict[str, int] = defaultdict(int)
    for t in trades:
        if t["side"] == "buy":
            inventory[t["trader"]] += t["base_atomic"]
        else:
            inventory[t["trader"]] -= t["base_atomic"]
    for w in (60, 300):
        buyers = {r["trader"] for r in cache[w]["buys"] if r["trader"]}
        if not buyers:
            _put(values, missing, f"buyer_retention_{w}", None)
        else:
            retained = sum(1 for wlt in buyers if inventory.get(wlt, 0) > 0)
            _put(values, missing, f"buyer_retention_{w}", retained / len(buyers))
        start_holders = {wlt for wlt, qty in inventory.items() if qty > 0}
        # Approximate starting holders as those with inventory before window sells.
        if not start_holders:
            _put(values, missing, f"wallet_churn_{w}", None)
        else:
            exited = {r["trader"] for r in cache[w]["sells"] if inventory.get(r["trader"], 0) <= 0}
            _put(values, missing, f"wallet_churn_{w}", len(exited) / len(start_holders))

    for w in (30, 60, 300):
        sizes = [_norm_quote(r["quote_atomic"], r["quote_decimals"]) for r in cache[w]["buys"]]
        _put(values, missing, f"buy_size_p10_{w}", _quantile(sizes, 0.10))
        _put(values, missing, f"buy_size_p50_{w}", _quantile(sizes, 0.50))
        _put(values, missing, f"buy_size_p90_{w}", _quantile(sizes, 0.90))
    for w in (30, 60):
        sizes = [_norm_quote(r["quote_atomic"], r["quote_decimals"]) for r in cache[w]["buys"]]
        _put(values, missing, f"buy_size_cv_{w}", _cv(sizes, 2))
        times = [r["event_time"] for r in cache[w]["rows"]]
        if len(times) < 3:
            _put(values, missing, f"interarrival_cv_{w}", None)
        else:
            gaps = [(b - a).total_seconds() for a, b in zip(times, times[1:])]
            _put(values, missing, f"interarrival_cv_{w}", _cv(gaps, 2))

    for w in (15, 30, 60):
        cur = cache[w]["sell_quote"]
        prev = _window_rows(trades, knowledge_cutoff - timedelta(seconds=w), w)
        prev_sell = sum(_norm_quote(r["quote_atomic"], r["quote_decimals"]) for r in prev if r["side"] == "sell") if window_coverage_complete else None
        if cur is None or prev_sell is None:
            _put(values, missing, f"sell_acceleration_{w}", None)
        else:
            _put(values, missing, f"sell_acceleration_{w}", (cur / w) - (prev_sell / w))

    for w in (30, 60, 300):
        sells = cache[w]["sells"]
        total = cache[w]["sell_quote"]
        if not sells or not total:
            _put(values, missing, f"largest_seller_share_{w}", None)
        else:
            by_w: dict[str, float] = defaultdict(float)
            for r in sells:
                by_w[r["trader"]] += _norm_quote(r["quote_atomic"], r["quote_decimals"])
            _put(values, missing, f"largest_seller_share_{w}", max(by_w.values()) / total)

    for w in (15, 30, 60, 300):
        now_px = _executable_price(last) if last else None
        past_rows = [t for t in trades if t["event_time"] <= knowledge_cutoff - timedelta(seconds=w)]
        past_px = _executable_price(past_rows[-1]) if past_rows else None
        if now_px is None or past_px is None or past_px == 0:
            _put(values, missing, f"return_{w}", None)
        else:
            _put(values, missing, f"return_{w}", now_px / past_px - 1.0)

    for w in (30, 60, 300):
        rows = cache[w]["rows"]
        px = [_executable_price(r) for r in rows]
        px = [p for p in px if p and p > 0]
        if len(px) < 2:
            _put(values, missing, f"realized_volatility_{w}", None)
        else:
            logs = [math.log(b / a) for a, b in zip(px, px[1:]) if a > 0]
            _put(values, missing, f"realized_volatility_{w}", math.sqrt(sum(x * x for x in logs)))

    for w in (30, 60, 300):
        if creator is None:
            _put(values, missing, f"net_creator_flow_{w}", None)
            continue
        rows = cache[w]["rows"]
        linked = [r for r in rows if r["trader"] == creator]
        buy = sum(_norm_quote(r["quote_atomic"], r["quote_decimals"]) for r in linked if r["side"] == "buy")
        sell = sum(_norm_quote(r["quote_atomic"], r["quote_decimals"]) for r in linked if r["side"] == "sell")
        _put(values, missing, f"net_creator_flow_{w}", buy - sell)

    excluded = program_holders or PROGRAM_OWNERS
    eligible = {w: q for w, q in (holders or {}).items() if w not in excluded and q > 0}
    supply = sum(eligible.values())
    if not eligible or supply <= 0:
        for name in (
            "holder_top_1_share",
            "holder_top_5_share",
            "holder_top_10_share",
            "holder_hhi",
        ):
            _put(values, missing, name, None)
    else:
        ranked = sorted(eligible.values(), reverse=True)
        _put(values, missing, "holder_top_1_share", sum(ranked[:1]) / supply)
        _put(values, missing, "holder_top_5_share", sum(ranked[:5]) / supply)
        _put(values, missing, "holder_top_10_share", sum(ranked[:10]) / supply)
        _put(values, missing, "holder_hhi", sum((q / supply) ** 2 for q in ranked))

    graph_defaults = [
        "cluster_hhi",
        "independent_cluster_count",
        "creator_linked_supply_share",
        "fresh_wallet_share",
        "immediate_funding_share",
        "funding_diversity",
        "cohort_recurrence",
        "co_buy_density",
        "weighted_degree_concentration",
        "cohort_modularity",
        "smart_wallet_retention",
        "wallet_skill_posterior_mean",
        "wallet_skill_effective_sample",
        "creator_graduation_posterior",
        "creator_collapse_posterior",
        "history_coverage_fraction",
    ]
    g = graph_features or {}
    for name in graph_defaults:
        _put(values, missing, name, g.get(name) if name != "history_coverage_fraction" else g.get(name, coverage_fraction))
    if coverage_fraction is not None:
        _put(values, missing, "history_coverage_fraction", coverage_fraction)

    missing_fields = [k for k, v in missing.items() if v]
    grade = QualityGrade.A
    score = 0.93
    if not window_coverage_complete:
        grade, score = QualityGrade.C, 0.4
    elif missing_fields:
        grade, score = QualityGrade.B, 0.7
    quality = DataQuality(
        grade=grade,
        score=score,
        ordering="exact" if window_coverage_complete else "provisional",
        coverage="complete_for_scope" if window_coverage_complete else "partial",
        missing_fields=missing_fields,
    )
    as_of = ObservationPoint(
        slot=cutoff_slot,
        observed_at=knowledge_cutoff,
        knowledge_cutoff=knowledge_cutoff,
        event_time=knowledge_cutoff,
        commitment="finalized",
    )
    return FeatureVector(
        feature_snapshot_id=content_id("feat", launch_id, window_seconds, cutoff_slot, schema_version),
        launch_id=launch_id,
        observation_window_seconds=window_seconds,
        as_of=as_of,
        schema_version=schema_version,
        values=values,
        dependency_ids=deps,
        quality=quality,
        missingness=missing,
    )
