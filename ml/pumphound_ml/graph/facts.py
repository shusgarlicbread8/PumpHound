"""Immutable factual graph layer. Transfers are facts; ownership is not."""

from __future__ import annotations

from collections import defaultdict
from datetime import datetime
from typing import Iterable

from pumphound_ml.clock import event_visible, parse_cutoff
from pumphound_ml.io import content_id
from pumphound_ml.types import (
    ChainEvent,
    GraphFact,
    GraphFactBatch,
    GraphNode,
    parse_atomic,
    parse_slot,
    parse_ts,
)

NODE_PREFIX = {
    "Wallet": "wallet",
    "Token": "token",
    "Launch": "launch",
    "Market": "market",
    "Transaction": "tx",
    "FundingSource": "fund",
}

IGNORE_TRANSFER_ROLES = frozenset({"rent", "fee", "trade_settlement"})


def _node_id(kind: str, key: str) -> str:
    return f"{NODE_PREFIX.get(kind, kind.lower())}:{key}"


def _upsert_node(
    nodes: dict[str, GraphNode],
    kind: str,
    key: str,
    slot: int,
    known_at: datetime,
    **attrs: object,
) -> str:
    nid = _node_id(kind, key)
    existing = nodes.get(nid)
    if existing is None:
        nodes[nid] = GraphNode(
            node_id=nid,
            node_type=kind,
            public_key=key if kind in {"Wallet", "Token", "Market", "FundingSource"} else None,
            first_seen_slot=slot,
            known_at=known_at,
            attributes=dict(attrs),
        )
        return nid
    existing.first_seen_slot = min(existing.first_seen_slot, slot)
    if known_at < existing.known_at:
        existing.known_at = known_at
    existing.attributes.update({k: v for k, v in attrs.items() if v is not None})
    return nid


def _fact(
    *,
    source: str,
    target: str,
    edge_type: str,
    event: ChainEvent,
    mint: str | None = None,
    amount: int | None = None,
    **attrs: object,
) -> GraphFact:
    return GraphFact(
        fact_id=content_id("fact", event.event_id, source, target, edge_type),
        source_node=source,
        target_node=target,
        edge_type=edge_type,
        event_id=event.event_id,
        slot=event.slot,
        known_at=event.known_at,
        canonicality=event.canonicality,
        asset_mint=mint,
        amount_atomic=amount,
        attributes={"signature": event.signature, "decoder_version": event.provenance.get("decoder_version"), **attrs},
    )


