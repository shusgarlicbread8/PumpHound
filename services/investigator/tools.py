"""Read-only investigator tools. No SQL, shell, network or filesystem access."""

from __future__ import annotations

from typing import Any, Callable

from pumphound_ml.graph.facts import build_facts
from pumphound_ml.graph.funding import FundingBudget, trace_funding
from pumphound_ml.types import parse_ts

TOOL_NAMES = (
    "get_launch_snapshot",
    "get_model_predictions",
    "get_wallet_profile",
    "get_cluster_evidence",
    "trace_funding_paths",
    "get_creator_history",
    "get_trade_timeline",
    "get_cost_sensitivity",
    "resolve_evidence",
)

MAX_ROWS = 100
MAX_DEPTH = 3


class ToolGateway:
    def __init__(self, store: dict[str, Any], *, as_of_slot: int, knowledge_cutoff: str) -> None:
        self.store = store
        self.as_of_slot = as_of_slot
        self.knowledge_cutoff = knowledge_cutoff

    def _cutoff_ok(self, slot: int | None) -> bool:
        if slot is None:
            return True
        return int(slot) <= int(self.as_of_slot)

    def get_launch_snapshot(self, launch_id: str, as_of_slot: int | None = None) -> dict[str, Any]:
        slot = int(as_of_slot if as_of_slot is not None else self.as_of_slot)
        if not self._cutoff_ok(slot):
            return {"unavailable": True, "reason": "as_of_after_frozen_cutoff"}
        row = (self.store.get("launches") or {}).get(launch_id)
        if row is None:
            return {"unavailable": True, "reason": "unknown_launch"}
        return {"launch_id": launch_id, "as_of_slot": slot, **row}

    def get_model_predictions(self, launch_id: str, cutoff: str | None = None) -> dict[str, Any]:
        preds = (self.store.get("predictions") or {}).get(launch_id)
        if preds is None:
            return {"unavailable": True, "reason": "prediction_absent"}
        return {"launch_id": launch_id, "cutoff": cutoff or self.knowledge_cutoff, **preds}

    def get_wallet_profile(self, wallet: str, as_of_slot: int | None = None) -> dict[str, Any]:
        slot = int(as_of_slot if as_of_slot is not None else self.as_of_slot)
        if not self._cutoff_ok(slot):
            return {"unavailable": True, "reason": "as_of_after_frozen_cutoff"}
        row = (self.store.get("wallets") or {}).get(wallet)
        if row is None:
            return {"unavailable": True, "reason": "unknown_or_incomplete_history", "unknown": True}
        return {"wallet": wallet, "as_of_slot": slot, **row}

    def get_cluster_evidence(self, cluster_id: str, version: str | None = None) -> dict[str, Any]:
        row = (self.store.get("clusters") or {}).get(cluster_id)
        if row is None:
            return {"unavailable": True, "reason": "unknown_cluster"}
        if version and row.get("version") != version:
            return {"unavailable": True, "reason": "cluster_version_mismatch"}
        return row

    def trace_funding_paths(self, wallet: str, as_of_slot: int | None = None, max_depth: int = 3) -> dict[str, Any]:
        slot = int(as_of_slot if as_of_slot is not None else self.as_of_slot)
        if not self._cutoff_ok(slot):
            return {"unavailable": True, "reason": "as_of_after_frozen_cutoff"}
        depth = min(int(max_depth), MAX_DEPTH)
        events = self.store.get("events") or []
        if events:
            cutoff = parse_ts(self.knowledge_cutoff)
            batch = build_facts(events, cutoff or self.knowledge_cutoff, slot)
            result = trace_funding(wallet, slot, FundingBudget(max_hops=depth), facts=batch)
            return {
                "wallet": result.wallet,
                "unknown": result.unknown,
                "stopped_at_hubs": result.stopped_at_hubs,
                "competing_funders": result.competing_funders,
                "evidence_ids": result.evidence_ids[:MAX_ROWS],
            }
        cached = (self.store.get("funding") or {}).get(wallet)
        return cached or {"unavailable": True, "reason": "funding_pending", "unknown": True}

    def get_creator_history(self, creator: str, as_of_slot: int | None = None) -> dict[str, Any]:
        slot = int(as_of_slot if as_of_slot is not None else self.as_of_slot)
        if not self._cutoff_ok(slot):
            return {"unavailable": True, "reason": "as_of_after_frozen_cutoff"}
        row = (self.store.get("creators") or {}).get(creator)
        return row or {"unavailable": True, "reason": "creator_history_unknown", "unknown": True}

    def get_trade_timeline(self, launch_id: str, start_slot: int, end_slot: int) -> dict[str, Any]:
        end = min(int(end_slot), self.as_of_slot)
        start = int(start_slot)
        trades = [
            t
            for t in (self.store.get("trades") or [])
            if t.get("launch_id") == launch_id and start <= int(t.get("slot") or 0) <= end
        ]
        return {"launch_id": launch_id, "trades": trades[:MAX_ROWS], "truncated": len(trades) > MAX_ROWS}

    def get_cost_sensitivity(self, launch_id: str, policy_version: str) -> dict[str, Any]:
        row = (self.store.get("costs") or {}).get(launch_id)
        if row is None:
            return {"unavailable": True, "reason": "cost_sensitivity_unknown"}
        return {"launch_id": launch_id, "policy_version": policy_version, **row}

    def resolve_evidence(self, evidence_ids: list[str]) -> dict[str, Any]:
        pack = self.store.get("evidence") or {}
        resolved = {}
        missing = []
        for eid in evidence_ids[:MAX_ROWS]:
            if eid in pack:
                resolved[eid] = pack[eid]
            else:
                missing.append(eid)
        return {"resolved": resolved, "missing": missing}


def tool_specs() -> list[dict[str, Any]]:
    return [
        {
            "type": "function",
            "function": {
                "name": name,
                "description": f"Read-only PumpHound tool {name}",
                "parameters": {"type": "object", "additionalProperties": True},
            },
        }
        for name in TOOL_NAMES
    ]


def dispatch(gateway: ToolGateway, name: str, arguments: dict[str, Any]) -> dict[str, Any]:
    if name not in TOOL_NAMES:
        return {"error": "unknown_or_forbidden_tool"}
    fn: Callable[..., dict[str, Any]] = getattr(gateway, name)
    return fn(**arguments)
