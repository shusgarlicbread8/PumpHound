# Runbook: backup restore

## Signal

- Need to rebuild a machine, verify a daily backup, or recover from store corruption.
- `scripts/restore-check.sh` reported `accepted: false`.

## Safety

1. Never restore over the live volume without an explicit operator decision and a second snapshot of the current data dir.
2. Isolated restore first: new Compose project, new volumes, new `PUMPHOUND_DATA_DIR`.
3. Secrets stay out of backup tarballs. Re-inject keys from the credential store.

## Recovery

1. `bash scripts/backup.sh` on the healthy side if it still runs.
2. `bash scripts/restore-check.sh data/exports/YYYY-MM-DD` — hashes and counts must match the manifest.
3. Isolated Postgres: `psql` the dump into an empty database created from `001_core.sql` only if the dump is not already a full schema dump. Prefer `pg_dump` restore into a blank cluster.
4. Isolated ClickHouse: load Native/JSONEachRow files per table; compare `count()` to the manifest.
5. Copy journal files listed in `journals/listing.txt`. Resume collector from restored `source_checkpoints`.
6. Replay `fixtures/replay/manifest.json` or a live-captured manifest. Compare content hash, not wall-clock fields.
7. Only then cut DNS/desktop to the restored stack.

## Done when

- `restore-check.json` has `accepted: true`.
- Row counts and file hashes match.
- One historical run reproduces (`replay_id` + result hash).
- Paper ledger `verify-ledger` is clean on the restored portfolio, or the portfolio is marked research-only if the backup predates ledger rows.
