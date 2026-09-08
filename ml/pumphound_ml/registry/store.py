"""Hash-validated per-target model registry. Promotion is explicit and gated."""

from __future__ import annotations

import argparse
import json
import os
import sys
import uuid
from collections.abc import Callable
from copy import deepcopy
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from pumphound_ml.features.extract import ordered_feature_names
from pumphound_ml.io import dump_json, load_json, load_yaml, sha256_canonical
from pumphound_ml.training.artifacts import validate_artifacts

DEFAULT_PATH = "artifacts/models/registry.json"
LIFECYCLE_STATUSES = frozenset(
    {"candidate", "challenger", "shadow", "champion", "retired", "rejected"}
)
SYNC_STATUSES = LIFECYCLE_STATUSES
UPSERT_SQL = """
INSERT INTO pumphound.model_registry (
    model_version,
    target_version,
    feature_schema_version,
    artifact_uri,
    artifact_sha256,
    status,
    manifest,
    metrics,
    created_at,
    promoted_at
) VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s, %s)
ON CONFLICT (model_version) DO UPDATE SET
    status = EXCLUDED.status,
    promoted_at = EXCLUDED.promoted_at
WHERE model_registry.target_version = EXCLUDED.target_version
  AND model_registry.feature_schema_version = EXCLUDED.feature_schema_version
  AND model_registry.artifact_uri = EXCLUDED.artifact_uri
  AND model_registry.artifact_sha256 = EXCLUDED.artifact_sha256
  AND model_registry.manifest = EXCLUDED.manifest
  AND model_registry.metrics = EXCLUDED.metrics
  AND model_registry.created_at = EXCLUDED.created_at
"""
INSERT_TRANSITION_SQL = """
INSERT INTO pumphound.model_transition_events (
    model_transition_id,
    idempotency_key,
    model_version,
    target_version,
    from_status,
    to_status,
    actor,
    reason,
    gate_version,
    gate_evidence,
    paper_only,
    occurred_at
) VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s, %s, TRUE, %s)
ON CONFLICT (idempotency_key) DO NOTHING
"""


