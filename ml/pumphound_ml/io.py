"""YAML/JSON/Parquet helpers and content hashes."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
from typing import Any, Mapping

import yaml

from pumphound_ml.types import to_plain

REPO_ROOT = Path(__file__).resolve().parents[2]


def resolve_path(path: str | Path, base: Path | None = None) -> Path:
    p = Path(path)
    if p.is_absolute():
        return p
    roots = [base, Path.cwd(), REPO_ROOT]
    for root in roots:
        if root is None:
            continue
        candidate = (root / p).resolve()
        if candidate.exists():
            return candidate
    return ((base or Path.cwd()) / p).resolve()


def load_yaml(path: str | Path) -> dict[str, Any]:
    target = resolve_path(path)
    with target.open("r", encoding="utf-8") as fh:
        data = yaml.safe_load(fh) or {}
    if not isinstance(data, dict):
        raise ValueError(f"{target} must contain a mapping")
    return data


def load_json(path: str | Path) -> Any:
    target = resolve_path(path)
    with target.open("r", encoding="utf-8") as fh:
        return json.load(fh)


def dump_json(path: str | Path, payload: Any) -> Path:
    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    with target.open("w", encoding="utf-8") as fh:
        json.dump(to_plain(payload), fh, indent=2, sort_keys=True)
        fh.write("\n")
    return target


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: str | Path) -> str:
    return sha256_bytes(Path(path).read_bytes())


def sha256_canonical(payload: Any) -> str:
    encoded = json.dumps(to_plain(payload), sort_keys=True, separators=(",", ":"), default=str)
    return sha256_bytes(encoded.encode("utf-8"))


def content_id(*parts: Any) -> str:
    return sha256_canonical(list(parts))[:32]


def load_jsonl(path: str | Path) -> list[dict[str, Any]]:
    target = resolve_path(path)
    rows: list[dict[str, Any]] = []
    text = target.read_text(encoding="utf-8")
    if target.suffix == ".json" and text.lstrip().startswith(("[", "{")):
        parsed = json.loads(text)
        if isinstance(parsed, list):
            return [row for row in parsed if isinstance(row, Mapping)]
        if isinstance(parsed, dict) and "events" in parsed:
            return list(parsed["events"])
        return [parsed]
    for line in text.splitlines():
        line = line.strip()
        if not line:
            continue
        rows.append(json.loads(line))
    return rows
