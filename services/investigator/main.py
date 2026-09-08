"""Investigator HTTP gateway. Bind 127.0.0.1:8092."""

from __future__ import annotations

import uuid
from datetime import UTC, datetime
from typing import Any

import uvicorn
from fastapi import FastAPI, HTTPException
from pydantic import BaseModel, Field

try:
    from .cache import InvestigationCache
    from .evidence import freeze_pack, known_ids, pack_hash
    from .journal import InvestigationJournal
    from .providers import (
        choose_provider,
        complete_with_metadata,
        deterministic_summary,
        load_prompt,
    )
    from .tools import TOOL_NAMES, ToolGateway, dispatch
    from .validation import ValidationError, policy_effect, validate_response
except ImportError:  # python services/investigator/main.py
    from cache import InvestigationCache
    from evidence import freeze_pack, known_ids, pack_hash
    from journal import InvestigationJournal
    from providers import (
        choose_provider,
        complete_with_metadata,
        deterministic_summary,
        load_prompt,
    )
    from tools import TOOL_NAMES, ToolGateway, dispatch
    from validation import ValidationError, policy_effect, validate_response

app = FastAPI(title="pumphound-investigator", version="0.1.0")
CACHE = InvestigationCache()
JOURNAL = InvestigationJournal()
JOBS: dict[str, dict[str, Any]] = {}
ROLES = ("quant", "wallet", "creator", "risk", "contrarian", "committee")


class InvestigateIn(BaseModel):
    subject_type: str
    subject_id: str
    as_of_slot: int
    knowledge_cutoff: str
    roles: list[str] = Field(default_factory=lambda: ["committee"])
    evidence: dict[str, Any] = Field(default_factory=dict)
    timeout_s: float = 15.0


def run_roles(body: InvestigateIn) -> dict[str, Any]:
    pack = freeze_pack(body.evidence, as_of_slot=body.as_of_slot, knowledge_cutoff=body.knowledge_cutoff)
    provider = choose_provider()
    outputs: dict[str, Any] = {}
    validated_outputs: dict[str, Any] = {}
    specialist = [r for r in body.roles if r in {"quant", "wallet", "creator", "risk"}]
    sequence = specialist + [r for r in body.roles if r == "contrarian"] + [r for r in body.roles if r == "committee"]
    if not sequence:
        sequence = ["committee"]

    for role in sequence:
        if role in {"contrarian", "committee"} and validated_outputs:
            pack = {
                **pack,
                "structured": {
                    **(pack.get("structured") or {}),
                    "model_outputs": dict(validated_outputs),
                },
            }
            pack["sha256"] = pack_hash(pack)
        ids = known_ids(pack)
        _, prompt_version = load_prompt(role)
        cache_key = CACHE.key(pack["sha256"], prompt_version, provider["model"], role)
        cached = CACHE.get(cache_key)
        if cached:
            outputs[role] = cached
            validated_outputs[role] = cached["advisory"]
            continue
        completion = complete_with_metadata(role, pack, timeout_s=body.timeout_s)
        advisory = completion.response
        degraded = completion.degraded
        try:
            report = validate_response(advisory, ids)
        except ValidationError as exc:
            advisory = deterministic_summary(role, pack)
            report = validate_response(advisory, ids)
            report["fallback_reason"] = exc.code
            degraded = True
        report["provider_status"] = completion.validation_status
        report["degraded"] = degraded
        JOURNAL.append(
            request_hash=completion.request_hash,
            provider=completion.provider,
            model=completion.model,
            prompt_version=completion.prompt_version,
            evidence_ids=completion.evidence_ids,
            response=advisory,
            validation=report,
            degraded=degraded,
            latency_ms=completion.latency_ms,
            response_format=completion.response_format,
        )
        result = {
            "advisory": advisory,
            "provider": completion.provider,
            "model_id": completion.model,
            "prompt_version": completion.prompt_version,
            "validation": report,
            "degraded": degraded,
            "policy_effect": policy_effect(advisory),
        }
        CACHE.put(cache_key, result)
        outputs[role] = result
        validated_outputs[role] = advisory

    return {
        "evidence_pack_sha256": pack["sha256"],
        "provider": provider["name"],
        "as_of_slot": body.as_of_slot,
        "roles": outputs,
        "tools_allowed": list(TOOL_NAMES),
        "policy_effect": False,
    }


@app.post("/internal/investigate")
def investigate(body: InvestigateIn) -> dict[str, Any]:
    for role in body.roles:
        if role not in ROLES:
            raise HTTPException(status_code=422, detail=f"unsupported_role:{role}")
    job_id = str(uuid.uuid4())
    result = run_roles(body)
    JOBS[job_id] = {
        "job_id": job_id,
        "created_at": datetime.now(UTC).isoformat(),
        **result,
    }
    return {"job_id": job_id, **result}


@app.post("/internal/tools/{name}")
def call_tool(name: str, body: dict[str, Any]) -> dict[str, Any]:
    if name not in TOOL_NAMES:
        raise HTTPException(status_code=422, detail="forbidden_tool")
    gateway = ToolGateway(body.get("store") or {}, as_of_slot=int(body.get("as_of_slot") or 0), knowledge_cutoff=str(body.get("knowledge_cutoff") or ""))
    return dispatch(gateway, name, body.get("arguments") or {})


@app.get("/internal/investigations/{job_id}")
def get_job(job_id: str) -> dict[str, Any]:
    row = JOBS.get(job_id)
    if row is None:
        raise HTTPException(status_code=404, detail="unknown_investigation")
    return row


@app.get("/internal/health")
def health() -> dict[str, Any]:
    provider = choose_provider()
    return {"status": "ok", "bind": "127.0.0.1:8092", "provider": provider["name"], "model": provider["model"]}


def main() -> None:
    import os

    host = os.environ.get("PUMPHOUND_INVESTIGATOR_BIND", "127.0.0.1")
    uvicorn.run(app, host=host, port=8092, log_level="info")


if __name__ == "__main__":
    main()
