"""Deterministic research-plane replay. Wall-clock is excluded from the result hash."""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from pumphound_ml.clock import event_visible
from pumphound_ml.features.extract import extract_features
from pumphound_ml.io import dump_json, load_jsonl, load_yaml, sha256_canonical
from pumphound_ml.replay.manifest import load_manifest, validate_manifest
from pumphound_ml.types import (
    WSOL_MINT,
    ReplayReport,
    load_events,
    parse_atomic,
    parse_ts,
)

# Versioned Pump curve split (0.95% + 0.30% = 1.25%). Not an unversioned constant.
FEE_SCHEDULE_VERSION = "pump-curve-125bps-2026-09"
PROTOCOL_FEE_BPS = 95
CREATOR_FEE_BPS = 30


@dataclass
class Lot:
    lot_id: str
    mint: str
    remaining: int
    cost_atomic: int
    acquired_seq: int


@dataclass
class Reservation:
    order_id: str
    mint: str
    amount: int
    released: bool = False


@dataclass
class PaperEngine:
    seed: int
    cash: dict[str, int] = field(default_factory=lambda: {WSOL_MINT: 0})
    movements: list[dict[str, Any]] = field(default_factory=list)
    lots: list[Lot] = field(default_factory=list)
    reservations: list[Reservation] = field(default_factory=list)
    decisions: list[dict[str, Any]] = field(default_factory=list)
    fills: list[dict[str, Any]] = field(default_factory=list)
    seq: int = 0
    reservation_violations: int = 0
    lot_violations: int = 0
    future_dependency_count: int = 0

    def fund(self, mint: str, amount: int, at: str) -> None:
        self._move(mint, amount, "initial_funding", at)

    def _move(self, mint: str, signed: int, category: str, at: str, order_id: str | None = None) -> None:
        if signed == 0:
            return
        self.cash[mint] = self.cash.get(mint, 0) + signed
        self.movements.append(
            {
                "mint": mint,
                "signed_atomic": signed,
                "category": category,
                "at": at,
                "order_id": order_id,
                "seq": self.seq,
            }
        )
        self.seq += 1

    def spendable(self, mint: str) -> int:
        reserved = sum(r.amount for r in self.reservations if r.mint == mint and not r.released)
        return self.cash.get(mint, 0) - reserved

    def reserve(self, order_id: str, mint: str, amount: int) -> None:
        if self.spendable(mint) < amount:
            self.reservation_violations += 1
            return
        self.reservations.append(Reservation(order_id=order_id, mint=mint, amount=amount))

    def release(self, order_id: str) -> None:
        for r in self.reservations:
            if r.order_id == order_id:
                r.released = True

    def quote_buy(self, gross: int, vb: int, vq: int) -> tuple[int, int, int, int]:
        proto = gross * PROTOCOL_FEE_BPS // 10_000
        creator = gross * CREATOR_FEE_BPS // 10_000
        reserve_in = gross - proto - creator
        if reserve_in <= 0 or vb <= 0 or vq <= 0:
            return 0, reserve_in, proto, creator
        k = vb * vq
        new_q = vq + reserve_in
        new_b = k // new_q
        out = vb - new_b
        return out, reserve_in, proto, creator

    def quote_sell(self, base_in: int, vb: int, vq: int) -> tuple[int, int, int]:
        if base_in <= 0 or vb <= 0 or vq <= 0:
            return 0, 0, 0
        k = vb * vq
        new_b = vb + base_in
        new_q = k // new_b
        gross_out = vq - new_q
        proto = gross_out * PROTOCOL_FEE_BPS // 10_000
        creator = gross_out * CREATOR_FEE_BPS // 10_000
        net = gross_out - proto - creator
        return net, proto, creator

    def fill_buy(
        self,
        *,
        order_id: str,
        mint: str,
        quote_mint: str,
        gross: int,
        vb: int,
        vq: int,
        at: str,
        slot: int,
        min_out: int,
    ) -> dict[str, Any] | None:
        out, reserve_in, proto, creator = self.quote_buy(gross, vb, vq)
        if out <= 0:
            self.release(order_id)
            return None
        if out < min_out:
            self.release(order_id)
            return {"status": "REJECTED_SLIPPAGE", "order_id": order_id}
        self.release(order_id)
        if self.spendable(quote_mint) < gross:
            self.reservation_violations += 1
            return {"status": "RISK_REJECTED", "order_id": order_id}
        self._move(quote_mint, -reserve_in, "trade_input", at, order_id)
        self._move(quote_mint, -proto, "fee", at, order_id)
        self._move(quote_mint, -creator, "fee", at, order_id)
        self._move(mint, out, "trade_output", at, order_id)
        self.lots.append(Lot(lot_id=f"lot-{order_id}", mint=mint, remaining=out, cost_atomic=gross, acquired_seq=self.seq))
        fill = {
            "fill_id": f"fill-{order_id}",
            "order_id": order_id,
            "side": "buy",
            "input_mint": quote_mint,
            "input_atomic": gross,
            "output_mint": mint,
            "output_atomic": out,
            "reserve_input": reserve_in,
            "fees": proto + creator,
            "slot": slot,
            "at": at,
        }
        self.fills.append(fill)
        return fill

    def fill_sell(
        self,
        *,
        order_id: str,
        mint: str,
        quote_mint: str,
        base_in: int,
        vb: int,
        vq: int,
        at: str,
        slot: int,
    ) -> dict[str, Any] | None:
        available = sum(lot.remaining for lot in self.lots if lot.mint == mint)
        if base_in > available:
            self.lot_violations += 1
            self.release(order_id)
            return None
        net, proto, creator = self.quote_sell(base_in, vb, vq)
        if net <= 0:
            self.release(order_id)
            return None
        remaining_need = base_in
        for lot in self.lots:
            if lot.mint != mint or lot.remaining <= 0 or remaining_need <= 0:
                continue
            take = min(lot.remaining, remaining_need)
            if take > lot.remaining:
                self.lot_violations += 1
                take = lot.remaining
            lot.remaining -= take
            remaining_need -= take
        if remaining_need != 0:
            self.lot_violations += 1
        self.release(order_id)
        self._move(mint, -base_in, "trade_input", at, order_id)
        # Fees are deducted from output; debit them from gross so they are not applied twice.
        self._move(quote_mint, net + proto + creator, "trade_output", at, order_id)
        self._move(quote_mint, -proto, "fee", at, order_id)
        self._move(quote_mint, -creator, "fee", at, order_id)
        fill = {
            "fill_id": f"fill-{order_id}",
            "order_id": order_id,
            "side": "sell",
            "input_mint": mint,
            "input_atomic": base_in,
            "output_mint": quote_mint,
            "output_atomic": net,
            "fees": proto + creator,
            "slot": slot,
            "at": at,
        }
        self.fills.append(fill)
        return fill

    def ledger_balanced(self) -> bool:
        by_mint: dict[str, int] = {}
        for mv in self.movements:
            by_mint[mv["mint"]] = by_mint.get(mv["mint"], 0) + mv["signed_atomic"]
        if by_mint != {k: v for k, v in self.cash.items() if v != 0 or k in by_mint}:
            # Compare reconstructed sums to cash.
            for mint, total in by_mint.items():
                if self.cash.get(mint, 0) != total:
                    return False
        for mint, total in by_mint.items():
            if total < 0:
                return False
        if any(self.spendable(m) < 0 for m in self.cash):
            return False
        if any(lot.remaining < 0 for lot in self.lots):
            return False
        return True

    def deterministic_payload(self) -> dict[str, Any]:
        return {
            "decisions": [
                {k: d[k] for k in ("action", "launch_id", "reason_codes", "cutoff_slot", "window") if k in d}
                for d in self.decisions
            ],
            "fills": [
                {
                    k: f[k]
                    for k in (
                        "order_id",
                        "side",
                        "input_mint",
                        "input_atomic",
                        "output_mint",
                        "output_atomic",
                        "slot",
                    )
                    if k in f
                }
                for f in self.fills
            ],
            "movements": [
                {k: m[k] for k in ("mint", "signed_atomic", "category", "seq") if k in m}
                for m in self.movements
            ],
            "lots": [{"lot_id": lot.lot_id, "remaining": lot.remaining, "mint": lot.mint} for lot in self.lots],
            "seed": self.seed,
        }


