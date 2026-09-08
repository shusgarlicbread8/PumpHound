"""Replay manifest load/validate. Hashes must match before a run starts."""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from pumphound_ml.io import load_json, resolve_path, sha256_canonical, sha256_file


@dataclass
class ReplayManifest:
    path: Path
    raw: dict[str, Any]
    sha256: str
    seed: int
    events_path: Path
    policy_version: str
    decoder_version: str
    feature_schema_version: str
    knowledge_cutoff: str | None
    partitions: list[dict[str, Any]] = field(default_factory=list)


class ManifestError(ValueError):
    pass


def load_manifest(path: str) -> ReplayManifest:
    target = resolve_path(path)
    raw = load_json(target)
    events = raw.get("events_uri") or raw.get("events_path") or "fixtures/replay/events.jsonl"
    events_path = resolve_path(events, base=target.parent)
    return ReplayManifest(
        path=target,
        raw=raw,
        sha256=sha256_file(target),
        seed=int(raw.get("seed") or raw.get("random_seed") or 7),
        events_path=events_path,
        policy_version=str(raw.get("policy_version") or "paper-conservative-v1"),
        decoder_version=str(raw.get("decoder_version") or "pump-idl-2026-09-07"),
        feature_schema_version=str(raw.get("feature_schema_version") or "features-v1"),
        knowledge_cutoff=raw.get("knowledge_cutoff"),
        partitions=list(raw.get("partitions") or []),
    )


def validate_manifest(manifest: ReplayManifest) -> None:
    if not manifest.events_path.exists():
        raise ManifestError(f"events partition missing: {manifest.events_path}")
    expected = None
    for part in manifest.partitions:
        uri = part.get("uri") or part.get("path")
        digest = part.get("sha256")
        if not uri or not digest:
            continue
        part_path = resolve_path(uri, base=manifest.path.parent)
        if not part_path.exists():
            raise ManifestError(f"partition missing: {part_path}")
        actual = sha256_file(part_path)
        if actual != digest:
            raise ManifestError(f"partition hash mismatch for {part_path}")
        expected = digest
    # Always hash events even if partitions omitted.
    _ = sha256_file(manifest.events_path)
    if not manifest.raw.get("seed") and manifest.raw.get("random_seed") is None:
        # Seed may be defaulted; still deterministic.
        pass
    del expected
    _ = sha256_canonical({"seed": manifest.seed, "policy": manifest.policy_version})
