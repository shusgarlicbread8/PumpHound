"""Explainable intent hypotheses. Until calibrated, emit evidence_score only."""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime

from pumphound_ml.graph.cooccurrence import PairComponents
from pumphound_ml.graph.funding import FundingPathResult
from pumphound_ml.graph.reputation import ReputationSnapshot
from pumphound_ml.io import content_id

HYPOTHESES = (
    "organic_early_buyer",
    "coordinated_sniper_cohort",
    "creator_controlled_demand",
    "distributor_operator",
    "momentum_copy_trader",
    "accumulator",
    "exit_preparation",
    "arbitrage_mev",
)


@dataclass
class IntentHypothesis:
    subject_type: str
    subject_id: str
    hypothesis: str
    evidence_score: float
    calibrated: bool
    probability: float | None
    as_of_slot: int
    model_version: str
    supporting_evidence_ids: list[str]
    counter_evidence_ids: list[str]
    data_gaps: list[str]
    confidence_band: list[float] | None = None

    def to_record(self) -> dict:
        payload = {
            "subject_type": self.subject_type,
            "subject_id": self.subject_id,
            "hypothesis": self.hypothesis,
            "evidence_score": self.evidence_score,
            "calibrated": False,
            "probability": None,
            "as_of_slot": self.as_of_slot,
            "model_version": self.model_version,
            "supporting_evidence_ids": self.supporting_evidence_ids,
            "counter_evidence_ids": self.counter_evidence_ids,
            "data_gaps": self.data_gaps,
        }
        if self.calibrated and self.probability is not None:
            payload["calibrated"] = True
            payload["probability"] = self.probability
            payload["confidence_band"] = self.confidence_band
        return payload


def infer_intent(
    *,
    subject_type: str,
    subject_id: str,
    as_of_slot: int,
    knowledge_cutoff: datetime,
    pairs: list[PairComponents] | None = None,
    funding: FundingPathResult | None = None,
    reputation: ReputationSnapshot | None = None,
    creator_linked: bool = False,
    history_complete: bool = True,
) -> list[IntentHypothesis]:
    del knowledge_cutoff  # recorded by caller; not used to invent later evidence
    durable = [p for p in (pairs or []) if p.durable and subject_id in {p.wallet_a, p.wallet_b}]
    max_pair = max((p.score for p in durable), default=0.0)
    shared = max((p.shared_launches for p in durable), default=0)
    hub_only = bool(funding and funding.stopped_at_hubs and not funding.competing_funders)
    nonhub = bool(funding and any(not (p.hops and p.hops[0].hub) and p.strength > 0.3 for p in funding.paths))
    suspicion = reputation.suspicion_posterior if reputation else None
    skill = reputation.skill_posterior if reputation else None
    gaps: list[str] = []
    if funding and funding.unknown:
        gaps.append("funding history incomplete or stops at unknown/hub source")
    if not history_complete:
        gaps.append("wallet history coverage incomplete")
    if suspicion is None:
        gaps.append("reputation posterior unavailable")

    scored: list[tuple[str, float, list[str], list[str]]] = []
    evidence = [eid for p in durable for eid in p.evidence_ids]
    if funding:
        evidence.extend(funding.evidence_ids)

    scored.append(
        (
            "coordinated_sniper_cohort",
            min(1.0, 0.5 * max_pair + 0.2 * min(1.0, shared / 3) + 0.3 * (1.0 if nonhub else 0.0)),
            evidence,
            ["edge:public-signal-unobserved"] if max_pair > 0 else [],
        )
    )
    scored.append(
        (
            "creator_controlled_demand",
            0.7 if creator_linked and nonhub else (0.2 if creator_linked else 0.0),
            evidence if creator_linked else [],
            ["path:hub-only"] if hub_only else [],
        )
    )
    scored.append(
        (
            "organic_early_buyer",
            0.65 if (not durable and not creator_linked and not nonhub) else 0.15,
            [],
            evidence,
        )
    )
    scored.append(
        (
            "distributor_operator",
            0.55 if (funding and len(funding.competing_funders) == 1 and not hub_only) else 0.05,
            funding.evidence_ids if funding else [],
            [],
        )
    )
    scored.append(("momentum_copy_trader", 0.2, [], []))
    scored.append(("accumulator", 0.15 if skill and skill > 0.6 else 0.05, [], []))
    scored.append(("exit_preparation", 0.1, [], []))
    scored.append(("arbitrage_mev", 0.05, [], []))

    out: list[IntentHypothesis] = []
    for name, score, support, counter in scored:
        out.append(
            IntentHypothesis(
                subject_type=subject_type,
                subject_id=subject_id,
                hypothesis=name,
                evidence_score=round(max(0.0, min(1.0, score)), 4),
                calibrated=False,
                probability=None,
                as_of_slot=as_of_slot,
                model_version="intent-heuristic-v1",
                supporting_evidence_ids=sorted(set(support)),
                counter_evidence_ids=sorted(set(counter)),
                data_gaps=gaps,
            )
        )
    out.sort(key=lambda h: -h.evidence_score)
    return out