def build_facts(
    events: Iterable[ChainEvent | dict],
    knowledge_cutoff: datetime | str,
    cutoff_slot: int | None = None,
) -> GraphFactBatch:
    """Convert canonical events into typed factual nodes/edges at a knowledge cutoff."""
    cutoff = parse_cutoff(knowledge_cutoff)
    raw: list[ChainEvent] = []
    for item in events:
        raw.append(item if isinstance(item, ChainEvent) else ChainEvent.from_mapping(item))
    raw.sort(key=lambda e: e.sort_key())
    if cutoff_slot is None:
        visible_slots = [e.slot for e in raw if e.known_at <= cutoff]
        cutoff_slot = max(visible_slots) if visible_slots else 0

    nodes: dict[str, GraphNode] = {}
    facts: list[GraphFact] = []
    rejected = 0
    owners: dict[str, str] = {}

    for event in raw:
        if not event_visible(event, cutoff_slot, cutoff):
            if event.slot > cutoff_slot or event.known_at > cutoff:
                rejected += 1
            continue
        kind = event.payload_kind
        p = event.payload
        if kind == "launch.created":
            creator = str(p.get("creator") or "")
            mint = str(p.get("mint") or "")
            launch_id = str(p.get("launch_id") or mint)
            market = str(p.get("market") or "")
            w = _upsert_node(nodes, "Wallet", creator, event.slot, event.known_at, role="creator")
            tok = _upsert_node(
                nodes,
                "Token",
                mint,
                event.slot,
                event.known_at,
                decimals=p.get("base_decimals"),
                quote_mint=p.get("quote_mint"),
            )
            ln = _upsert_node(
                nodes,
                "Launch",
                launch_id,
                event.slot,
                event.known_at,
                creator=creator,
                mint=mint,
                protocol=event.protocol,
            )
            mk = _upsert_node(nodes, "Market", market, event.slot, event.known_at, venue=event.protocol)
            tx = _upsert_node(nodes, "Transaction", event.signature, event.slot, event.known_at)
            facts.append(_fact(source=w, target=ln, edge_type="CREATED", event=event))
            facts.append(_fact(source=ln, target=tok, edge_type="ISSUED", event=event))
            facts.append(_fact(source=tok, target=mk, edge_type="TRADED_ON", event=event))
            facts.append(
                _fact(
                    source=tx,
                    target=w,
                    edge_type="PAID_FEE_TO",
                    event=event,
                    mint=str(p.get("quote_mint") or ""),
                )
            )
        elif kind == "trade.executed":
            trader = str(p.get("trader") or "")
            mint = str(p.get("mint") or "")
            side = str(p.get("side") or "buy").lower()
            quote = parse_atomic(p.get("quote_amount_atomic"))
            w = _upsert_node(nodes, "Wallet", trader, event.slot, event.known_at)
            tok = _upsert_node(nodes, "Token", mint, event.slot, event.known_at)
            _upsert_node(nodes, "Transaction", event.signature, event.slot, event.known_at)
            edge = "BOUGHT" if side == "buy" else "SOLD"
            facts.append(
                _fact(
                    source=w,
                    target=tok,
                    edge_type=edge,
                    event=event,
                    mint=str(p.get("quote_mint") or ""),
                    amount=quote,
                    side=side,
                    launch_id=p.get("launch_id"),
                    base_amount_atomic=str(p.get("base_amount_atomic") or "0"),
                )
            )
        elif kind == "transfer.observed":
            role = str(p.get("transfer_role") or "unknown")
            src_owner = p.get("source_owner")
            dst_owner = p.get("destination_owner")
            src_acct = str(p.get("source_account") or "")
            dst_acct = str(p.get("destination_account") or "")
            if src_owner:
                owners[src_acct] = str(src_owner)
            if dst_owner:
                owners[dst_acct] = str(dst_owner)
            src = str(src_owner or owners.get(src_acct) or src_acct)
            dst = str(dst_owner or owners.get(dst_acct) or dst_acct)
            amount = p.get("amount") or {}
            atomic = parse_atomic(amount.get("atomic") if isinstance(amount, dict) else p.get("amount_atomic"))
            mint = str(amount.get("mint") if isinstance(amount, dict) else p.get("mint") or "")
            if role in IGNORE_TRANSFER_ROLES:
                continue
            sw = _upsert_node(nodes, "Wallet", src, event.slot, event.known_at)
            dw = _upsert_node(nodes, "Wallet", dst, event.slot, event.known_at)
            facts.append(
                _fact(
                    source=sw,
                    target=dw,
                    edge_type="TRANSFERRED_TO",
                    event=event,
                    mint=mint,
                    amount=atomic,
                    transfer_role=role,
                    source_account=src_acct,
                    destination_account=dst_acct,
                    ata_not_person=src_acct != src or dst_acct != dst,
                )
            )
        elif kind == "launch.migrated":
            launch_id = str(p.get("launch_id") or "")
            old_m = str(p.get("old_market") or "")
            new_m = str(p.get("new_market") or "")
            ln = _upsert_node(nodes, "Launch", launch_id, event.slot, event.known_at)
            mk = _upsert_node(nodes, "Market", new_m, event.slot, event.known_at, migrated_from=old_m)
            facts.append(_fact(source=ln, target=mk, edge_type="MIGRATED_TO", event=event))
        elif kind == "launch.completed":
            launch_id = str(p.get("launch_id") or "")
            market = str(p.get("market") or "")
            ln = _upsert_node(nodes, "Launch", launch_id, event.slot, event.known_at, completed=True)
            mk = _upsert_node(nodes, "Market", market, event.slot, event.known_at)
            facts.append(_fact(source=ln, target=mk, edge_type="MIGRATED_TO", event=event, completed=True))

    seen_facts: set[str] = set()
    unique_facts: list[GraphFact] = []
    for fact in facts:
        if fact.fact_id in seen_facts:
            continue
        seen_facts.add(fact.fact_id)
        unique_facts.append(fact)

    coverage = {
        "node_count": len(nodes),
        "fact_count": len(unique_facts),
        "wallet_count": sum(1 for n in nodes.values() if n.node_type == "Wallet"),
        "unknown_history": rejected > 0,
    }
    return GraphFactBatch(
        algorithm_version="facts-v1",
        knowledge_cutoff=cutoff,
        cutoff_slot=cutoff_slot,
        nodes=list(nodes.values()),
        facts=unique_facts,
        coverage=coverage,
        rejected_future_count=rejected,
    )


def wallet_first_buy(
    batch: GraphFactBatch,
    wallet: str,
    token: str | None = None,
) -> GraphFact | None:
    nid = f"wallet:{wallet}"
    tok = f"token:{token}" if token else None
    buys = [
        f
        for f in batch.facts
        if f.edge_type == "BOUGHT" and f.source_node == nid and (tok is None or f.target_node == tok)
    ]
    buys.sort(key=lambda f: f.slot)
    return buys[0] if buys else None


def inbound_transfers(batch: GraphFactBatch, wallet: str) -> list[GraphFact]:
    nid = f"wallet:{wallet}"
    return [f for f in batch.facts if f.edge_type == "TRANSFERRED_TO" and f.target_node == nid]
