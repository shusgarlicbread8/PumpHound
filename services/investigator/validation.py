"""Strict advisory response validation and evidence grounding."""

from __future__ import annotations

import re
from typing import Any

from pydantic import BaseModel, ConfigDict, Field
from pydantic import ValidationError as PydanticValidationError

LIVE_TRADE = re.compile(
    r"\b(buy now|sell now|market buy|market sell|place order|submit (?:the )?transaction|"
    r"sign (?:the )?(?:tx|transaction)|live (?:trade|order)|broadcast|send it|"
    r"execute (?:now|live)|enter live|go long|go short)\b",
    re.IGNORECASE,
)
PROB_RE = re.compile(r"\b(?:p|prob(?:ability)?|confidence)\s*[:=]\s*([+-]?\d+(?:\.\d+)?)\b", re.IGNORECASE)
RELATIONSHIP_RE = re.compile(
    r"\b(coordinat(?:e|ed|es|ion|ing)?|same (?:owner|operator|entity)|"
    r"controlled by|funded by|linked to|connected to|associated with|"
    r"common ownership|wallet cluster|relationship)\b",
    re.IGNORECASE,
)


class ValidationError(ValueError):
    def __init__(self, code: str, message: str) -> None:
        super().__init__(message)
        self.code = code


class EvidenceClaim(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True)

    claim: str = Field(min_length=1)
    supporting_evidence_ids: list[str]
    counter_evidence_ids: list[str]
    uncertainty: str


class AdvisoryResponse(BaseModel):
    """The only model-authored fields accepted by the investigator."""

    model_config = ConfigDict(extra="forbid", strict=True)

    summary: str = Field(min_length=1)
    bull_case: list[str]
    bear_case: list[str]
    coordination_hypotheses: list[EvidenceClaim]
    creator_risks: list[str]
    wallet_risks: list[str]
    contradictory_evidence: list[str]
    missing_information: list[str]
    confidence: float = Field(ge=0.0, le=1.0)
    # Kept because the existing investigator contract requires claim/evidence citations.
    claims: list[EvidenceClaim]


def load_schema() -> dict[str, Any]:
    """Return the OpenAI-compatible strict response schema."""
    schema = AdvisoryResponse.model_json_schema()
    schema["additionalProperties"] = False
    return schema


def _machine_fields(response: dict[str, Any]) -> list[str]:
    fields = [str(response.get("summary") or "")]
    for claim in response.get("claims") or []:
        fields.append(str(claim.get("claim") or ""))
        fields.append(str(claim.get("uncertainty") or ""))
    for claim in response.get("coordination_hypotheses") or []:
        fields.append(str(claim.get("claim") or ""))
        fields.append(str(claim.get("uncertainty") or ""))
    for key in (
        "bull_case",
        "bear_case",
        "creator_risks",
        "wallet_risks",
        "contradictory_evidence",
        "missing_information",
    ):
        fields.extend(str(x) for x in (response.get(key) or []))
    return fields


def validate_response(response: dict[str, Any], known_evidence: set[str]) -> dict[str, Any]:
    if not isinstance(response, dict):
        raise ValidationError("not_object", "response must be an object")
    try:
        validated = AdvisoryResponse.model_validate(response)
    except PydanticValidationError as exc:
        raise ValidationError("schema_validation", str(exc)) from exc

    normalized = validated.model_dump(mode="json")

    cited: set[str] = set()
    unverified: list[str] = []
    all_claims = [
        *normalized["claims"],
        *normalized["coordination_hypotheses"],
    ]
    for claim in all_claims:
        for key in ("supporting_evidence_ids", "counter_evidence_ids"):
            ids = claim[key]
            for eid in ids:
                if eid not in known_evidence:
                    raise ValidationError("unknown_evidence", f"unknown evidence id {eid}")
                cited.add(eid)
        text = claim["claim"]
        for match in PROB_RE.finditer(text):
            value = float(match.group(1))
            if value < 0 or value > 1:
                raise ValidationError("unsupported_number", f"probability {value} outside [0,1]")
        if not claim["supporting_evidence_ids"]:
            unverified.append(text)
            if RELATIONSHIP_RE.search(text):
                raise ValidationError(
                    "unsupported_relationship",
                    "relationship claims require supporting evidence ids",
                )

    for hypothesis in normalized["coordination_hypotheses"]:
        if not hypothesis["supporting_evidence_ids"]:
            raise ValidationError(
                "unsupported_relationship",
                "coordination hypotheses require supporting evidence ids",
            )

    for field in _machine_fields(normalized):
        if LIVE_TRADE.search(field):
            raise ValidationError("live_trade_language", "live-trade language is forbidden in machine fields")

    return {
        "ok": True,
        "cited_evidence_ids": sorted(cited),
        "unverified_claims": unverified,
        "affects_policy": False,
    }


def policy_effect(_response: dict[str, Any] | None) -> bool:
    """Investigator output never mutates paper policy state."""
    return False
