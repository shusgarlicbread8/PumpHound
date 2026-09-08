"""PumpHound ML worker. Bind 127.0.0.1:8091. Never reuse another token's score."""

from __future__ import annotations

import logging
import os
import threading
import time
import uuid
from datetime import UTC, datetime
from typing import Any, Literal

import uvicorn
from fastapi import FastAPI, HTTPException
from pumphound_ml.extraction.dataset import build_dataset
from pumphound_ml.graph.refresh import refresh_graph
from pumphound_ml.labels.short_horizon import append_short_horizon_labels
from pumphound_ml.lifecycle.jobs import DurableJobStore
from pumphound_ml.lifecycle.maturation import PostgresLabelMaturer
from pumphound_ml.lifecycle.retraining import run_retraining
from pumphound_ml.lifecycle.snapshots import DatasetSnapshot, register_snapshot
from pumphound_ml.registry.store import ModelRegistry
from pumphound_ml.replay.runner import run_replay
from pumphound_ml.serving.app import vector_from_payload
from pumphound_ml.serving.predict import predict_bundle, predict_champions
from pumphound_ml.training.tune import run_training
from pumphound_ml.types import JobRecord, to_plain
from pydantic import BaseModel

app = FastAPI(title="pumphound-ml-worker", version="0.1.0")
logger = logging.getLogger("pumphound_ml_worker")
registry = ModelRegistry(
    os.environ.get("PUMPHOUND_MODEL_REGISTRY_PATH", "artifacts/models/registry.json")
)
JOBS: dict[str, JobRecord] = {}
JOB_LOCK = threading.Lock()
DURABLE_JOB_KINDS = frozenset(
    {"label_maturation", "snapshot", "retrain", "adaptive_retrain"}
)


class PredictIn(BaseModel):
    model_version: str | None = None
    authority: Literal["control", "shadow"] = "control"
    feature_vector: dict[str, Any]
    timeout_s: float = 2.0


class JobIn(BaseModel):
    kind: str
    manifest: dict[str, Any] | None = None
    config_path: str | None = None
    manifest_path: str | None = None
    dataset_config_path: str | None = None
    train_config_path: str | None = None
    minimum_new_labels: int | None = None
    trigger: str | None = None


def _postgres_dsn() -> str:
    dsn = os.environ.get("PUMPHOUND_POSTGRES_DSN", "")
    if not dsn:
        raise ValueError("PUMPHOUND_POSTGRES_DSN_required")
    return dsn


def _execute_job(
    kind: str, req: dict[str, Any], *, job_id: str | None = None
) -> dict[str, Any]:
    if kind == "dataset":
        return build_dataset(req.get("config_path") or "ml/configs/dataset-v1.yaml")
    if kind == "train":
        return run_training(req.get("config_path") or "ml/configs/train-v1.yaml")
    if kind in {"graph", "graph_refresh"}:
        return refresh_graph(req.get("manifest_path") or "ml/configs/graph-v1.yaml")
    if kind == "replay":
        return run_replay(
            req.get("manifest_path") or "fixtures/replay/manifest.json"
        ).to_dict()
    if kind == "label_maturation":
        return PostgresLabelMaturer(_postgres_dsn()).run()
    if kind == "snapshot":
        result = append_short_horizon_labels(
            req.get("config_path") or "ml/configs/dataset-adaptive-existing.yaml"
        )
        snapshot = DatasetSnapshot(
            dataset_snapshot_id=result["dataset_snapshot_id"],
            content_sha256=result["content_sha256"],
            uri=result["dataset_uri"],
            manifest=result["manifest"],
        )
        register_snapshot(snapshot, _postgres_dsn())
        return result
    if kind in {"retrain", "adaptive_retrain"}:
        manifest = req.get("manifest") or {}
        return run_retraining(
            dataset_config_path=req.get("dataset_config_path")
            or req.get("config_path")
            or manifest.get("dataset_config_path")
            or "ml/configs/dataset-adaptive-existing.yaml",
            train_config_path=req.get("train_config_path")
            or manifest.get("train_config_path")
            or "ml/configs/train-adaptive-short-horizon.yaml",
            dsn=_postgres_dsn(),
            job_id=job_id,
            minimum_new_labels=int(req.get("minimum_new_labels") or 0),
        )
    raise ValueError(f"unknown_job_kind:{kind}")


