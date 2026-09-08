# Runbook: disk pressure (ClickHouse / journals)

## Signal

- Host `df` on the data or Docker volume below the doctor 20G comfort / 5G fail lines.
- ClickHouse reports parts or `too many parts`, merge backlog, or write errors.
- Collector `journal write failed` / `raw persist failed`.
- System status disk fields (when exported) in warning.

## Safety

1. Freeze new paper entries immediately if persistence is threatened. Show a hard degraded banner.
2. Shed AI and enrichment first. **Do not** sample away candidate program events without recording sampling.
3. Do not `DROP` raw_sightings to “make space” until a verified export exists.

## Recovery

1. `bash scripts/export-partitions.sh` for recent days; confirm partition manifests and hashes.
2. `bash scripts/backup.sh`.
3. After export verification, apply ClickHouse TTL or drop **provider raw** older than 30–90 days only. Keep canonical trades/launches.
4. Compact journal directories that are already persisted (`ingestion_batches.status` in `persisted`/`published`).
5. Expand the volume or move ClickHouse data to a larger disk. Restart ClickHouse; wait for healthcheck.
6. Resume paper only when free space is back above the operator threshold and a restore-check of the latest export passed.

## Done when

- Writes succeed; collector lag recovering.
- Export manifests exist for any deleted hot partitions.
- Paper banner cleared only after persistence is healthy.
