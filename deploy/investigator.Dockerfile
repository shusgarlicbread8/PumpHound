# syntax=docker/dockerfile:1
# Optional investigator gateway. Local/SiliconFlow if configured, else
# deterministic summaries. Bind 0.0.0.0 on the private overlay only.
FROM python:3.12-slim-bookworm
WORKDIR /app
COPY services/investigator /app/services/investigator
COPY schemas /app/schemas
RUN pip install --no-cache-dir fastapi "uvicorn[standard]" pydantic httpx pyyaml \
    && mkdir -p /app/artifacts \
    && chmod 777 /app/artifacts
ENV PYTHONUNBUFFERED=1
ENV PYTHONPATH=/app/services
ENV PUMPHOUND_INVESTIGATOR_BIND=0.0.0.0
EXPOSE 8092
USER nobody
CMD ["python", "/app/services/investigator/main.py"]