def _run_job(job_id: str) -> None:
    with JOB_LOCK:
        job = JOBS[job_id]
        job.state = "running"
        job.updated_at = datetime.now(UTC)
    try:
        kind = job.kind
        req = job.request
        result = _execute_job(kind, req, job_id=job_id)
        with JOB_LOCK:
            job.state = "succeeded"
            job.result = result
            job.progress = {"pct": 100}
            job.updated_at = datetime.now(UTC)
    except Exception as exc:  # noqa: BLE001
        with JOB_LOCK:
            job.state = "failed"
            job.error_code = str(exc)
            job.updated_at = datetime.now(UTC)


def _run_durable_job(job_id: str) -> None:
    store = DurableJobStore(_postgres_dsn())
    claimed = store.claim(job_id)
    if claimed is None:
        return
    kind, request = claimed
    try:
        store.succeed(
            job_id,
            _execute_job(kind, request, job_id=job_id),
        )
    except Exception as exc:  # noqa: BLE001
        store.fail(job_id, str(exc))


def _poll_durable_jobs() -> None:
    store = DurableJobStore(_postgres_dsn())
    while True:
        try:
            claimed = store.claim_next(DURABLE_JOB_KINDS)
            if claimed is None:
                time.sleep(2.0)
                continue
            job_id, kind, request = claimed
            try:
                result = _execute_job(kind, request, job_id=job_id)
                store.succeed(job_id, result)
            except Exception as exc:  # noqa: BLE001
                store.fail(job_id, str(exc))
        except Exception:  # noqa: BLE001
            time.sleep(5.0)


def _schedule_continuous_maturation() -> None:
    store = DurableJobStore(_postgres_dsn())
    while True:
        minute = datetime.now(UTC).replace(second=0, microsecond=0).isoformat()
        try:
            store.enqueue(
                "label_maturation",
                {"schedule": "continuous-v1", "minute": minute},
            )
        except Exception:
            logger.exception("continuous label maturation enqueue failed")
        time.sleep(30.0)


def _schedule_continuous_retraining() -> None:
    dsn = _postgres_dsn()
    store = DurableJobStore(dsn)
    while True:
        try:
            import psycopg

            with psycopg.connect(dsn) as connection, connection.cursor() as cursor:
                cursor.execute(
                    "SELECT COUNT(*) FROM pumphound.outcome_labels WHERE status = 'MATURED'"
                )
                matured_count = int(cursor.fetchone()[0])
                cursor.execute(
                    "SELECT COALESCE(MAX((manifest->>'live_label_count')::bigint), 0) FROM pumphound.dataset_snapshots"
                )
                latest_live_labels = int(cursor.fetchone()[0])
                cursor.execute(
                    "SELECT COUNT(*) FROM pumphound.background_jobs WHERE state IN ('queued', 'running') AND kind IN ('retrain', 'adaptive_retrain')"
                )
                active_retrain_jobs = int(cursor.fetchone()[0])

            new_labels = matured_count - latest_live_labels
            if new_labels >= 50 and active_retrain_jobs == 0:
                logger.info(
                    "triggering adaptive retraining: %d new live labels",
                    new_labels,
                )
                store.enqueue(
                    "adaptive_retrain",
                    {
                        "schedule": "continuous-adaptive-v1",
                        "minimum_new_labels": 50,
                        "matured_count": matured_count,
                        "latest_live_labels": latest_live_labels,
                    },
                )
        except Exception:
            logger.exception("continuous retraining schedule check failed")
        time.sleep(60.0)


@app.post("/internal/predict")
def predict(body: PredictIn) -> dict[str, Any]:
    vector = vector_from_payload(body.feature_vector)
    if not vector.launch_id or not vector.feature_snapshot_id:
        raise HTTPException(
            status_code=422, detail="launch_id and feature_snapshot_id required"
        )
    if body.model_version is None:
        return predict_champions(
            vector,
            registry=registry,
            timeout_s=max(0.0, body.timeout_s),
            authority=body.authority,
        )
    if body.timeout_s <= 0:
        bundle = predict_bundle(
            vector, model_version=body.model_version, registry=registry, timeout_s=0.0
        )
        if not bundle.abstention_reason:
            bundle.abstention_reason = "timeout"
            bundle.outputs = {}
        return to_plain(bundle)
    bundle = predict_bundle(
        vector,
        model_version=body.model_version,
        registry=registry,
        timeout_s=body.timeout_s,
    )
    # Refuse to return a bundle that belongs to a different token/window.
    if bundle.launch_id != vector.launch_id:
        raise HTTPException(
            status_code=409, detail="stale_or_other_token_score_refused"
        )
    if bundle.observation_window_seconds != vector.observation_window_seconds:
        raise HTTPException(status_code=409, detail="incompatible_window_score_refused")
    return to_plain(bundle)


