#!/usr/bin/env bash
# Export ClickHouse daily partitions with row counts and hashes.
# Usage: export-partitions.sh [YYYY-MM-DD]

set -euo pipefail
. "$(cd "$(dirname "$0")" && pwd)/lib.sh"
load_dotenv_keys_only

need docker

day="${1:-$(date -u +%Y-%m-%d)}"
out="$DATA_DIR/exports/partitions/$day"
mkdir -p "$out"

which="$(pick_compose)"
ch_query() { echo ""; }

case "$which" in
  local)
    ch_query() {
      docker compose -f "$ROOT/compose.yaml" exec -T clickhouse \
        clickhouse-client --user pumphound --password pumphound --query "$1"
    }
    ;;
  prod)
    ch_query() {
      docker compose -f "$ROOT/deploy/compose.production.yaml" exec -T clickhouse \
        clickhouse-client --user "${CLICKHOUSE_USER:-pumphound}" --password "${CLICKHOUSE_PASSWORD:?}" --query "$1"
    }
    ;;
  none)
    echo "no compose stack is running" >&2
    exit 1
    ;;
esac

hash_file() {
  if have shasum; then shasum -a 256 "$1" | awk '{print $1}'; else sha256sum "$1" | awk '{print $1}'; fi
}

day_compact="${day//-/}"
echo "export partitions day=$day"

rows=()
# raw_sightings is partitioned by toYYYYMMDD(observed_at)
for table in raw_sightings provider_observations portfolio_marks; do
  file="$out/${table}.jsonl"
  q="SELECT * FROM pumphound.${table} WHERE toYYYYMMDD(observed_at) = ${day_compact} FORMAT JSONEachRow"
  if [[ "$table" == "portfolio_marks" ]]; then
    q="SELECT * FROM pumphound.${table} WHERE toYYYYMMDD(at_time) = ${day_compact} FORMAT JSONEachRow"
  fi
  if ! ch_query "$q" >"$file"; then
    : >"$file"
  fi
  count="$(wc -l <"$file" | tr -d ' ')"
  digest="$(hash_file "$file")"
  rows+=("{\"table\":\"pumphound.${table}\",\"partition\":\"$day\",\"row_count\":$count,\"file\":\"${table}.jsonl\",\"sha256\":\"$digest\"}")
done

# slot-range tables: export nothing unless a slot window is provided
if [[ -n "${PUMPHOUND_EXPORT_SLOT_FROM:-}" && -n "${PUMPHOUND_EXPORT_SLOT_TO:-}" ]]; then
  for table in canonical_event_revisions market_snapshots; do
    file="$out/${table}.jsonl"
    ch_query "SELECT * FROM pumphound.${table} WHERE slot >= ${PUMPHOUND_EXPORT_SLOT_FROM} AND slot <= ${PUMPHOUND_EXPORT_SLOT_TO} FORMAT JSONEachRow" >"$file" || : >"$file"
    count="$(wc -l <"$file" | tr -d ' ')"
    digest="$(hash_file "$file")"
    rows+=("{\"table\":\"pumphound.${table}\",\"slot_from\":\"$PUMPHOUND_EXPORT_SLOT_FROM\",\"slot_to\":\"$PUMPHOUND_EXPORT_SLOT_TO\",\"row_count\":$count,\"file\":\"${table}.jsonl\",\"sha256\":\"$digest\"}")
  done
fi

json="$(printf '%s\n' "${rows[@]}" | paste -sd, -)"
cat >"$out/partition-manifest.json" <<EOF
{
  "kind": "partition-export",
  "day": "$day",
  "created_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "decoder_version": "pump-idl-2026-09-07",
  "partitions": [$json],
  "independent_of_database_retention": true
}
EOF

echo "wrote $out/partition-manifest.json"