def _reserves(events, launch_id: str, cutoff_slot: int) -> tuple[int, int]:
    vb, vq = 1_073_000_000_000_000, 30_000_000_000
    for event in events:
        if event.slot > cutoff_slot:
            break
        p = event.payload
        if event.payload_kind == "launch.created" and str(p.get("launch_id") or "") == launch_id:
            vb = parse_atomic(p.get("virtual_base_atomic")) or vb
            vq = parse_atomic(p.get("virtual_quote_atomic")) or vq
        if event.payload_kind == "trade.executed" and str(p.get("launch_id") or "") == launch_id:
            if p.get("virtual_base_atomic") is not None:
                vb = parse_atomic(p.get("virtual_base_atomic"))
            if p.get("virtual_quote_atomic") is not None:
                vq = parse_atomic(p.get("virtual_quote_atomic"))
    return vb, vq


def _policy_enter(values: dict[str, float | None], policy: dict[str, Any]) -> tuple[bool, list[str]]:
    entry = policy.get("entry") or {}
    reasons: list[str] = []
    age = values.get("age_seconds")
    if age is None:
        return False, ["age_unknown"]
    if age < float(entry.get("minimum_age_seconds", 30)):
        reasons.append("too_young")
    if age > float(entry.get("maximum_age_seconds", 900)):
        reasons.append("too_old")
    buyers = values.get("unique_buyers_30") or values.get("unique_buyers_60")
    if buyers is None or buyers < float(entry.get("minimum_unique_buyers", 25)):
        reasons.append("buyers")
    imb = values.get("buy_sell_imbalance_30")
    if imb is None or imb < float(entry.get("minimum_buy_sell_imbalance_30s", 0.20)):
        reasons.append("imbalance")
    ret = values.get("return_30")
    if ret is None or ret < float(entry.get("minimum_return_30s", 0.05)):
        reasons.append("return_low")
    if ret is not None and ret > float(entry.get("maximum_return_30s", 0.50)):
        reasons.append("return_high")
    return (len(reasons) == 0, reasons)


