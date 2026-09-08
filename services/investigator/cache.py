"""Cache by evidence-pack hash + prompt version + model ID."""

from __future__ import annotations

import hashlib
import json
import threading
from typing import Any


class InvestigationCache:
    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._store: dict[str, dict[str, Any]] = {}

    @staticmethod
    def key(evidence_pack_sha256: str, prompt_version: str, model_id: str, role: str) -> str:
        raw = json.dumps(
            {
                "evidence": evidence_pack_sha256,
                "prompt": prompt_version,
                "model": model_id,
                "role": role,
            },
            sort_keys=True,
        )
        return hashlib.sha256(raw.encode()).hexdigest()

    def get(self, key: str) -> dict[str, Any] | None:
        with self._lock:
            return self._store.get(key)

    def put(self, key: str, value: dict[str, Any]) -> None:
        with self._lock:
            self._store[key] = value