class ModelRegistry:
    def __init__(self, path: str = DEFAULT_PATH, *, strict: bool = False) -> None:
        self.path = Path(path)
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self._data: dict[str, Any] = {
            "models": {},
            "champions": {},
            "shadows": {},
            "transitions": [],
        }
        if strict and not self.path.is_file():
            raise FileNotFoundError(f"registry_not_found:{self.path}")
        if self.path.exists():
            try:
                self._data = load_json(self.path)
                if not isinstance(self._data, dict):
                    raise TypeError("registry root must be a mapping")
            except (OSError, TypeError, ValueError) as exc:
                if strict:
                    raise ValueError(f"malformed_registry:{self.path}") from exc
                self._data = {
                    "models": {},
                    "champions": {},
                    "shadows": {},
                    "transitions": [],
                }
        self._data.setdefault("models", {})
        self._data.setdefault("champions", {})
        self._data.setdefault("shadows", {})
        self._data.setdefault("transitions", [])
        legacy = self._data.pop("champion", None)
        if legacy and legacy in self._data["models"]:
            target = self._data["models"][legacy].get("target_version")
            if target:
                self._data["champions"].setdefault(target, legacy)
        for target in {
            str(record.get("target_version"))
            for record in self._data["models"].values()
            if record.get("status") == "shadow" and record.get("target_version")
        }:
            if target not in self._data["shadows"]:
                matching = [
                    record
                    for record in self._data["models"].values()
                    if record.get("status") == "shadow"
                    and record.get("target_version") == target
                ]
                latest = max(
                    matching,
                    key=lambda record: (
                        str(record.get("created_at") or ""),
                        str(record.get("model_version") or ""),
                    ),
                )
                self._data["shadows"][target] = latest["model_version"]
        for record in self._data["models"].values():
            target = str(record.get("target_version") or "")
            if record.get("status") == "shadow":
                record.setdefault("experimental_label", "EXPERIMENTAL")
            if record.get("status") == "shadow" and self._data["shadows"].get(
                target
            ) != record.get("model_version"):
                record["status"] = "retired"

    def persist(self) -> None:
        dump_json(self.path, self._data)

    @staticmethod
    def _event_status(status: str) -> str:
        return "CHALLENGER" if status in {"candidate", "challenger"} else status.upper()

    def _append_transition(
        self,
        version: str,
        *,
        from_status: str | None,
        to_status: str,
        actor: str,
        reason: str,
        gate_version: str | None = None,
        gate_evidence: dict[str, Any] | None = None,
    ) -> dict[str, Any]:
        if not actor.strip() or not reason.strip():
            raise ValueError("transition_actor_and_reason_required")
        target = str(self._data["models"][version].get("target_version") or "")
        occurred_at = datetime.now(UTC).isoformat()
        sequence = len(self._data["transitions"])
        key = sha256_canonical(
            [version, from_status, to_status, actor, reason, occurred_at, sequence]
        )
        event = {
            "model_transition_id": str(uuid.uuid5(uuid.NAMESPACE_URL, key)),
            "idempotency_key": key,
            "model_version": version,
            "target_version": target,
            "from_status": self._event_status(from_status) if from_status else None,
            "to_status": self._event_status(to_status),
            "actor": actor,
            "reason": reason,
            "gate_version": gate_version,
            "gate_evidence": deepcopy(gate_evidence or {}),
            "paper_only": True,
            "occurred_at": occurred_at,
        }
        self._data["transitions"].append(event)
        return event

    def register(self, version: str, record: dict[str, Any]) -> dict[str, Any]:
        if record.get("status") == "champion":
            raise ValueError("register_cannot_auto_promote")
        if record.get("status") not in {None, "candidate", "challenger", "shadow"}:
            raise ValueError("register_status_must_be_candidate_challenger_or_shadow")
        if version in self._data.get("models", {}):
            raise ValueError("model_version_already_registered")
        if record.get("status") == "shadow" and not record.get("target_version"):
            raise ValueError("target_version_required_for_shadow")
        rec = {
            "model_version": version,
            "status": record.get("status") or "candidate",
            "target_version": record.get("target_version"),
            "feature_schema_version": record.get("feature_schema_version")
            or "features-v1",
            "feature_names": list(
                record.get("feature_names") or ordered_feature_names()
            ),
            "artifact_uri": record.get("artifact_uri"),
            "artifacts": deepcopy(record.get("artifacts") or {}),
            "calibrated": bool(record.get("calibrated")),
            "calibration": deepcopy(record.get("calibration") or {}),
            "familiarity_threshold": record.get("familiarity_threshold"),
            "policy_threshold": deepcopy(record.get("policy_threshold") or {}),
            "metrics": record.get("metrics") or {},
            "sample_counts": deepcopy(record.get("sample_counts") or {}),
            "leakage_audit": deepcopy(record.get("leakage_audit") or {}),
            "training_manifest_sha256": record.get("training_manifest_sha256"),
            "data_manifest_sha256": record.get("data_manifest_sha256"),
            "dataset_promotion_eligible": bool(
                record.get("dataset_promotion_eligible", True)
            ),
            "champion_gate_failures": list(record.get("champion_gate_failures") or []),
            "experimental_label": (
                record.get("experimental_label")
                or ("EXPERIMENTAL" if record.get("status") == "shadow" else None)
            ),
            "small_sample_warnings": list(record.get("small_sample_warnings") or []),
            "test_split_accessed_during_tuning": bool(
                record.get("test_split_accessed_during_tuning")
            ),
            "created_at": record.get("created_at") or datetime.now(UTC).isoformat(),
            "promoted_at": None,
        }
        self._data.setdefault("models", {})[version] = rec
        self._append_transition(
            version,
            from_status=None,
            to_status=str(rec["status"]),
            actor=str(record.get("transition_actor") or "training"),
            reason=str(record.get("transition_reason") or "model_registered"),
            gate_evidence={"artifact_uri": rec.get("artifact_uri")},
        )
        if rec["status"] == "shadow":
            target = str(rec.get("target_version") or "")
            previous = self._data["shadows"].get(target)
            if previous and previous in self._data["models"]:
                previous_status = str(self._data["models"][previous]["status"])
                self._data["models"][previous]["status"] = "retired"
                self._append_transition(
                    previous,
                    from_status=previous_status,
                    to_status="retired",
                    actor="registry",
                    reason=f"superseded_by_shadow:{version}",
                )
            self._data["shadows"][target] = version
        self.persist()
        return rec

    def get(self, version: str) -> dict[str, Any] | None:
        return deepcopy(self._data.get("models", {}).get(version))

    def schema(self, version: str) -> dict[str, Any]:
        rec = self.get(version)
        if rec is None:
            raise KeyError(version)
        feature_names = list(rec["feature_names"])
        return {
            "model_version": version,
            "feature_schema_version": rec["feature_schema_version"],
            "feature_names": feature_names,
            "required_live_feature_names": feature_names,
            "minimum_feature_contract": {
                "required_identifiers": [
                    "launch_id",
                    "feature_snapshot_id",
                    "observation_window_seconds",
                ],
                "feature_schema_version": rec["feature_schema_version"],
                "required_value_keys": feature_names,
                "value_type": "finite_number_or_null",
                "null_values_allowed": True,
                "additional_value_keys_allowed": True,
                "minimum_quality_grade": "C",
                "unavailable_quality_rejected": True,
            },
            "target_version": rec["target_version"],
            "calibrated": rec["calibrated"],
            "familiarity_threshold": rec.get("familiarity_threshold"),
            "status": rec["status"],
        }

    def champion_for(self, target: str) -> dict[str, Any] | None:
        version = self._data.get("champions", {}).get(target)
        return self.get(version) if version else None

    def champions(self) -> dict[str, str]:
        return deepcopy(self._data.get("champions", {}))

    def shadow_for(self, target: str) -> dict[str, Any] | None:
        version = self._data.get("shadows", {}).get(target)
        record = self.get(version) if version else None
        return record if record and record.get("status") == "shadow" else None

    def active_shadows(self) -> dict[str, str]:
        return deepcopy(self._data.get("shadows", {}))

    def promotion_failures(
        self, record: dict[str, Any], gates: dict[str, Any]
    ) -> list[str]:
        failures: list[str] = []
        counts = record.get("sample_counts") or {}
        matured = int(counts.get("matured") or 0)
        positives = int(counts.get("positives") or 0)
        if matured < int(gates.get("minimum_matured_samples", 1)):
            failures.append("minimum_matured_samples")
        if positives < int(gates.get("minimum_positives", 1)):
            failures.append("minimum_positives")
        held = (record.get("metrics") or {}).get("held_out") or {}
        if not held:
            failures.append("held_out_report_required")
        metric_gates = gates.get("held_out_metrics") or {}
        for metric, rule in metric_gates.items():
            value = held.get(metric)
            if value is None:
                failures.append(f"held_out_metric_missing:{metric}")
                continue
            if "min" in rule and float(value) < float(rule["min"]):
                failures.append(f"held_out_metric_below_gate:{metric}")
            if "max" in rule and float(value) > float(rule["max"]):
                failures.append(f"held_out_metric_above_gate:{metric}")
        if gates.get("require_calibration", True):
            if not record.get("calibrated"):
                failures.append("calibration_required")
            max_ece = gates.get("maximum_calibration_ece")
            cal_ece = ((record.get("calibration") or {}).get("metrics") or {}).get(
                "ece"
            )
            if max_ece is not None and (
                cal_ece is None or float(cal_ece) > float(max_ece)
            ):
                failures.append("calibration_ece_gate")
        if not (record.get("leakage_audit") or {}).get("passed"):
            failures.append("leakage_audit_required")
        artifacts_ok, artifact_reason = validate_artifacts(
            record.get("artifacts") or {}
        )
        if not artifacts_ok:
            failures.append(artifact_reason or "artifact_validation_failed")
        if not record.get("training_manifest_sha256"):
            failures.append("training_manifest_hash_required")
        if not record.get("data_manifest_sha256"):
            failures.append("data_manifest_hash_required")
        if not record.get("dataset_promotion_eligible"):
            failures.append("dataset_marked_non_promotable")
        if gates.get("require_policy_threshold", True) and not (
            record.get("policy_threshold") or {}
        ).get("available"):
            failures.append("policy_threshold_required")
        return failures

    def transition(
        self,
        version: str,
        *,
        target_status: str,
        reason: str,
        gates: dict[str, Any] | None = None,
        actor: str = "operator",
        gate_version: str | None = None,
    ) -> dict[str, Any]:
        rec = self.get(version)
        if rec is None:
            raise KeyError(version)
        if not reason.strip():
            raise ValueError("explicit_reason_required")
        if target_status not in LIFECYCLE_STATUSES:
            raise ValueError(f"invalid_model_status:{target_status}")
        current = str(rec.get("status") or "")
        if current == target_status:
            raise ValueError("model_status_unchanged")
        allowed = {
            "candidate": {"challenger", "shadow", "champion", "retired", "rejected"},
            "challenger": {"shadow", "champion", "retired", "rejected"},
            "shadow": {"champion", "retired", "rejected"},
            "champion": {"retired"},
            "retired": set(),
            "rejected": set(),
        }
        if target_status not in allowed.get(current, set()):
            raise ValueError(f"invalid_model_transition:{current}->{target_status}")
        if target_status == "champion":
            if rec.get("experimental_label") == "EXPERIMENTAL":
                raise ValueError("shadow_model_cannot_be_promoted_to_champion")
            failures = self.promotion_failures(rec, gates or {})
            if failures:
                raise ValueError("promotion_gates_failed:" + ",".join(failures))
        if target_status == "shadow":
            target = str(rec.get("target_version") or "")
            if not target:
                raise ValueError("target_version_required_for_shadow")
            previous = self._data["shadows"].get(target)
            if previous and previous != version and previous in self._data["models"]:
                previous_status = str(self._data["models"][previous]["status"])
                self._data["models"][previous]["status"] = "retired"
                self._append_transition(
                    previous,
                    from_status=previous_status,
                    to_status="retired",
                    actor=actor,
                    reason=f"superseded_by_shadow:{version}",
                )
            self._data["shadows"][target] = version
        rec["status"] = target_status
        rec["promoted_at"] = (
            datetime.now(UTC).isoformat()
            if target_status in {"shadow", "champion"}
            else rec.get("promoted_at")
        )
        rec["promotion_reason"] = reason
        self._data["models"][version] = rec
        if target_status == "champion":
            target = str(rec.get("target_version") or "")
            if not target:
                raise ValueError("target_version_required")
            previous = self._data["champions"].get(target)
            if previous and previous != version and previous in self._data["models"]:
                previous_status = str(self._data["models"][previous]["status"])
                self._data["models"][previous]["status"] = "retired"
                self._append_transition(
                    previous,
                    from_status=previous_status,
                    to_status="retired",
                    actor=actor,
                    reason=f"superseded_by_champion:{version}",
                    gate_version=gate_version,
                )
            self._data["champions"][target] = version
        if target_status in {"retired", "rejected"}:
            target = str(rec.get("target_version") or "")
            if self._data["champions"].get(target) == version:
                self._data["champions"].pop(target, None)
            if self._data["shadows"].get(target) == version:
                self._data["shadows"].pop(target, None)
        self._append_transition(
            version,
            from_status=current,
            to_status=target_status,
            actor=actor,
            reason=reason,
            gate_version=gate_version,
            gate_evidence={"gates": gates or {}},
        )
        self.persist()
        return rec

    def promote(
        self,
        version: str,
        *,
        target_status: str,
        reason: str,
        gates: dict[str, Any] | None = None,
    ) -> dict[str, Any]:
        return self.transition(
            version,
            target_status=target_status,
            reason=reason,
            gates=gates,
        )

    def transitions(self, version: str | None = None) -> list[dict[str, Any]]:
        events = self._data.get("transitions", [])
        if version is not None:
            events = [
                event for event in events if event.get("model_version") == version
            ]
        return deepcopy(events)

    def list_versions(self) -> list[str]:
        return sorted(self._data.get("models", {}))


