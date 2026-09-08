"""Point-in-time reputation. Skill and suspicion stay on separate axes."""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime

from pumphound_ml.io import content_id


def posterior_rate(successes: int, failures: int, alpha: float, beta: float) -> float:
    return (successes + alpha) / (successes + failures + alpha + beta)


@dataclass
class ReputationSnapshot:
    subject_type: str
    subject_id: str
    as_of_slot: int
    knowledge_cutoff: datetime
    version: str
    sample_count: int
    skill_posterior: float
    skill_alpha: float
    skill_beta: float
    suspicion_posterior: float
    suspicion_alpha: float
    suspicion_beta: float
    graduation_posterior: float | None = None
    collapse_posterior: float | None = None
    coverage: dict = field(default_factory=dict)
    calibrated: bool = False

    @property
    def snapshot_id(self) -> str:
        return content_id("rep", self.subject_type, self.subject_id, self.as_of_slot, self.version)


def shrink_rate(
    successes: int,
    trials: int,
    *,
    alpha: float = 1.0,
    beta: float = 3.0,
) -> tuple[float, float, float, int]:
    failures = max(0, trials - successes)
    return posterior_rate(successes, failures, alpha, beta), alpha + successes, beta + failures, trials


def wallet_reputation(
    *,
    wallet: str,
    as_of_slot: int,
    knowledge_cutoff: datetime,
    good_exits: int,
    bad_exits: int,
    coordinated_flags: int,
    independent_flags: int,
    prior_alpha: float = 1.0,
    prior_beta: float = 3.0,
    coverage: dict | None = None,
) -> ReputationSnapshot:
    skill, sa, sb, n = shrink_rate(good_exits, good_exits + bad_exits, alpha=prior_alpha, beta=prior_beta)
    suspicion, ua, ub, _ = shrink_rate(
        coordinated_flags,
        coordinated_flags + independent_flags,
        alpha=prior_alpha,
        beta=prior_beta,
    )
    return ReputationSnapshot(
        subject_type="wallet",
        subject_id=wallet,
        as_of_slot=as_of_slot,
        knowledge_cutoff=knowledge_cutoff,
        version="reputation-v1",
        sample_count=n,
        skill_posterior=skill,
        skill_alpha=sa,
        skill_beta=sb,
        suspicion_posterior=suspicion,
        suspicion_alpha=ua,
        suspicion_beta=ub,
        coverage=coverage or {"unknown": n == 0},
    )


def creator_reputation(
    *,
    creator: str,
    as_of_slot: int,
    knowledge_cutoff: datetime,
    graduated: int,
    matured: int,
    collapses: int,
    prior_alpha: float = 1.0,
    prior_beta: float = 3.0,
) -> ReputationSnapshot:
    grad, ga, gb, n = shrink_rate(graduated, matured, alpha=prior_alpha, beta=prior_beta)
    col, ca, cb, _ = shrink_rate(collapses, matured, alpha=prior_alpha, beta=prior_beta)
    return ReputationSnapshot(
        subject_type="creator",
        subject_id=creator,
        as_of_slot=as_of_slot,
        knowledge_cutoff=knowledge_cutoff,
        version="reputation-v1",
        sample_count=n,
        skill_posterior=grad,
        skill_alpha=ga,
        skill_beta=gb,
        suspicion_posterior=col,
        suspicion_alpha=ca,
        suspicion_beta=cb,
        graduation_posterior=grad,
        collapse_posterior=col,
        coverage={"matured_launches": matured, "unknown": matured == 0},
    )
