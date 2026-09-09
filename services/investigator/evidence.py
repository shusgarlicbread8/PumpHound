"""Freeze and sanitize the only structured evidence allowed into model prompts."""

from __future__ import annotations

import hashlib
import json
from typing import Any

UNTRUSTED_FIELDS = (
    "name",
    "symbol",
    "metadata",
    "metadata_uri",
    "website",
    "twitter",
    "telegram",
    "memo",
    "description",
    "social",
    "socials",
    "profile_text",
    # Exa web-search hits (narrative_search): arbitrary third-party page text, same trust level
    # as the social-bio fields above — quote it, don't treat it as fact or instructions. "answer"
    # is Exa's own synthesized cross-source answer (narrative_search uses /answer, not /search) —
    # still built from the same scraped web pages, so it carries the same injection risk as a raw
    # snippet and gets the same treatment, not a lighter one just because it reads more cleanly.
    "answer",
    "title",
    "snippet",
)

STRUCTURED_SECTIONS = (
    "model_outputs",
    "predictions",
    "creator_profile",
    "creator_profiles",
    "holder_metrics",
    "wallet_history",
    "wallet_histories",
    "funding",
    "cohorts",
    "clusters",
    "tx_evidence",
    "narrative_search",
)


def pack_hash(pack: dict[str, Any]) -> str:
    canonical = {key: value for key, value in pack.items() if key != "sha256"}
    encoded = json.dumps(canonical, sort_keys=True, default=str, separators=(",", ":"))
    return hashlib.sha256(encoded.encode()).hexdigest()


def wrap_untrusted(text: Any, source: str, limit: int = 240) -> dict[str, Any]:
    raw = "" if text is None else str(text)
    truncated = raw[:limit]
    return {
        "quoted": truncated,
        "source": source,
        "truncated": len(raw) > limit,
        "untrusted": True,
        "length": len(raw),
    }


def _sanitize(value: Any, source: str, untrusted: list[dict[str, Any]]) -> Any:
    if isinstance(value, dict):
        clean: dict[str, Any] = {}
        for key, child in value.items():
            name = str(key)
            if name.lower() in UNTRUSTED_FIELDS:
                untrusted.append(wrap_untrusted(child, f"{source}:{name}"))
                continue
            clean[name] = _sanitize(child, f"{source}:{name}", untrusted)
        return clean
    if isinstance(value, list):
        return [_sanitize(child, f"{source}[{index}]", untrusted) for index, child in enumerate(value)]
    if value is None or isinstance(value, (str, int, float, bool)):
        return value
    return str(value)


def freeze_pack(payload: dict[str, Any], *, as_of_slot: int, knowledge_cutoff: str) -> dict[str, Any]:
    evidence: dict[str, Any] = {}
    untrusted: list[dict[str, Any]] = []
    for item in payload.get("evidence") or []:
        eid = str(item.get("id") or item.get("evidence_id") or "")
        if not eid:
            continue
        evidence[eid] = _sanitize(dict(item), eid, untrusted)
    pack = {
        "as_of_slot": as_of_slot,
        "knowledge_cutoff": knowledge_cutoff,
        "evidence": evidence,
        "structured": {
            section: _sanitize(payload[section], section, untrusted)
            for section in STRUCTURED_SECTIONS
            if section in payload
        },
        # Kept for display/audit only. providers.build_prompt_payload excludes it.
        "untrusted_quotes": untrusted,
    }
    pack["sha256"] = pack_hash(pack)
    return pack


def known_ids(pack: dict[str, Any]) -> set[str]:
    ids = set((pack.get("evidence") or {}).keys())

    def collect(value: Any) -> None:
        if isinstance(value, dict):
            for key in ("id", "evidence_id"):
                if value.get(key):
                    ids.add(str(value[key]))
            for child in value.values():
                collect(child)
        elif isinstance(value, list):
            for child in value:
                collect(child)

    collect(pack.get("structured") or {})
    return ids