def _sync_manifest(
    record: dict[str, Any], *, artifacts_valid: bool, validation_reason: str | None
) -> dict[str, Any]:
    return {
        "target": record.get("target_version"),
        "feature_schema": record.get("feature_schema_version"),
        "feature_schema_version": record.get("feature_schema_version"),
        "feature_names": deepcopy(record.get("feature_names") or []),
        "artifacts": deepcopy(record.get("artifacts") or {}),
        "artifact_validation": {
            "valid": artifacts_valid,
            "reason": validation_reason,
        },
        "calibrated": bool(record.get("calibrated")),
        "calibration": deepcopy(record.get("calibration") or {}),
        "familiarity_threshold": record.get("familiarity_threshold"),
        "policy_threshold": deepcopy(record.get("policy_threshold") or {}),
        "sample_counts": deepcopy(record.get("sample_counts") or {}),
        "leakage_audit": deepcopy(record.get("leakage_audit") or {}),
        "data_manifest_sha256": record.get("data_manifest_sha256"),
        "training_manifest_sha256": record.get("training_manifest_sha256"),
        "champion_gate_failures": deepcopy(record.get("champion_gate_failures") or []),
        "experimental_label": record.get("experimental_label"),
        "small_sample_warnings": deepcopy(record.get("small_sample_warnings") or []),
        "test_split_accessed_during_tuning": bool(
            record.get("test_split_accessed_during_tuning")
        ),
    }


