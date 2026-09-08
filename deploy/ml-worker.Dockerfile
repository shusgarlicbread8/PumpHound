# syntax=docker/dockerfile:1
# Research-plane worker. Predict abstains until a real model is registered.
# Bind 0.0.0.0 inside the private overlay only; do not publish 8091 publicly.
FROM python:3.12-slim-bookworm
WORKDIR /app
RUN apt-get update \
    && apt-get install -y --no-install-recommends libgomp1 \
    && rm -rf /var/lib/apt/lists/*
COPY pyproject.toml /app/pyproject.toml
COPY ml /app/ml
COPY services/ml-worker /app/services/ml-worker
COPY fixtures /app/fixtures
COPY schemas /app/schemas
RUN pip install --no-cache-dir \
      fastapi "uvicorn[standard]" pydantic httpx pyyaml \
      polars numpy scikit-learn orjson pyarrow optuna \
    && mkdir -p /app/artifacts \
    && chmod 777 /app/artifacts
ENV PYTHONUNBUFFERED=1
ENV PYTHONPATH=/app/ml:/app/services
ENV PUMPHOUND_ML_BIND=0.0.0.0
EXPOSE 8091
USER nobody
CMD ["python", "/app/services/ml-worker/main.py"]
