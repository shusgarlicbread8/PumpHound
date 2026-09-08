# Runbook: processed-fork rollback

## Signal

- Commitment moved from processed → confirmed/finalized and a signature disappeared or changed block membership.
- Canonicality flipped to `orphaned`.
- Duplicate sighting with conflicting `slot` / `tx_index`.
- Health `inconsistent`.

## Safety

1. Do not delete raw history. Retract with `action: RETRACT` and a new revision.
2. Do not apply a second economic trade for the surviving branch if the `event_id` already exists — update canonicality.
3. Paper fills that depended on the orphaned branch get a **new** simulation run (`canonical_reconciliation`), not an in-place edit.

## Recovery

1. Mark orphaned events; keep sightings.
2. Restore reducer state from the last consistent checkpoint/journal, then replay the surviving branch in `(slot, tx_index, instruction_path, event_index)` order.
3. Recompute features/predictions used **after** the fork point; old predictions stay immutable.
4. UI tape must show revision/commitment labels.

## Done when

- Latest revision per `event_id` matches the canonical block.
- No double-counted trades.
- Paper correction run linked with evidence IDs.
