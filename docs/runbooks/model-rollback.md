# Runbook: model rollback

## Signal

- Schema rejects, NaN/Inf, or feature-order mismatch on `/internal/predict`.
- Champion metrics collapse on held-out / live shadow vs the card.
- Wrong `feature_schema_version` after a deploy.
- Operator wants the previous champion.

## Safety

1. Promotion is **never** automatic. Train jobs only queue.
2. Rollback does not delete artifacts. Status becomes `retired` or `rejected`.
3. Decisions already persisted keep their `prediction_id` / `model_version`.
4. If no model is safe, run `MODEL NOT TRAINED` / heuristic / abstain. Do not reuse another token’s score.

## Recovery

1. `GET /models` — note champion, shadow, candidates.
2. `POST /models/{previous}/promote` with reason `rollback` after compatibility check (schema, horizon, label version).
3. Point the worker at the previous bundle hash. Confirm `/internal/models/{version}/schema`.
4. Replay a pinned manifest with the old version; hash must match the historical report for that version.
5. Record the audit row (actor, before/after hash, reason).

## Done when

- Live predictions persist **before** any policy action, with the rolled-back version id.
- Shadow of the bad model can continue offline.
- UI model card shows the rollback, not a silent swap.
