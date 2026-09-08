"""Durable helpers for migration-001 background jobs."""

from __future__ import annotations

import uuid
from collections.abc import Callable
from datetime import UTC, datetime
from typing import Any

from pumphound_ml.io import sha256_canonical

ENQUEUE_LOCK_SQL = "SELECT pg_advisory_xact_lock(hashtextextended(%s, 0))"
FIND_JOB_SQL = """
SELECT job_id, state
FROM pumphound.background_jobs
WHERE kind = %s AND request_hash = %s
ORDER BY created_at DESC
LIMIT 1
"""
INSERT_JOB_SQL = """
INSERT INTO pumphound.background_jobs (
    job_id, kind, request_hash, request, state, progress, created_at, updated_at
) VALUES (%s, %s, %s, %s, 'queued', %s, %s, %s)
"""
CLAIM_JOB_SQL = """
UPDATE pumphound.background_jobs
SET state = 'running', updated_at = %s
WHERE job_id = %s AND state = 'queued'
RETURNING kind, request
"""
CLAIM_NEXT_SQL = """
WITH next_job AS (
    SELECT job_id
    FROM pumphound.background_jobs
    WHERE state = 'queued'
      AND kind = ANY(%s)
    ORDER BY created_at, job_id
    FOR UPDATE SKIP LOCKED
    LIMIT 1
)
UPDATE pumphound.background_jobs AS job
SET state = 'running', updated_at = %s
FROM next_job
WHERE job.job_id = next_job.job_id
RETURNING job.job_id, job.kind, job.request
"""
SUCCEED_JOB_SQL = """
UPDATE pumphound.background_jobs
SET state = 'succeeded',
    cursor = %s,
    progress = %s,
    error_code = NULL,
    updated_at = %s
WHERE job_id = %s AND state = 'running'
"""
FAIL_JOB_SQL = """
UPDATE pumphound.background_jobs
SET state = 'failed', error_code = %s, updated_at = %s
WHERE job_id = %s AND state = 'running'
"""
GET_JOB_SQL = """
SELECT job_id, kind, state, request, cursor, progress, error_code, created_at, updated_at
FROM pumphound.background_jobs
WHERE job_id = %s
"""


class DurableJobStore:
    def __init__(
        self,
        dsn: str,
        *,
        connect: Callable[[str], Any] | None = None,
        json_adapter: Callable[[dict[str, Any]], Any] | None = None,
    ) -> None:
        if not dsn:
            raise ValueError("PUMPHOUND_POSTGRES_DSN_required")
        if connect is None:
            import psycopg

            connect = psycopg.connect
        if json_adapter is None:
            from psycopg.types.json import Jsonb

            json_adapter = Jsonb
        self.dsn = dsn
        self.connect = connect
        self.json_adapter = json_adapter

    def enqueue(self, kind: str, request: dict[str, Any]) -> dict[str, Any]:
        request_hash = sha256_canonical({"kind": kind, "request": request})
        now = datetime.now(UTC)
        with (
            self.connect(self.dsn) as connection,
            connection.transaction(),
            connection.cursor() as cursor,
        ):
            cursor.execute(ENQUEUE_LOCK_SQL, (request_hash,))
            cursor.execute(FIND_JOB_SQL, (kind, request_hash))
            existing = cursor.fetchone()
            if existing is not None:
                return {
                    "job_id": str(existing[0]),
                    "state": str(existing[1]),
                    "idempotent": True,
                }
            job_id = uuid.uuid4()
            cursor.execute(
                INSERT_JOB_SQL,
                (
                    job_id,
                    kind,
                    request_hash,
                    self.json_adapter(request),
                    self.json_adapter({}),
                    now,
                    now,
                ),
            )
        return {"job_id": str(job_id), "state": "queued", "idempotent": False}

    def claim(self, job_id: str) -> tuple[str, dict[str, Any]] | None:
        with (
            self.connect(self.dsn) as connection,
            connection.transaction(),
            connection.cursor() as cursor,
        ):
            cursor.execute(CLAIM_JOB_SQL, (datetime.now(UTC), uuid.UUID(job_id)))
            row = cursor.fetchone()
        return (str(row[0]), dict(row[1])) if row else None

    def claim_next(
        self, kinds: set[str] | frozenset[str]
    ) -> tuple[str, str, dict[str, Any]] | None:
        with (
            self.connect(self.dsn) as connection,
            connection.transaction(),
            connection.cursor() as cursor,
        ):
            cursor.execute(
                CLAIM_NEXT_SQL,
                (sorted(kinds), datetime.now(UTC)),
            )
            row = cursor.fetchone()
        return (str(row[0]), str(row[1]), dict(row[2])) if row else None

    def succeed(self, job_id: str, result: dict[str, Any]) -> None:
        now = datetime.now(UTC)
        with self.connect(self.dsn) as connection, connection.cursor() as cursor:
            cursor.execute(
                SUCCEED_JOB_SQL,
                (
                    self.json_adapter({"result": result}),
                    self.json_adapter({"pct": 100}),
                    now,
                    uuid.UUID(job_id),
                ),
            )

    def fail(self, job_id: str, error: str) -> None:
        with self.connect(self.dsn) as connection, connection.cursor() as cursor:
            cursor.execute(
                FAIL_JOB_SQL,
                (error[:1000], datetime.now(UTC), uuid.UUID(job_id)),
            )

    def get(self, job_id: str) -> dict[str, Any] | None:
        with self.connect(self.dsn) as connection, connection.cursor() as cursor:
            cursor.execute(GET_JOB_SQL, (uuid.UUID(job_id),))
            row = cursor.fetchone()
        if row is None:
            return None
        result = (row[4] or {}).get("result") if isinstance(row[4], dict) else None
        return {
            "job_id": str(row[0]),
            "kind": row[1],
            "state": row[2],
            "request": row[3],
            "result": result,
            "progress": row[5] or {},
            "error_code": row[6],
            "created_at": row[7],
            "updated_at": row[8],
        }
