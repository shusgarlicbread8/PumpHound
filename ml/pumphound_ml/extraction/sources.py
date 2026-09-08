"""Offline loading and content deduplication of official canonical event journals."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
from typing import Any

from pumphound_ml.io import load_json, resolve_path, sha256_canonical
from pumphound_ml.types import ChainEvent


def _source_configs(cfg: dict[str, Any]) -> list[dict[str, Any]]:
    configured = cfg.get("event_sources")
    if configured is None:
        uri = cfg.get("events_uri")
        configured = [{"name": "events", "uri": uri}] if uri else []
    if not isinstance(configured, list) or not configured:
        raise FileNotFoundError(
            "event_sources or events_uri is required; refusing to invent rows"
        )
    if any(not isinstance(source, dict) for source in configured):
        raise TypeError("event_sources must contain mappings")
    return configured


def _reject_raw_source(path: Path) -> None:
    lowered = {part.lower() for part in path.parts}
    if "raw" in lowered or path.name.lower().startswith("raw-"):
        raise ValueError(f"raw_provider_source_forbidden:{path}")


def _read_events(path: Path) -> tuple[list[tuple[str, ChainEvent]], str]:
    digest = hashlib.sha256()
    events: list[tuple[str, ChainEvent]] = []
    with path.open("rb") as handle:
        for number, raw_line in enumerate(handle, start=1):
            digest.update(raw_line)
            if not raw_line.strip():
                continue
            try:
                row = json.loads(raw_line)
            except json.JSONDecodeError as exc:
                raise ValueError(f"malformed_canonical_jsonl:{path}:{number}") from exc
            if not isinstance(row, dict):
                raise TypeError(f"canonical_event_not_mapping:{path}:{number}")
            if (
                not row.get("event_id")
                or row.get("slot") is None
                or not isinstance(row.get("payload"), dict)
                or not row["payload"].get("kind")
            ):
                raise ValueError(f"official_canonical_event_required:{path}:{number}")
            events.append(
                (
                    sha256_canonical(_economic_content(row)),
                    ChainEvent.from_mapping(row),
                )
            )
    return events, digest.hexdigest()


def _date_range(events: list[ChainEvent]) -> dict[str, str | None]:
    times = [event.event_time for event in events]
    return {
        "minimum_event_time": min(times).isoformat() if times else None,
        "maximum_event_time": max(times).isoformat() if times else None,
    }


def _economic_content(row: dict[str, Any]) -> dict[str, Any]:
    return {
        key: row.get(key)
        for key in (
            "event_id",
            "slot",
            "tx_index",
            "event_index",
            "instruction_path",
            "signature",
            "protocol",
            "program_id",
            "payload",
        )
    }


def load_canonical_sources(
    cfg: dict[str, Any],
) -> tuple[list[ChainEvent], dict[str, Any]]:
    by_id: dict[str, tuple[str, ChainEvent]] = {}
    source_reports: list[dict[str, Any]] = []
    total_duplicates = 0
    for source_index, source in enumerate(_source_configs(cfg)):
        name = str(source.get("name") or f"source-{source_index}")
        uri = source.get("uri")
        if not uri:
            raise FileNotFoundError(f"canonical source URI missing:{name}")
        path = resolve_path(str(uri))
        _reject_raw_source(path)
        if not path.is_file():
            if source.get("optional"):
                source_reports.append(
                    {
                        "name": name,
                        "uri": str(uri),
                        "available": False,
                        "row_count": 0,
                        "unique_added": 0,
                        "duplicates": 0,
                    }
                )
                continue
            raise FileNotFoundError(
                f"canonical event source missing:{path}. Refusing to invent rows"
            )
        before = path.stat()
        parsed_with_hash, file_hash = _read_events(path)
        after = path.stat()
        if source.get("require_stable") and (
            before.st_size != after.st_size or before.st_mtime_ns != after.st_mtime_ns
        ):
            raise RuntimeError(f"canonical_source_changed_during_read:{path}")
        parsed = [event for _, event in parsed_with_hash]
        source_duplicates = 0
        unique_added = 0
        for content_hash, event in parsed_with_hash:
            event_id = event.event_id
            previous = by_id.get(event_id)
            if previous is None:
                by_id[event_id] = (content_hash, event)
                unique_added += 1
                continue
            source_duplicates += 1
            total_duplicates += 1
            if previous[0] != content_hash:
                raise ValueError(f"canonical_event_id_content_conflict:{event_id}")
        source_manifest = None
        manifest_uri = source.get("manifest_uri")
        if manifest_uri:
            manifest_path = resolve_path(str(manifest_uri))
            if manifest_path.is_file():
                source_manifest = load_json(manifest_path)
        source_reports.append(
            {
                "name": name,
                "uri": str(uri),
                "available": True,
                "sha256": file_hash,
                "row_count": len(parsed),
                "unique_added": unique_added,
                "duplicates": source_duplicates,
                **_date_range(parsed),
                "upstream_manifest_uri": manifest_uri,
                "upstream_manifest_sha256": (
                    sha256_canonical(source_manifest)
                    if source_manifest is not None
                    else None
                ),
                "upstream_complete": bool(
                    isinstance(source_manifest, dict)
                    and (source_manifest.get("completeness") or {}).get(
                        "all_launches_complete"
                    )
                    is True
                ),
                "upstream_status": (
                    (source_manifest.get("completeness") or {}).get("status")
                    if isinstance(source_manifest, dict)
                    else "unavailable"
                ),
                "upstream_raw_count": (
                    source_manifest.get("raw_count")
                    if isinstance(source_manifest, dict)
                    else None
                ),
                "upstream_canonical_count": (
                    source_manifest.get("canonical_event_count")
                    if isinstance(source_manifest, dict)
                    else None
                ),
            }
        )
    events = [item[1] for item in by_id.values()]
    events.sort(key=lambda event: (*event.sort_key(), event.event_id))
    manifested_sources = [
        source for source in source_reports if source.get("upstream_manifest_uri")
    ]
    source_manifest = {
        "schema_version": 1,
        "kind": "canonical_event_sources",
        "sources": source_reports,
        "source_row_count": sum(
            int(source.get("row_count") or 0) for source in source_reports
        ),
        "deduplicated_event_count": len(events),
        "duplicate_event_count": total_duplicates,
        "conflicting_event_count": 0,
        "event_id_order_sha256": sha256_canonical([event.event_id for event in events]),
        "all_required_historical_sources_complete": bool(manifested_sources)
        and all(
            source.get("upstream_complete") is True for source in manifested_sources
        ),
        **_date_range(events),
    }
    source_manifest["content_sha256"] = sha256_canonical(source_manifest)
    return events, source_manifest
