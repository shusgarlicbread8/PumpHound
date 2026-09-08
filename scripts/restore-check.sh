#!/usr/bin/env bash
# Verify a backup directory: hashes, counts, and a JSON receipt.
# Does not overwrite the live database. Isolated restore is a follow-up
# operator action documented in docs/runbooks/restore.md.

set -euo pipefail
. "$(cd "$(dirname "$0")" && pwd)/lib.sh"

backup_path="${1:-}"
if [[ -z "$backup_path" ]]; then
  echo "usage: restore-check.sh <backup_dir>" >&2
  exit 2
fi
[[ -d "$backup_path" ]] || { echo "not a directory: $backup_path" >&2; exit 1; }
[[ -f "$backup_path/manifest.json" ]] || { echo "missing manifest.json" >&2; exit 1; }

hash_file() {
  if have shasum; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    sha256sum "$1" | awk '{print $1}'
  fi
}

need python3

receipt="$backup_path/restore-check.json"
echo "restore-check $backup_path"

python3 - "$backup_path" "$receipt" <<'PY'
import hashlib, json, sys
from pathlib import Path

root = Path(sys.argv[1])
receipt_path = Path(sys.argv[2])
manifest = json.loads((root / "manifest.json").read_text())
mismatches = []
checked = []

def sha256(p: Path) -> str:
    h = hashlib.sha256()
    with p.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()

pg = root / manifest["postgres"]["file"]
if not pg.exists():
    mismatches.append({"path": str(pg), "reason": "missing"})
else:
    got = sha256(pg)
    checked.append({"path": str(pg.relative_to(root)), "sha256": got, "bytes": pg.stat().st_size})
    if got != manifest["postgres"]["sha256"]:
        mismatches.append({"path": str(pg.relative_to(root)), "reason": "hash_mismatch"})

for row in manifest.get("clickhouse", []):
    p = root / row["file"]
    if not p.exists():
        mismatches.append({"path": row["file"], "reason": "missing"})
        continue
    got = sha256(p)
    checked.append({"path": row["file"], "sha256": got, "bytes": p.stat().st_size, "row_count": row.get("row_count")})
    if got != row["sha256"]:
        mismatches.append({"path": row["file"], "reason": "hash_mismatch"})

listing = root / manifest["journals"]["listing"]
if listing.exists():
    got = sha256(listing)
    checked.append({"path": str(listing.relative_to(root)), "sha256": got})
    if got != manifest["journals"]["sha256"]:
        mismatches.append({"path": "journals/listing.txt", "reason": "hash_mismatch"})

accepted = len(mismatches) == 0
receipt = {
    "kind": "restore-check",
    "backup_day": manifest.get("day"),
    "checked_at": __import__("datetime").datetime.utcnow().strftime("%Y-%m-%dT%H:%M:%SZ"),
    "files_checked": checked,
    "mismatches": mismatches,
    "live_database_overwritten": False,
    "accepted": accepted,
    "note": "Hash comparison only. Isolated restore + replay is required before treating the backup as accepted for disaster recovery.",
}
receipt_path.write_text(json.dumps(receipt, indent=2) + "\n")
print(json.dumps({"accepted": accepted, "mismatches": len(mismatches), "receipt": str(receipt_path)}))
sys.exit(0 if accepted else 1)
PY
