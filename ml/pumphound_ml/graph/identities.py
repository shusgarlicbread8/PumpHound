"""Wallet identity safety: never destructively merge nodes."""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime

from pumphound_ml.graph.labels import IdentityLabel, labels_as_of
from pumphound_ml.types import GraphFactBatch, GraphNode


@dataclass
class WalletIdentity:
    wallet: str
    node_id: str
    first_seen_slot: int
    known_at: datetime
    label: str | None = None
    label_source: str | None = None
    cluster_ids: list[str] = field(default_factory=list)
    merged: bool = False


def resolve_identities(
    batch: GraphFactBatch,
    labels: list[IdentityLabel] | None = None,
    memberships: dict[str, list[str]] | None = None,
) -> list[WalletIdentity]:
    """One identity record per wallet public key. Clusters do not change wallet identity."""
    asof = labels_as_of(labels or [], batch.cutoff_slot, batch.knowledge_cutoff)
    out: list[WalletIdentity] = []
    seen: set[str] = set()
    for node in batch.nodes:
        if node.node_type != "Wallet" or not node.public_key:
            continue
        if node.public_key in seen:
            continue
        seen.add(node.public_key)
        lab = asof.get(node.public_key)
        out.append(
            WalletIdentity(
                wallet=node.public_key,
                node_id=node.node_id,
                first_seen_slot=node.first_seen_slot,
                known_at=node.known_at,
                label=lab.label if lab else None,
                label_source=lab.source_uri if lab else None,
                cluster_ids=list((memberships or {}).get(node.public_key, [])),
                merged=False,
            )
        )
    return out


def assert_no_wallet_merge(nodes: list[GraphNode]) -> None:
    wallets = [n for n in nodes if n.node_type == "Wallet" and n.public_key]
    keys = [n.public_key for n in wallets]
    if len(keys) != len(set(keys)):
        raise ValueError("wallet nodes must not be destructively merged")
