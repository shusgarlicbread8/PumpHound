# Candidate enrichment

Candidate enrichment is an optional, append-only evidence pipeline for persisted
`HEURISTIC_V1` `BUY` decisions. Broad `WATCH` enrichment is intentionally disabled
so weak launches cannot consume the provider budget. The baseline decision and simulated
paper order are committed first and never wait for, or consume, enrichment output.
AI may read a cached dossier as advisory evidence; it cannot alter policy or orders.

## Levels

- **Level 0** reads PumpHound's PostgreSQL graph, profile, cohort, reputation, and
  permanent enrichment cache. It makes zero provider calls.
- **Level 1** uses Helius `getTransfersByAddress`, at most one page and 100 records
  per selected wallet. Later requests use `slot.gte=last_enriched_slot+1`.
- **Level 2** uses `getTransactionsForAddress` only when explicitly enabled, when
  an allowed eligibility reason is recorded, and after atomically reserving its
  configured 100-credit upper bound. It is disabled by default.

Wallet selection is evidence-only and ordered: creator, first five buyers,
optionally buyers six through ten, known largest holder, largest recent seller,
and observed high-reputation/cohort wallets. Wallets are deduplicated and hard
capped by `max_wallets_per_candidate` (default 10).

Funding is always labelled `INFERRED_INITIAL_FUNDER`, never ownership. The rule
uses the earliest meaningful inbound native SOL transfer while excluding self,
wrap/unwrap activity, null sources, and classified service addresses. Incomplete
history remains explicitly `INCOMPLETE` and receives lower configured confidence.

## Budgets and entitlement

`configs/enrichment-v1.json` is disabled by default. Daily, monthly,
per-candidate, candidates/hour, and wallets/candidate limits are hard constraints.
Credits are reserved atomically in PostgreSQL before each request and settled in
the request ledger afterward. Missing entitlement or key produces
`NOT_ENTITLED`; exhausted limits produce `BUDGET_WITHHELD`.

Safe enable settings (Developer+ Helius entitlement required):

```bash
export PUMPHOUND_ENRICHMENT_ENABLED=1
export PUMPHOUND_HELIUS_ENRICHMENT_ENTITLED=1
export PUMPHOUND_ENRICHMENT_CONFIG_PATH=configs/enrichment-v1.json
export PUMPHOUND_ENRICHMENT_LEVEL2_ENABLED=0
```

Set `PUMPHOUND_HELIUS_API_KEY` through the existing secret mechanism. Do not put
it in source control. Level 2 should remain disabled unless its cost and
eligibility policy are intentionally approved.

Status and cached evidence:

- `GET /api/v1/enrichment/status`
- `GET /api/v1/enrichment/candidates/{decision_id}`
- `GET /api/v1/enrichment/wallets/{wallet}`

These endpoints expose counts, credits, candidate state, inference confidence,
coverage, and evidence hashes. They never expose provider credentials.