def _sync_rows(registry: ModelRegistry) -> list[tuple[Any, ...]]:
    models = registry._data.get("models")
    champions = registry._data.get("champions")
    shadows = registry._data.get("shadows")
    if (
        not isinstance(models, dict)
        or not isinstance(champions, dict)
        or not isinstance(shadows, dict)
    ):
        raise TypeError("malformed_registry_collections")

    champion_targets: dict[str, str] = {}
    shadow_targets: dict[str, str] = {}
    rows: list[tuple[Any, ...]] = []
    for version, record in models.items():
        if not isinstance(record, dict):
            raise TypeError(f"malformed_model_record:{version}")
        status = str(record.get("status") or "")
        if status not in SYNC_STATUSES:
            continue
        target = str(record.get("target_version") or "")
        schema = str(record.get("feature_schema_version") or "")
        artifact_uri = str(record.get("artifact_uri") or "")
        created_at = record.get("created_at")
        if not target or not schema or not artifact_uri or not created_at:
            raise ValueError(f"incomplete_model_record:{version}")
        artifacts = record.get("artifacts") or {}
        artifacts_valid, validation_reason = validate_artifacts(artifacts)
        if status == "champion":
            if not artifacts_valid:
                raise ValueError(
                    f"invalid_champion_artifacts:{version}:{validation_reason}"
                )
            if target in champion_targets:
                raise ValueError(f"multiple_champions_for_target:{target}")
            if champions.get(target) != version:
                raise ValueError(f"champion_index_mismatch:{target}:{version}")
            champion_targets[target] = version
        if status == "shadow":
            if target in shadow_targets:
                raise ValueError(f"multiple_active_shadows_for_target:{target}")
            if shadows.get(target) != version:
                raise ValueError(f"shadow_index_mismatch:{target}:{version}")
            shadow_targets[target] = version
        manifest = _sync_manifest(
            record,
            artifacts_valid=artifacts_valid,
            validation_reason=validation_reason,
        )
        rows.append(
            (
                version,
                target,
                schema,
                artifact_uri,
                sha256_canonical(artifacts),
                status,
                manifest,
                deepcopy(record.get("metrics") or {}),
                created_at,
                record.get("promoted_at"),
            )
        )
    for target, version in champions.items():
        if champion_targets.get(str(target)) != version:
            raise ValueError(f"champion_index_missing_record:{target}:{version}")
    for target, version in shadows.items():
        if shadow_targets.get(str(target)) != version:
            raise ValueError(f"shadow_index_missing_record:{target}:{version}")
    return rows


