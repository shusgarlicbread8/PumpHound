# Runbook: corrupted replay manifest

## Signal

- `load-replay.sh` or `just replay` rejects the JSON.
- File hash ≠ `raw_partitions[].sha256`.
- `deterministic_result_sha256` changes between two same-seed runs for a previously pinned result.
- Missing IDL lock, decoder version or policy URI.

## Safety

1. Do not “fix” a live-captured manifest by silently pointing at another partition.
2. Do not mix today’s labels into a historical `knowledge_cutoff`.
3. Synthetic fixtures must keep the `SYNTHETIC — not live mainnet` label.

## Recovery

1. Re-hash every listed file. If a partition changed, create a **new** manifest and retire the old one.
2. Restore partitions from backup/export if the bytes drifted on disk.
3. Confirm decoder/IDL/policy/seed pins.
4. Run twice; compare content hashes, ignoring wall-clock `computed_at` presentation.

## Done when

- Two runs match.
- `future_dependency_count == 0`.
- Manifest hash recorded in the replay report.
