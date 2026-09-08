"""Bounded funding ancestry. Hubs stop the crawl and do not prove common control."""

from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, timedelta
from typing import Iterable

from pumphound_ml.graph.facts import GraphFactBatch, inbound_transfers, wallet_first_buy
from pumphound_ml.graph.hubs import hub_penalty, is_hub, recipient_degrees
from pumphound_ml.graph.labels import IdentityLabel, label_map
from pumphound_ml.types import FundingHop, FundingPath, FundingPathResult, GraphFact

DEFAULT_MAX_HOPS = 3
DEFAULT_MAX_TRANSFERS = 100
DEFAULT_LOOKBACK_DAYS = 30
DEFAULT_MATERIALITY = 100_000


@dataclass(frozen=True)
class FundingBudget:
    max_hops: int = DEFAULT_MAX_HOPS
    max_transfers_per_wallet: int = DEFAULT_MAX_TRANSFERS
    lookback_days: int = DEFAULT_LOOKBACK_DAYS
    materiality_native_atomic: int = DEFAULT_MATERIALITY
    stop_at_hubs: bool = True

    @classmethod
    def from_config(cls, data: dict | None) -> FundingBudget:
        data = data or {}
        return cls(
            max_hops=int(data.get("max_hops", DEFAULT_MAX_HOPS)),
            max_transfers_per_wallet=int(data.get("max_transfers_per_wallet", DEFAULT_MAX_TRANSFERS)),
            lookback_days=int(data.get("lookback_days", DEFAULT_LOOKBACK_DAYS)),
            materiality_native_atomic=int(data.get("materiality_native_atomic", DEFAULT_MATERIALITY)),
            stop_at_hubs=bool(data.get("stop_at_hubs", True)),
        )


def _wallet_key(node_id: str) -> str:
    return node_id.split(":", 1)[-1]


def _material(fact: GraphFact, budget: FundingBudget) -> bool:
    role = str((fact.attributes or {}).get("transfer_role") or "unknown")
    if role in {"rent", "fee", "trade_settlement"}:
        return False
    amount = fact.amount_atomic or 0
    return amount >= budget.materiality_native_atomic