@app.post("/predict")
def predict_all(body: PredictIn) -> dict[str, Any]:
    vector = vector_from_payload(body.feature_vector)
    if not vector.launch_id or not vector.feature_snapshot_id:
        raise HTTPException(
            status_code=422, detail="launch_id and feature_snapshot_id required"
        )
    return predict_champions(
        vector,
        registry=registry,
        timeout_s=max(0.0, body.timeout_s),
        authority=body.authority,
    )


@app.post("/predict/shadow")
def predict_shadow(body: PredictIn) -> dict[str, Any]:
    vector = vector_from_payload(body.feature_vector)
    if not vector.launch_id or not vector.feature_snapshot_id:
        raise HTTPException(
            status_code=422, detail="launch_id and feature_snapshot_id required"
        )
    return predict_champions(
        vector,
        registry=registry,
        timeout_s=max(0.0, body.timeout_s),
        authority="shadow",
    )


@app.post("/internal/jobs")
def enqueue(body: JobIn) -> dict[str, Any]:
    if body.kind in DURABLE_JOB_KINDS:
        try:
            store = DurableJobStore(_postgres_dsn())
            queued = store.enqueue(body.kind, body.model_dump())
        except ValueError as exc:
            raise HTTPException(status_code=503, detail=str(exc)) from exc
        if not queued["idempotent"] and queued["state"] == "queued":
            thread = threading.Thread(
                target=_run_durable_job,
                args=(queued["job_id"],),
                daemon=True,
            )
            thread.start()
        return queued
    job_id = str(uuid.uuid4())
    rec = JobRecord(
        job_id=job_id,
        kind=body.kind,
        state="queued",
        request=body.model_dump(),
    )
    with JOB_LOCK:
        JOBS[job_id] = rec
    thread = threading.Thread(target=_run_job, args=(job_id,), daemon=True)
    thread.start()
    return {"job_id": job_id, "state": rec.state}


@app.get("/internal/jobs/{job_id}")
def job_status(job_id: str) -> dict[str, Any]:
    if os.environ.get("PUMPHOUND_POSTGRES_DSN"):
        try:
            durable = DurableJobStore(_postgres_dsn()).get(job_id)
        except (ValueError, TypeError):
            durable = None
        if durable is not None:
            return durable
    with JOB_LOCK:
        job = JOBS.get(job_id)
    if job is None:
        raise HTTPException(status_code=404, detail="unknown_job")
    return to_plain(job)


@app.get("/internal/models/{version}/schema")
def model_schema(version: str) -> dict[str, Any]:
    try:
        return registry.schema(version)
    except KeyError as exc:
        raise HTTPException(status_code=404, detail="unknown_model_version") from exc


@app.get("/internal/health")
def health() -> dict[str, Any]:
    with JOB_LOCK:
        queued = sum(1 for j in JOBS.values() if j.state in {"queued", "running"})
    return {
        "status": "ok",
        "bind": "127.0.0.1:8091",
        "models": registry.list_versions(),
        "champions": registry.champions(),
        "shadows": registry.active_shadows(),
        "queue_depth": queued,
    }


def main() -> None:
    host = os.environ.get("PUMPHOUND_ML_BIND", "127.0.0.1")
    if os.environ.get("PUMPHOUND_POSTGRES_DSN"):
        threading.Thread(target=_poll_durable_jobs, daemon=True).start()
        threading.Thread(target=_schedule_continuous_maturation, daemon=True).start()
        threading.Thread(target=_schedule_continuous_retraining, daemon=True).start()
    uvicorn.run(app, host=host, port=8091, log_level="info")


if __name__ == "__main__":
    main()
