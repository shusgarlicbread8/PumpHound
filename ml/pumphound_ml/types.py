"""Canonical research-plane types. Atomic quantities stay integers."""

from __future__ import annotations

from collections.abc import Iterable, Mapping
from dataclasses import asdict, dataclass, field, is_dataclass
from datetime import UTC, datetime
from enum import Enum
from typing import Any

SCHEMA_VERSION = 1
DECODER_VERSION = "pump-idl-2026-09-07"
FEATURE_SCHEMA_VERSION = "features-v1"
CHAIN = "solana-mainnet"
WSOL_MINT = "So11111111111111111111111111111111111111112"
USDC_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
PUMP_PROGRAM_ID = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
OBSERVATION_WINDOWS = (5, 10, 15, 30, 60, 120, 300, 900)
LABEL_HORIZONS = {
    "graduated_by_1h_v1": 3600,
    "graduated_by_6h_v1": 21600,
    "graduated_by_24h_v1": 86400,
    "up_50_before_down_20_15m_v1": 900,
    "hit_+25_before_-15": 900,
    "severe_collapse_1h_v1": 4500,
    "net_return_15m_v1": 900,
    "exit_hazard_60s_v1": 60,
}


class Mode(str, Enum):
    LIVE_WATCH = "LIVE_WATCH"
    LIVE_PAPER = "LIVE_PAPER"
    REPLAY = "REPLAY"
    RESEARCH = "RESEARCH"


class Protocol(str, Enum):
    PUMP_CURVE = "pump_curve"
    PUMP_SWAP = "pump_swap"


class QualityGrade(str, Enum):
    A = "A"
    B = "B"
    C = "C"
    UNAVAILABLE = "UNAVAILABLE"


class Stance(str, Enum):
    WATCH = "WATCH"
    WAIT = "WAIT"
    AVOID = "AVOID"
    PAPER_CANDIDATE = "PAPER_CANDIDATE"


def utc_now() -> datetime:
    return datetime.now(UTC)


def parse_ts(value: datetime | str | None) -> datetime | None:
    if value is None:
        return None
    if isinstance(value, datetime):
        if value.tzinfo is None:
            return value.replace(tzinfo=UTC)
        return value.astimezone(UTC)
    text = str(value).replace("Z", "+00:00")
    dt = datetime.fromisoformat(text)
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=UTC)
    return dt.astimezone(UTC)


def fmt_ts(value: datetime | None) -> str | None:
    if value is None:
        return None
    return value.astimezone(UTC).strftime("%Y-%m-%dT%H:%M:%S.%fZ")


def parse_slot(value: Any) -> int:
    if value is None:
        return 0
    return int(value)


def parse_atomic(value: Any) -> int:
    if value is None:
        return 0
    if isinstance(value, bool):
        raise TypeError("atomic quantity cannot be bool")
    return int(str(value))


def to_plain(obj: Any) -> Any:
    if is_dataclass(obj) and not isinstance(obj, type):
        return {k: to_plain(v) for k, v in asdict(obj).items()}
    if isinstance(obj, Enum):
        return obj.value
    if isinstance(obj, datetime):
        return fmt_ts(obj)
    if isinstance(obj, dict):
        return {str(k): to_plain(v) for k, v in obj.items()}
    if isinstance(obj, (list, tuple)):
        return [to_plain(v) for v in obj]
    return obj


@dataclass
class ObservationPoint:
    slot: int
    observed_at: datetime
    knowledge_cutoff: datetime
    tx_index: int | None = None
    event_index: int | None = None
    event_time: datetime | None = None
    commitment: str = "finalized"

    @classmethod
    def from_mapping(cls, data: Mapping[str, Any] | None) -> ObservationPoint:
        data = data or {}
        observed = parse_ts(data.get("observed_at")) or datetime(1970, 1, 1, tzinfo=UTC)
        cutoff = parse_ts(data.get("knowledge_cutoff")) or observed
        return cls(
            slot=parse_slot(data.get("slot", 0)),
            observed_at=observed,
            knowledge_cutoff=cutoff,
            tx_index=data.get("tx_index"),
            event_index=data.get("event_index"),
            event_time=parse_ts(data.get("event_time")),
            commitment=str(data.get("commitment", "finalized")),
        )