def run_replay(manifest_path: str) -> ReplayReport:
    manifest = load_manifest(manifest_path)
    validate_manifest(manifest)
    events = load_events(load_jsonl(manifest.events_path))
    knowledge_cutoff = parse_ts(manifest.knowledge_cutoff) or max((e.known_at for e in events), default=datetime(1970, 1, 1, tzinfo=timezone.utc))
    policy = {}
    try:
        policy = load_yaml(f"ml/configs/{manifest.policy_version}.yaml")
    except FileNotFoundError:
        policy = {"entry": {}}

    engine = PaperEngine(seed=manifest.seed)
    initial = int((manifest.raw.get("portfolio") or {}).get("initial_quote_atomic") or 2_000_000_000)
    engine.fund(WSOL_MINT, initial, "1970-01-01T00:00:00.000000Z")

    launches = [
        e
        for e in events
        if e.payload_kind == "launch.created" and event_visible(e, e.slot, knowledge_cutoff)
    ]
    future_deps = 0
    for event in events:
        if event.known_at > knowledge_cutoff or event.slot > max((e.slot for e in events), default=0):
            future_deps += 1
        if event.available_from_slot > event.slot:
            future_deps += 1

    scripted = list(manifest.raw.get("scripted_orders") or [])
    if scripted:
        for i, order in enumerate(scripted):
            launch_id = str(order["launch_id"])
            slot = int(order.get("slot") or 0)
            at = str(order.get("at") or "1970-01-01T00:00:00.000000Z")
            vb, vq = _reserves(events, launch_id, slot)
            mint = str(order.get("mint") or "")
            oid = f"ord-{i:04d}"
            if order.get("side") == "buy":
                gross = int(order.get("input_atomic") or 100_000_000)
                engine.reserve(oid, WSOL_MINT, gross)
                fill = engine.fill_buy(
                    order_id=oid,
                    mint=mint,
                    quote_mint=WSOL_MINT,
                    gross=gross,
                    vb=vb,
                    vq=vq,
                    at=at,
                    slot=slot,
                    min_out=int(order.get("min_out") or 1),
                )
                engine.decisions.append(
                    {
                        "action": "PAPER_BUY" if fill and fill.get("status") != "REJECTED_SLIPPAGE" else "ABSTAIN",
                        "launch_id": launch_id,
                        "reason_codes": ["scripted"],
                        "cutoff_slot": slot,
                        "window": int(order.get("window") or 60),
                    }
                )
            else:
                base_in = int(order.get("input_atomic") or 0)
                engine.reserve(oid, mint, base_in)
                fill = engine.fill_sell(
                    order_id=oid,
                    mint=mint,
                    quote_mint=WSOL_MINT,
                    base_in=base_in,
                    vb=vb,
                    vq=vq,
                    at=at,
                    slot=slot,
                )
                engine.decisions.append(
                    {
                        "action": "PAPER_SELL" if fill else "ABSTAIN",
                        "launch_id": launch_id,
                        "reason_codes": ["scripted"],
                        "cutoff_slot": slot,
                        "window": int(order.get("window") or 60),
                    }
                )
    else:
        for launch in launches:
            lid = str(launch.payload.get("launch_id") or "")
            mint = str(launch.payload.get("mint") or "")
            for window in (60,):
                from pumphound_ml.clock import cutoff_from_creation

                cutoff_slot, cutoff_time = cutoff_from_creation(launch.event_time, window, launch.slot)
                if cutoff_time > knowledge_cutoff:
                    engine.future_dependency_count += 1
                    continue
                vec = extract_features(
                    events,
                    launch_id=lid,
                    window_seconds=window,
                    cutoff_slot=cutoff_slot,
                    knowledge_cutoff=min(cutoff_time, knowledge_cutoff),
                    creation_time=launch.event_time,
                    creation_slot=launch.slot,
                    creator=str(launch.payload.get("creator") or ""),
                )
                ok, reasons = _policy_enter(vec.values, policy)
                action = "PAPER_BUY" if ok else "ABSTAIN"
                engine.decisions.append(
                    {
                        "action": action,
                        "launch_id": lid,
                        "reason_codes": reasons or ["policy_pass"],
                        "cutoff_slot": cutoff_slot,
                        "window": window,
                    }
                )
                if not ok:
                    continue
                vb, vq = _reserves(events, lid, cutoff_slot)
                oid = f"ord-{lid}-{window}"
                engine.reserve(oid, WSOL_MINT, 100_000_000)
                engine.fill_buy(
                    order_id=oid,
                    mint=mint,
                    quote_mint=WSOL_MINT,
                    gross=100_000_000,
                    vb=vb,
                    vq=vq,
                    at=cutoff_time.isoformat(),
                    slot=cutoff_slot,
                    min_out=1,
                )

    engine.future_dependency_count += future_deps
    digest = sha256_canonical(engine.deterministic_payload())
    out_dir = Path("artifacts/replays") / digest[:12]
    summary = {
        "manifest_sha256": manifest.sha256,
        "deterministic_result_sha256": digest,
        "decision_count": len(engine.decisions),
        "fill_count": len(engine.fills),
        "ledger_balanced": engine.ledger_balanced(),
        "reservation_violations": engine.reservation_violations,
        "lot_violations": engine.lot_violations,
        "future_dependency_count": engine.future_dependency_count,
        "payload": engine.deterministic_payload(),
    }
    summary_path = dump_json(out_dir / "summary.json", summary)
    return ReplayReport(
        manifest_sha256=manifest.sha256,
        deterministic_result_sha256=digest,
        decision_count=len(engine.decisions),
        fill_count=len(engine.fills),
        ledger_balanced=engine.ledger_balanced(),
        reservation_violations=engine.reservation_violations,
        lot_violations=engine.lot_violations,
        future_dependency_count=engine.future_dependency_count,
        summary_uri=str(summary_path),
        policy_version=manifest.policy_version,
    )
