# Runbook: NATS / consumer queue lag

## Signal

- JetStream consumer ack pending growing (`nats` monitoring `:8222` on the **private** network only).
- Feature/score latency rising; core-api `chain` flips `delayed`.
- Graph or ML queues (when implemented) report depth or crawl-budget exhaustion.
- Disk in the NATS volume climbing.

## Safety

1. Apply the shed order in `docs/OPERATIONS.md`: AI → enrichment → funding crawl → graph refresh.
2. Do not drop canonical program events to “catch up.”
3. If the NATS volume threatens the host, stop paper entries (`disk-pressure`).

## Recovery

1. `docker compose exec nats wget -qO- http://127.0.0.1:8222/jsz?consumers=1` (or production equivalent). Do not publish 8222 on `0.0.0.0`.
2. Identify the slow consumer (`core-api`, future ml-worker). Restart **that** process first; keep collector writing.
3. If a consumer is poison-payload looping, route the message to quarantine/dead-letter with a reason; do not ack-success.
4. Raise consumer replicas only after confirming the reducer is not CPU-bound on a single market lock.
5. After drain, confirm sequence/epoch: clients with a changed stream epoch must REST-rehydrate.

## Done when

- Ack pending returned to a bounded baseline.
- No dropped canonical records (replay the journal if needed).
- Lag metrics and UI health match; paper unfrozen only if persistence and lag policy allow.