@dataclass
class DataQuality:
    grade: QualityGrade = QualityGrade.C
    score: float = 0.0
    chain_lag_ms: int | None = None
    market_lag_ms: int | None = None
    ordering: str = "unknown"
    coverage: str = "unknown"
    missing_fields: list[str] = field(default_factory=list)
    stale_components: list[str] = field(default_factory=list)
    reasons: list[str] = field(default_factory=list)

    @classmethod
    def unknown(cls, reason: str) -> DataQuality:
        return cls(grade=QualityGrade.UNAVAILABLE, score=0.0, reasons=[reason])

    @classmethod
    def from_lags(cls, chain_lag_ms: int, market_lag_ms: int, exact_order: bool) -> DataQuality:
        delayed = chain_lag_ms > 2000 or market_lag_ms > 1000
        if delayed:
            grade, score = QualityGrade.C, 0.4
        elif exact_order:
            grade, score = QualityGrade.A, 0.95
        else:
            grade, score = QualityGrade.B, 0.7
        return cls(
            grade=grade,
            score=score,
            chain_lag_ms=chain_lag_ms,
            market_lag_ms=market_lag_ms,
            ordering="exact" if exact_order else "provisional",
            coverage="complete_for_scope",
            stale_components=["chain_stream"] if delayed else [],
        )


@dataclass
class AssetAmount:
    mint: str
    atomic: int
    decimals: int

    def as_json(self) -> dict[str, Any]:
        return {"mint": self.mint, "atomic": str(self.atomic), "decimals": self.decimals}


@dataclass
class ChainEvent:
    event_id: str
    payload_kind: str
    payload: dict[str, Any]
    slot: int
    available_from_slot: int
    observed_at: datetime
    computed_at: datetime
    block_time: datetime | None
    signature: str
    protocol: str = Protocol.PUMP_CURVE.value
    program_id: str = PUMP_PROGRAM_ID
    transaction_succeeded: bool = True
    canonicality: str = "canonical"
    commitment: str = "finalized"
    action: str = "UPSERT"
    tx_index: int | None = 0
    event_index: int = 0
    instruction_path: list[int] = field(default_factory=lambda: [0])
    provenance: dict[str, Any] = field(default_factory=dict)
    revision: int = 1

    @property
    def known_at(self) -> datetime:
        return self.observed_at

    @property
    def event_time(self) -> datetime:
        return self.block_time or self.observed_at

    @classmethod
    def from_mapping(cls, data: Mapping[str, Any]) -> ChainEvent:
        payload = dict(data.get("payload") or {})
        kind = str(payload.get("kind") or data.get("kind") or "unknown")
        observed = parse_ts(data.get("observed_at")) or datetime(1970, 1, 1, tzinfo=UTC)
        return cls(
            event_id=str(data.get("event_id") or data.get("id") or ""),
            payload_kind=kind,
            payload=payload,
            slot=parse_slot(data.get("slot", 0)),
            available_from_slot=parse_slot(data.get("available_from_slot", data.get("slot", 0))),
            observed_at=observed,
            computed_at=parse_ts(data.get("computed_at")) or observed,
            block_time=parse_ts(data.get("block_time")),
            signature=str(data.get("signature") or ""),
            protocol=str(data.get("protocol") or Protocol.PUMP_CURVE.value),
            program_id=str(data.get("program_id") or PUMP_PROGRAM_ID),
            transaction_succeeded=bool(data.get("transaction_succeeded", True)),
            canonicality=str(data.get("canonicality") or "canonical"),
            commitment=str(data.get("commitment") or "finalized"),
            action=str(data.get("action") or "UPSERT"),
            tx_index=data.get("tx_index"),
            event_index=int(data.get("event_index") or 0),
            instruction_path=list(data.get("instruction_path") or [0]),
            provenance=dict(data.get("provenance") or {}),
            revision=int(data.get("revision") or 1),
        )

    def sort_key(self) -> tuple[int, int, tuple[int, ...], int]:
        return (self.slot, self.tx_index or 0, tuple(self.instruction_path), self.event_index)


