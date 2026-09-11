# Historical Pump/PumpSwap corpus

`history_mode` defaults to `helius_full` when `PUMPHOUND_HELIUS_API_KEY` is
available. This uses Helius-exclusive `getTransactionsForAddress` with full raw
transactions; it does not use the deprecated Enhanced Transactions parser.

Estimate the configured upper bound without making an RPC request:

```sh
just backfill-history-estimate configs/history-large-25k.json
```

Run the conservative smoke:

```sh
just backfill-history configs/history-smoke.json
```

Run the bounded 25k-launch target:

```sh
just backfill-history configs/history-large-25k.json
```

Collect the last 90 days as complete weekly windows, then import into the live
store and repair launch times / PumpSwap identity / decision-age snapshots:

```sh
just backfill-history-recent
just apply-sql db/postgres/020_evidence_layer.sql
just repair-evidence
```

Re-run the current official decoders over the saved raw archive without making
provider calls:

```sh
just backfill-history-redecode configs/history-large-25k.json
```

Re-decode stages and atomically replaces only derived canonical/decode-state
artifacts. Raw archive files and provider pagination checkpoints are preserved.
Normal resume also detects missing or stale per-signature decode state and
reprocesses those archived transactions before requesting another provider
page.

The large config round-robins 30 complete, chronologically listed day windows.
It stops at 25,000 unique launch observations, 500,000 unique raw
transactions, or 600 RPC requests. At 1,000 full records per successful call,
100 credits is the conservative per-call upper bound, so the configured
maximum estimate is 60,000 credits. Current Helius documentation meters full
responses at 10 credits per 100 records, rounded up with a 10-credit minimum;
the manifest records both that returned-record estimate and the conservative
upper bound.

If `getTransactionsForAddress` is unavailable or denied by the account plan,
the default permits an explicit, recorded `standard_rpc` fallback. Other RPC
failures do not change modes. A single limiter controls aggregate request rate
across all windows and programs. HTTP 429 and 5xx responses are retried using
`Retry-After` when supplied, otherwise bounded exponential backoff with
deterministic jitter. Exhausting retries leaves that stream resumable while
independent streams continue.

Set `allow_standard_fallback_on_denial` to `false` to fail closed, or set
`"history_mode": "standard_rpc"` to request the fallback directly. Every
manifest reports the effective mode, fallback reason, calendar gaps, and the
fact that address history cannot prove complete launch coverage.

Quality gates are part of each config. A terminal run with insufficient decode
success, excessive schema incompatibility, too few canonical events, or too few
true/provisional launches writes `failed_quality_gate` to the manifest and exits
nonzero. Zero canonical events always fail. Provider/fetch errors and decoder
schema incompatibilities are reported as separate, aggregated groups with
bounded signature examples and a hard cap of 200 groups per category.
Recovered transient attempts are reported separately and removed from
unresolved provider failures after a later successful page. Any unresolved
provider/fetch failure produces a nonzero exit.

Checkpoints store separate `source_semantics_hash` and `run_config_hash`
values. Rate, concurrency, retry/backoff, timeout, and quality-gate changes are
resume-compatible. Changing source windows, program IDs, mode, filters, or
transaction encoding requires a new output directory.
