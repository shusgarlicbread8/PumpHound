"""Owner-resolved transfers. ATA addresses are not treated as person-wallets."""

from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime
from typing import Iterable

from pumphound_ml.clock import event_visible
from pumphound_ml.types import ChainEvent, parse_atomic


@dataclass
class TransferRecord:
    event_id: str
    source_owner: str | None
    destination_owner: str | None
    source_account: str
    destination_account: str
    mint: str
    amount_atomic: int
    decimals: int
    role: str
    slot: int
    known_at: datetime
    owner_resolved: bool


def extract_transfers(
    events: Iterable[ChainEvent],
    knowledge_cutoff: datetime,
    cutoff_slot: int,
) -> list[TransferRecord]:
    owners: dict[str, str] = {}
    out: list[TransferRecord] = []
    for event in events:
        if not event_visible(event, cutoff_slot, knowledge_cutoff):
            continue
        if event.payload_kind != "transfer.observed":
            continue
        p = event.payload
        src_acct = str(p.get("source_account") or "")
        dst_acct = str(p.get("destination_account") or "")
        src_owner = p.get("source_owner")
        dst_owner = p.get("destination_owner")
        if src_owner:
            owners[src_acct] = str(src_owner)
        if dst_owner:
            owners[dst_acct] = str(dst_owner)
        resolved_src = str(src_owner or owners.get(src_acct) or "") or None
        resolved_dst = str(dst_owner or owners.get(dst_acct) or "") or None
        amount = p.get("amount") or {}
        atomic = parse_atomic(amount.get("atomic") if isinstance(amount, dict) else p.get("amount_atomic"))
        mint = str(amount.get("mint") if isinstance(amount, dict) else p.get("mint") or "")
        decimals = int(amount.get("decimals") if isinstance(amount, dict) else p.get("decimals") or 9)
        out.append(
            TransferRecord(
                event_id=event.event_id,
                source_owner=resolved_src,
                destination_owner=resolved_dst,
                source_account=src_acct,
                destination_account=dst_acct,
                mint=mint,
                amount_atomic=atomic,
                decimals=decimals,
                role=str(p.get("transfer_role") or "unknown"),
                slot=event.slot,
                known_at=event.known_at,
                owner_resolved=bool(resolved_src and resolved_dst),
            )
        )
    return out