def _sync_transition_rows(registry: ModelRegistry) -> list[tuple[Any, ...]]:
    rows = []
    for event in registry.transitions():
        required = {
            "model_transition_id",
            "idempotency_key",
            "model_version",
            "target_version",
            "to_status",
            "actor",
            "reason",
            "occurred_at",
        }
        if not required <= set(event):
            raise ValueError("malformed_model_transition")
        rows.append(
            (
                event["model_transition_id"],
                event["idempotency_key"],
                event["model_version"],
                event["target_version"],
                event.get("from_status"),
                event["to_status"],
                event["actor"],
                event["reason"],
                event.get("gate_version"),
                deepcopy(event.get("gate_evidence") or {}),
                event["occurred_at"],
            )
        )
    return rows


def sync_postgres(
    registry: ModelRegistry,
    dsn: str,
    *,
    connect: Callable[[str], Any] | None = None,
    json_adapter: Callable[[dict[str, Any]], Any] | None = None,
) -> dict[str, Any]:
    """Transactionally mirror active file records; never mutate registry status."""
    if not dsn:
        raise ValueError("PUMPHOUND_POSTGRES_DSN_required")
    rows = _sync_rows(registry)
    transition_rows = _sync_transition_rows(registry)
    if connect is None:
        import psycopg

        connect = psycopg.connect
    if json_adapter is None:
        from psycopg.types.json import Jsonb

        json_adapter = Jsonb
    with (
        connect(dsn) as connection,
        connection.transaction(),
        connection.cursor() as cursor,
    ):
        for row in rows:
            values = list(row)
            values[6] = json_adapter(values[6])
            values[7] = json_adapter(values[7])
            cursor.execute(UPSERT_SQL, tuple(values))
            if getattr(cursor, "rowcount", 1) == 0:
                raise ValueError(f"immutable_model_conflict:{values[0]}")
        for row in transition_rows:
            values = list(row)
            values[9] = json_adapter(values[9])
            cursor.execute(INSERT_TRANSITION_SQL, tuple(values))
    return {
        "synced": len(rows),
        "transitions_synced": len(transition_rows),
        "champions": registry.champions(),
        "shadows": registry.active_shadows(),
        "registry_path": str(registry.path),
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Explicitly promote a PumpHound model candidate"
    )
    sub = parser.add_subparsers(dest="command", required=True)
    promote = sub.add_parser("promote")
    promote.add_argument("--version", required=True)
    promote.add_argument("--reason", required=True)
    promote.add_argument("--config", default="ml/configs/train-v1.yaml")
    promote.add_argument("--registry", default=DEFAULT_PATH)
    sync = sub.add_parser("sync-postgres")
    sync.add_argument("--registry", default=DEFAULT_PATH)
    args = parser.parse_args(argv)
    try:
        if args.command == "promote":
            cfg = load_yaml(args.config)
            gates = (cfg.get("promotion") or {}).get("gates") or {}
            result = ModelRegistry(args.registry).promote(
                args.version,
                target_status="champion",
                reason=args.reason,
                gates=gates,
            )
        else:
            result = sync_postgres(
                ModelRegistry(args.registry, strict=True),
                os.environ.get("PUMPHOUND_POSTGRES_DSN", ""),
            )
    except (KeyError, OSError, TypeError, ValueError) as exc:
        print(str(exc), file=sys.stderr)
        return 2
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
