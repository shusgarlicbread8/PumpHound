"""Append-only investigator audit journal with secret-free request metadata."""

from __future__ import annotations

import json
import os
import threading
from datetime import UTC, datetime
from pathlib import Path
from typing import Any


class InvestigationJournal:
    def __init__(self, path: str | Path | None = None) -> None:
        configured = path or os.environ.get(
            "PUMPHOUND_INVESTIGATOR_JOURNAL",
            "data/journal/investigator.jsonl",
        )
        self.path = Path(configured)
        self._lock = threading.Lock()

    def append(
        self,
        *,
        request_hash: str,
        provider: str,
        model: str,
        prompt_version: str,
        evidence_ids: list[str],
        response: dict[str, Any],
        validation: dict[str, Any],
        degraded: bool,
        latency_ms: int,
        response_format: str,
    ) -> None:
        entry = {
            "timestamp": datetime.now(UTC).isoformat(),
            "request_hash": request_hash,
            "provider": provider,
            "model": model,
            "prompt_version": prompt_version,
            "evidence_ids": evidence_ids,
            "response": response,
            "validation": validation,
            "degraded": degraded,
            "latency_ms": latency_ms,
            "response_format": response_format,
            "policy_effect": False,
        }
        encoded = json.dumps(entry, sort_keys=True, separators=(",", ":"), default=str)
        with self._lock:
            self.path.parent.mkdir(parents=True, exist_ok=True)
            with self.path.open("a", encoding="utf-8") as handle:
                handle.write(encoded)
                handle.write("\n")
