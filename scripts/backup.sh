#!/usr/bin/env bash
# Daily backup: Postgres dump, ClickHouse partition exports, journal listing,
# and a manifest with row counts and SHA-256 hashes. Never copies .env.

set -euo pipefail
. "$(cd "$(dirname "$0")" && pwd)/lib.sh"
load_dotenv_keys_only

need docker
need shasum || need sha256sum

hash_file() {
  if have shasum; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    sha256sum "$1" | awk '{print $1}'
  fi
}

day="${1:-$(date -u +%Y-%m-%d)}"
out="$DATA_DIR/exports/$day"
mkdir -p "$out"/{postgres,clickhouse,journals}

which="$(pick_compose)"
pg_dump_cmd=()
ch_query() { :; }

case "$which" in
  local)
    pg_dump_cmd=(docker compose -f "$ROOT/compose.yaml" exec -T postgres)
    ch_query() {
      docker compose -f "$ROOT/compose.yaml" exec -T clickhouse \
        clickhouse-client --user pumphound --password pumphound --query "$1"
    }
    ;;
  prod)
    pg_dump_cmd=(docker compose -f "$ROOT/deploy/compose.production.yaml" exec -T postgres)
    ch_query() {
      docker compose -f "$ROOT/deploy/compose.production.yaml" exec -T clickhouse \
        clickhouse-client --user "${CLICKHOUSE_USER:-pumphound}" --password "${CLICKHOUSE_PASSWORD:?}" --query "$1"
    }
    ;;
  none)
    echo "no compose stack is running; cannot backup" >&2
    exit 1
    ;;
esac

echo "backup day=$day compose=$which dest=$out"

echo "postgres"
"${pg_dump_cmd[@]}" pg_dump -U pumphound -d pumphound --no-owner --no-privileges \
  >"$out/postgres/pumphound.sql"
pg_hash="$(hash_file "$out/postgres/pumphound.sql")"
pg_bytes="$(wc -c <"$out/postgres/pumphound.sql" | tr -d ' ')"

pg_counts="$out/postgres/table_counts.tsv"
"${pg_dump_cmd[@]}" psql -U pumphound -d pumphound -At -c \
  "SELECT table_schema||'.'||table_name||E'\t'||n_live_tup
   FROM pg_stat_user_tables ORDER BY 1" >"$pg_counts" || true

echo "clickhouse"
tables=(raw_sightings canonical_event_revisions market_snapshots feature_snapshots predictions outcome_labels)
ch_manifest_rows=()
for table in "${tables[@]}"; do
  count="$(ch_query "SELECT count() FROM pumphound.${table}" | tr -d '\r' || echo 0)"
  file="$out/clickhouse/${table}.native"
  if ch_query "SELECT * FROM pumphound.${table} FORMAT Native" >"$file" 2>/dev/null; then
    :
  else
    : >"$file"
  fi
  ch_hash="$(hash_file "$file")"
  ch_manifest_rows+=("{\"table\":\"pumphound.${table}\",\"row_count\":${count:-0},\"file\":\"clickhouse/${table}.native\",\"sha256\":\"$ch_hash\"}")
done

echo "journals"
journal_count=0
if [[ -d "$DATA_DIR/journal" ]]; then
  find "$DATA_DIR/journal" -type f | sort >"$out/journals/listing.txt" || true
  journal_count="$(wc -l <"$out/journals/listing.txt" | tr -d ' ')"
else
  : >"$out/journals/listing.txt"
fi
journal_list_hash="$(hash_file "$out/journals/listing.txt")"

ch_json="$(printf '%s\n' "${ch_manifest_rows[@]}" | paste -sd, -)"

cat >"$out/manifest.json" <<EOF
{
  "kind": "pumphound-backup",
  "day": "$day",
  "created_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "compose": "$which",
  "decoder_version": "pump-idl-2026-09-07",
  "postgres": {
    "file": "postgres/pumphound.sql",
    "bytes": $pg_bytes,
    "sha256": "$pg_hash",
    "table_counts": "postgres/table_counts.tsv"
  },
  "clickhouse": [$ch_json],
  "journals": {
    "listing": "journals/listing.txt",
    "file_count": $journal_count,
    "sha256": "$journal_list_hash"
  },
  "secrets_included": false,
  "accepted": false,
  "note": "A backup is not accepted until restore-check.sh reproduces row counts and hashes in an isolated environment."
}
EOF

echo "wrote $out/manifest.json"
echo "run: bash scripts/restore-check.sh $out"