def load_events(rows: Iterable[Mapping[str, Any]]) -> list[ChainEvent]:
    events = [ChainEvent.from_mapping(row) for row in rows]
    events.sort(key=lambda e: e.sort_key())
    return events


@dataclass
class GraphNode:
    node_id: str
    node_type: str
    public_key: str | None
    first_seen_slot: int
    known_at: datetime
    attributes: dict[str, Any] = field(default_factory=dict)


@dataclass
class GraphFact:
    fact_id: str
    source_node: str
    target_node: str
    edge_type: str
    event_id: str
    slot: int
    known_at: datetime
    canonicality: str = "canonical"
    asset_mint: str | None = None
    amount_atomic: int | None = None
    attributes: dict[str, Any] = field(default_factory=dict)
    superseded_at: datetime | None = None


@dataclass
class GraphFactBatch:
    algorithm_version: str
    knowledge_cutoff: datetime
    cutoff_slot: int
    nodes: list[GraphNode]
    facts: list[GraphFact]
    coverage: dict[str, Any] = field(default_factory=dict)
    rejected_future_count: int = 0


@dataclass
class FundingHop:
    from_wallet: str
    to_wallet: str
    amount_atomic: int
    mint: str
    slot: int
    event_id: str
    seconds_before_first_buy: float | None
    hub: bool = False


@dataclass
class FundingPath:
    hops: list[FundingHop]
    strength: float
    components: dict[str, float]
    truncated: bool
    truncation_reasons: list[str]


@dataclass
class FundingPathResult:
    wallet: str
    cutoff_slot: int
    knowledge_cutoff: datetime
    paths: list[FundingPath]
    competing_funders: list[str]
    stopped_at_hubs: list[str]
    unknown: bool
    evidence_ids: list[str]
    budget: dict[str, int]


@dataclass
class FeatureVector:
    feature_snapshot_id: str
    launch_id: str
    observation_window_seconds: int
    as_of: ObservationPoint
    schema_version: str
    values: dict[str, float | None]
    dependency_ids: list[str]
    quality: DataQuality
    missingness: dict[str, bool] = field(default_factory=dict)

    def ordered_values(self, names: list[str]) -> list[float | None]:
        return [self.values.get(name) for name in names]


@dataclass
class PredictionBundle:
    prediction_id: str
    feature_snapshot_id: str
    model_version: str
    calibrated: bool
    computed_at: datetime
    outputs: dict[str, dict[str, float]]
    explanation_id: str | None = None
    abstention_reason: str | None = None
    launch_id: str | None = None
    observation_window_seconds: int | None = None


@dataclass
class LabelRecord:
    launch_id: str
    cutoff_slot: int
    observation_window_seconds: int
    label: str
    value: int | float | None
    status: str
    horizon_seconds: int
    matured_at: datetime | None
    evidence_ids: list[str] = field(default_factory=list)
    attributes: dict[str, Any] = field(default_factory=dict)


@dataclass
class ReplayReport:
    manifest_sha256: str
    deterministic_result_sha256: str
    decision_count: int
    fill_count: int
    ledger_balanced: bool
    reservation_violations: int
    lot_violations: int
    future_dependency_count: int
    summary_uri: str
    run_id: str | None = None
    policy_version: str | None = None

    def to_dict(self) -> dict[str, Any]:
        return to_plain(self)


@dataclass
class JobRecord:
    job_id: str
    kind: str
    state: str
    request: dict[str, Any]
    progress: dict[str, Any] = field(default_factory=dict)
    result: dict[str, Any] | None = None
    error_code: str | None = None
    created_at: datetime = field(default_factory=utc_now)
    updated_at: datetime = field(default_factory=utc_now)
