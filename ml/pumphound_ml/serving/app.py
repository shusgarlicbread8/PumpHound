"""Internal FastAPI scorer used by the ml-worker process."""

from __future__ import annotations

from typing import Any, Literal

from fastapi import FastAPI, HTTPException
from pydantic import BaseModel

from pumphound_ml.registry.store import ModelRegistry
from pumphound_ml.serving.predict import predict_bundle, predict_champions
from pumphound_ml.types import (
    DataQuality,
    FeatureVector,
    ObservationPoint,
    QualityGrade,
    to_plain,
)

registry = ModelRegistry()


class PredictRequest(BaseModel):
    model_version: str | None = None
    authority: Literal["control", "shadow"] = "control"
    feature_vector: dict[str, Any]
    timeout_s: float = 2.0


def vector_from_payload(payload: dict[str, Any]) -> FeatureVector:
    as_of = ObservationPoint.from_mapping(payload.get("as_of") or {})
    q = payload.get("quality") or {}
    grade = q.get("grade") or "C"
    quality = DataQuality(
        grade=QualityGrade(grade)
        if grade in {g.value for g in QualityGrade}
        else QualityGrade.C,
        score=float(q.get("score") or 0.0),
        missing_fields=list(q.get("missing_fields") or []),
        reasons=list(q.get("reasons") or []),
        ordering=str(q.get("ordering") or "unknown"),
        coverage=str(q.get("coverage") or "unknown"),
    )
    return FeatureVector(
        feature_snapshot_id=str(payload.get("feature_snapshot_id") or ""),
        launch_id=str(payload.get("launch_id") or ""),
        observation_window_seconds=int(payload.get("observation_window_seconds") or 60),
        as_of=as_of,
        schema_version=str(payload.get("schema_version") or "features-v1"),
        values=dict(payload.get("values") or {}),
        dependency_ids=list(payload.get("dependency_ids") or []),
        quality=quality,
    )


def create_app() -> FastAPI:
    app = FastAPI(title="PumpHound ML serving", version="0.1.0")

    @app.post("/internal/predict")
    def predict(req: PredictRequest) -> dict[str, Any]:
        vector = vector_from_payload(req.feature_vector)
        if not vector.launch_id or not vector.feature_snapshot_id:
            raise HTTPException(
                status_code=422, detail="launch_id and feature_snapshot_id required"
            )
        if req.model_version is None:
            return predict_champions(
                vector,
                registry=registry,
                timeout_s=req.timeout_s,
                authority=req.authority,
            )
        bundle = predict_bundle(
            vector,
            model_version=req.model_version,
            registry=registry,
            timeout_s=req.timeout_s,
        )
        return to_plain(bundle)

    @app.post("/predict")
    def predict_all(req: PredictRequest) -> dict[str, Any]:
        vector = vector_from_payload(req.feature_vector)
        if not vector.launch_id or not vector.feature_snapshot_id:
            raise HTTPException(
                status_code=422, detail="launch_id and feature_snapshot_id required"
            )
        return predict_champions(
            vector,
            registry=registry,
            timeout_s=req.timeout_s,
            authority=req.authority,
        )

    @app.post("/predict/shadow")
    def predict_shadow(req: PredictRequest) -> dict[str, Any]:
        vector = vector_from_payload(req.feature_vector)
        if not vector.launch_id or not vector.feature_snapshot_id:
            raise HTTPException(
                status_code=422, detail="launch_id and feature_snapshot_id required"
            )
        return predict_champions(
            vector,
            registry=registry,
            timeout_s=req.timeout_s,
            authority="shadow",
        )

    @app.get("/internal/models/{version}/schema")
    def schema(version: str) -> dict[str, Any]:
        try:
            return registry.schema(version)
        except KeyError as exc:
            raise HTTPException(
                status_code=404, detail="unknown_model_version"
            ) from exc

    @app.get("/internal/health")
    def health() -> dict[str, Any]:
        return {
            "status": "ok",
            "models": registry.list_versions(),
            "champions": registry.champions(),
            "shadows": registry.active_shadows(),
        }

    return app


app = create_app()