def trace_funding(
    wallet: str,
    cutoff,
    budget: FundingBudget | None = None,
    *,
    facts: GraphFactBatch | None = None,
    labels: Iterable[IdentityLabel] | None = None,
    first_buy_slot: int | None = None,
) -> FundingPathResult:
    """Walk inbound transfers at most 3 hops / 100 transfers / 30 days; stop at hubs."""
    if facts is None:
        raise ValueError("facts batch is required; unknown ancestry is not invented")
    budget = budget or FundingBudget()
    cutoff_slot = int(cutoff.cutoff_slot) if hasattr(cutoff, "cutoff_slot") else int(cutoff)
    knowledge_cutoff = facts.knowledge_cutoff
    degrees = recipient_degrees(facts.facts)
    label_types = label_map(labels or [], cutoff_slot, knowledge_cutoff)

    first = wallet_first_buy(facts, wallet)
    buy_slot = first_buy_slot if first_buy_slot is not None else (first.slot if first else cutoff_slot)
    lookback_start = knowledge_cutoff - timedelta(days=budget.lookback_days)

    stopped_hubs: list[str] = []
    evidence: list[str] = []
    paths: list[FundingPath] = []
    reasons: list[str] = []

    def hop_ok(fact: GraphFact) -> bool:
        if fact.slot > buy_slot or fact.slot > cutoff_slot:
            return False
        if fact.known_at > knowledge_cutoff or fact.known_at < lookback_start:
            return False
        return _material(fact, budget)

    def walk(current: str, depth: int, hops: list[FundingHop], visited: set[str]) -> None:
        if depth >= budget.max_hops:
            reasons.append("max_hops")
            paths.append(_finish_path(hops, truncated=True, extra=["max_hops"], degrees=degrees, labels=label_types))
            return
        inbound = [f for f in inbound_transfers(facts, current) if hop_ok(f)]
        inbound.sort(key=lambda f: (f.slot, f.event_id))
        inbound = inbound[: budget.max_transfers_per_wallet]
        if len(inbound_transfers(facts, current)) > budget.max_transfers_per_wallet:
            reasons.append("transfer_budget")
        if not inbound:
            if hops:
                paths.append(_finish_path(hops, truncated=False, extra=[], degrees=degrees, labels=label_types))
            return
        for fact in inbound:
            src = _wallet_key(fact.source_node)
            if src in visited:
                continue
            hub = is_hub(src, labels=label_types, degrees=degrees)
            hop = FundingHop(
                from_wallet=src,
                to_wallet=current,
                amount_atomic=fact.amount_atomic or 0,
                mint=fact.asset_mint or "",
                slot=fact.slot,
                event_id=fact.event_id,
                seconds_before_first_buy=None,
                hub=hub,
            )
            evidence.append(fact.event_id)
            next_hops = hops + [hop]
            if hub and budget.stop_at_hubs:
                stopped_hubs.append(src)
                paths.append(
                    _finish_path(next_hops, truncated=True, extra=["stopped_at_hub"], degrees=degrees, labels=label_types)
                )
                continue
            walk(src, depth + 1, next_hops, visited | {src})

    walk(wallet, 0, [], {wallet})
    if not paths:
        reasons.append("no_material_inbound")

    funders = []
    seen_f: set[str] = set()
    for path in paths:
        if path.hops:
            root = path.hops[0].from_wallet
            if root not in seen_f:
                seen_f.add(root)
                funders.append(root)

    unknown = not paths or all(p.truncated and not p.strength for p in paths)
    return FundingPathResult(
        wallet=wallet,
        cutoff_slot=cutoff_slot,
        knowledge_cutoff=knowledge_cutoff,
        paths=paths,
        competing_funders=funders,
        stopped_at_hubs=sorted(set(stopped_hubs)),
        unknown=unknown,
        evidence_ids=sorted(set(evidence)),
        budget={
            "max_hops": budget.max_hops,
            "max_transfers_per_wallet": budget.max_transfers_per_wallet,
            "lookback_days": budget.lookback_days,
        },
    )


def _finish_path(
    hops: list[FundingHop],
    *,
    truncated: bool,
    extra: list[str],
    degrees: dict[str, int],
    labels: dict[str, str],
) -> FundingPath:
    if not hops:
        return FundingPath(hops=[], strength=0.0, components={}, truncated=truncated, truncation_reasons=extra)
    root = hops[0].from_wallet
    dest_total = sum(h.amount_atomic for h in hops if h.to_wallet == hops[-1].to_wallet) or hops[-1].amount_atomic
    share = hops[-1].amount_atomic / dest_total if dest_total else 0.0
    depth_pen = 1.0 / (1.0 + (len(hops) - 1))
    hub_w = hub_penalty(root, labels=labels, degrees=degrees)
    strength = max(0.0, min(1.0, share * depth_pen * hub_w))
    return FundingPath(
        hops=hops,
        strength=strength,
        components={
            "initial_capital_share": share,
            "depth_penalty": depth_pen,
            "hub_weight": hub_w,
        },
        truncated=truncated,
        truncation_reasons=extra,
    )


def common_funder_strength(a: FundingPathResult, b: FundingPathResult) -> float:
    """Shared non-hub funders only. Exchange overlap is not ownership evidence."""
    a_roots = {p.hops[0].from_wallet for p in a.paths if p.hops and not p.hops[0].hub and p.strength > 0}
    b_roots = {p.hops[0].from_wallet for p in b.paths if p.hops and not p.hops[0].hub and p.strength > 0}
    shared = a_roots & b_roots
    if not shared:
        return 0.0
    return min(1.0, len(shared) / max(1, len(a_roots | b_roots)))
