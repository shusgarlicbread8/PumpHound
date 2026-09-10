# Data durability

Honest state as of 2026-09-09. What actually protects this system's data against loss, what
doesn't yet, and what was found and fixed while checking.

## Where the data physically lives

Confirmed by inspecting the actual filesystem, not assumed:

- `data/`, `artifacts/`, and `target/` (Rust build cache) are symlinks into an APFS sparsebundle
  (`/Volumes/Crucial X9/PumpHoundData.sparsebundle`) mounted at `/Volumes/PumpHoundData`. Mounted
  automatically by `scripts/mount-pumphound-data.sh`.
- Docker Desktop's own VM disk (`~/Library/Containers/com.docker.docker/Data/vms/0/data/Docker.raw`)
  is *also* a symlink, to `/Volumes/Crucial X9/Docker/Docker.raw` — meaning the Postgres and
  ClickHouse named volumes (which live inside that VM disk, at
  `/var/lib/docker/volumes/pumphound_pumphound-{pg,ch}/_data`) are on the X9 too, not the internal
  Mac disk.

So: yes, the live databases and the journals/artifacts are genuinely on the Crucial X9, not just
believed to be. **That is also this setup's single point of failure** — the live data and every
backup taken with `scripts/backup.sh` land on the same physical drive. Losing, corrupting, or
disconnecting the X9 loses both the primary copy and every backup simultaneously. Nothing in this
repo copies a backup off that drive yet; that's a real, open gap, not something fixed here.

## Backups

`scripts/backup.sh` does a full `pg_dump` + ClickHouse table export + journal listing + a
SHA-256/row-count manifest. It existed but had **never been run** — `data/exports/` was empty
until 2026-09-09. Run manually once (~30GB+ output: Postgres dump ~7GB, `raw_sightings` export
alone ~20GB uncompressed). Given that size, don't run it every couple of hours — check for one
from the last ~24h and only refresh if stale. This script has no automation (no cron/launchd
entry) wired up yet; running it is currently a manual step.

## Crash/restart safety in the ingest path

### Bug found and fixed: checkpoint could advance past data that was never stored

`services/collector/src/main.rs`'s ingest loop wrote the raw sighting to Postgres/ClickHouse
(`persist_raw_batch`) and advanced the durable resume cursor (`record_checkpoint`) as two
**independent, concurrent** operations (`tokio::join!`), with the checkpoint's result discarded
(`let _ = checkpointed;`) regardless of whether the data write actually succeeded. A transient
Postgres/ClickHouse failure could advance the cursor past a sighting that was never durably
stored anywhere queryable — on restart the collector resumes from that cursor and would never
retry it. Fixed: the checkpoint now only advances after `persist_raw_batch` has confirmed
success; a failed persist logs why and leaves the checkpoint where it was, so the sighting is not
silently skipped on the next run. The write-behind journal append happens unconditionally before
either of these, independent of both — raw bytes were never actually at risk from this bug, but
the ability to know the checkpoint could be trusted was.

### Real gap found: there was no backfill at all

`SolanaWebsocketSource::session()` took a `cursor: Option<&ChainCursor>` parameter and never used
it (`_cursor`) — `ChainCursor` was durably written on every processed sighting and read back at
startup, then thrown away. `logsSubscribe` (the live path) is push-only with no replay of its
own. **Any downtime — a crash, a deploy, a laptop going to sleep — was a silent, permanent gap.**
The collector would come back up and just start receiving new live messages from whenever it
reconnected, with no attempt to recover what happened in between and no record that anything was
missed.

Added `crates/chain-source/src/backfill.rs`: on startup, if a checkpoint exists, page through
`getSignaturesForAddress` for each tracked program, keeping newer signatures and every other
transaction still in the checkpoint slot, and stopping only once a strictly older slot appears.
The checkpoint is the last processed transaction, not the end of that slot. Feed each missed
signature through the exact same decode/persist/journal pipeline live transactions
use. Runs concurrently with (not blocking) the live subscription, so there's no timing gap
between "backfill's last covered slot" and "live's first received slot" — any overlap is
deduplicated by the existing `sighting_id` / `ON CONFLICT DO NOTHING` path the same as any other
duplicate delivery.

**This does not mean arbitrary downtime is now recoverable — read this part before assuming it
does.** Measured live on 2026-09-09: the pump.fun program address is mentioned in roughly
55-60 transactions/second network-wide, almost all of it unrelated to any mint PumpHound tracks.
The first version of this backfill used a "generous" 50,000-signature cap per program; on its
first live restart it hit that cap without covering a checkpoint gap of only ~5 seconds, and
then spent the next several minutes fetching full transaction detail for all 50,000 of them
before being caught and killed manually. **Per-program signature enumeration can only ever
recover on the order of tens of seconds to roughly a minute of gap** before the RPC-call volume
becomes impractical and starts risking the provider's rate limits — it is a safety net for a
crash-restart-loop or a quick deploy, not a way to recover from the collector being down for
hours. The shipped caps (`MAX_PAGES_PER_PROGRAM = 3`, i.e. ~3,000 signatures/program, bounded to
~60s of fetching per program at a 20ms throttle) are sized to that reality, and a run that hits
the cap logs it explicitly (`backfill hit its page safety cap...`) rather than silently leaving
part of the gap unfilled.

**What would actually fix long-gap recovery** (not implemented): backfill per already-tracked
mint instead of per program. An individual token's own transaction volume is a small fraction of
the shared program address's, so `getSignaturesForAddress(mint)` for each mint in `launches` /
`hot_set_memberships` could realistically cover hours, not seconds — at the cost of one query per
mint instead of one per program. It still has a blind spot: a token that launched *and* was
fully missed during the outage isn't in that mint list yet, so brand-new launches during a long
outage remain unrecoverable by that method either. That's a real follow-up, not done here.

## What this means in practice today

- A brief crash, panic, or restart (seconds, maybe up to ~a minute) — the collector now catches
  up automatically and logs that it did (`pumphound_chain_source::backfill: backfill: caught up`,
  `sightings_recovered=N`).
- A longer outage (minutes to hours) — live tailing resumes correctly from "now" with no crash
  and no runaway RPC usage, but the gap itself is only partially recovered (however much the
  per-program cap could cover) and that is now logged explicitly, not silent. Fully closing a
  multi-hour gap needs the per-mint backfill described above, which does not exist yet.
- The underlying Postgres/ClickHouse data and the raw journal files are physically on the X9 and
  are not lost by a crash of the collector or core-api process itself — those are separate
  processes from the databases, which have their own crash recovery (Postgres WAL, ClickHouse's
  own MergeTree durability). This document is about the *ingest pipeline's* resume behavior, not
  the databases' own storage engines.
- The X9 being the only copy of both the live data and every backup is the biggest remaining risk
  in this list, and needs the user's own decision (a second drive, cloud storage, something else)
  rather than a silent code change.

## Bug found and fixed: `quote_mint` decoded as the System Program ID for most launches

Found 2026-09-09 via the Radar page rendering "—" for Price/MC on most rows despite the backend
having usable price data for individual launches on request. `SELECT quote_mint, count(*) FROM
pumphound.launches GROUP BY quote_mint` showed 7,700 of ~11,000+ launches with
`quote_mint = "11111111111111111111111111111111"` (32 zero bytes, Solana's System Program ID) —
the single largest group, ahead of the 2,766 correctly WSOL-quoted launches.

Root cause, confirmed by cross-checking `crates/protocol-pump/src/events.rs`'s `decode_create`/
`decode_trade`/migration decoders field-for-field against the vendored IDL
(`vendor/pump-idl/pump.json`'s `CreateEvent`/`TradeEvent`/`CompletePumpAmmMigrationEvent`): the
borsh layout was being read correctly end-to-end — every field surrounding `quote_mint`
(`token_program`, `is_mayhem_mode`, real reserve amounts, etc.) decoded to sane, valid values, not
garbage. This was **not** a misaligned read. pump.fun's own program leaves `quote_mint` as
`Pubkey::default()` (32 zero bytes) for the ordinary SOL-denominated bonding curve — the field is
only populated for launches priced against an explicit alternate asset (USDC, PUMP, ...). The
decoder already had the matching convention for the *sibling* fields — `virtual_quote_reserves`/
`quote_amount` fall back to `virtual_sol_reserves`/`sol_amount` when zero — but never applied that
same "zero means: this is SOL-denominated" mapping to `quote_mint` itself, so it was stored as a
literal invalid pubkey instead of WSOL's real mint. Fixed with a `normalize_quote_mint` helper
applied at all three read sites (create, trade, migrate); see the doc comment beside it in
`events.rs` for the full reasoning. Verified live: every launch decoded after the fix landed
(2026-09-09 23:47 UTC) correctly shows `quote_mint = So11111111111111111111111111111111111111112`.

**Historical rows are not yet corrected.** The ~7,700 pre-fix rows in `pumphound.launches` (and
their mirrors in `pumphound.markets`) still hold the invalid zero-pubkey `quote_mint`. Since the
mapping is now known to be unconditional (zero always means WSOL for this decoder), a corrective
`UPDATE ... SET quote_mint = 'So111...112' WHERE quote_mint = '111...111'` is safe to run — but it
was deliberately not run in the same session as the live restart, to avoid adding a bulk write
against Postgres while it was already recovering from unrelated I/O pressure (see below). Until
that correction runs, anything trained on or computed from historical `launches.quote_mint`
(price/decimals lookups, fee math, execution-twin quote-asset routing) should treat pre-2026-09-09
rows with the zero-pubkey value as WSOL-denominated, not as truly unknown.

## Found while restarting for the above: collector restarts were compounding two heavy jobs

Restarting the collector to deploy the `quote_mint` fix exposed a second, real issue: on startup,
`backfill_since_checkpoint` (see above) and `Stores::refresh_intelligence`'s periodic
`CO_BOUGHT_WITH` graph rebuild (a self-join over `graph_facts`, `services/collector/src/main.rs`)
were firing within the same 30-second window of each other on every restart, and together pushed
`/api/v1/home` past 15s for the duration of the overlap. Two fixes landed:

- `MAX_PAGES_PER_PROGRAM` (backfill's per-program signature cap) cut from 3 pages (3,000
  signatures) to 1 page (1,000): every observed restart was hitting the old cap outright, meaning
  the "brief restart" safety net was unconditionally spending its full ~5-minute worst-case budget
  regardless of how short the actual gap was. Real pump_curve throughput has evidently grown past
  the 55-60 tx/s originally measured.
- The collector's first intelligence refresh now waits 150s after startup instead of 30s, clearing
  backfill's (reduced) worst case before the graph rebuild's own heavy write joins in.

Even decoupled from backfill, the `CO_BOUGHT_WITH` rebuild alone was observed taking 1-2+ minutes
on its own (284K rows in `graph_facts`) and causing the same latency symptom whenever it fires —
this recurs every `PUMPHOUND_INTELLIGENCE_REFRESH_SECONDS` (already set to 900s/15min in `.env`,
not the 300s code default), so it's an infrequent but real cost. It's a pre-existing characteristic
of this job, not something introduced tonight.

## Root cause, found by reproducing it live: chronic disk I/O saturation, not a periodic job

The two fixes above were real but did not explain everything — spikes kept happening with no
15-minute-cycle job visibly running. Reproduced directly on 2026-09-10, ~01:15-01:25 UTC+1
(collector ~40 min into its post-fix run), by running three things concurrently for ~10 minutes:
a 1/sec curl loop against `/api/v1/home`, a 1/sec `pg_stat_activity` snapshot (state, wait_event,
query), and `iostat` on both the internal disk and the external Crucial X9 (`disk4`, see "Where the
data physically lives" above).

**Result: this is not intermittent.** 147 requests sampled, mean latency 3.2s (vs. the normal
0.2-0.4s), 36 spikes over 3s, up to **37.8s**, recurring roughly every 15-30 seconds throughout the
entire window — not clustered around the 900s refresh boundary. The refresh cycle's own window
(~01:20:06) was inside this sample and was not distinguishable from the baseline: the app was
already this degraded before and after it fired.

**It is disk I/O, confirmed at three independent layers:**

- **Postgres-level, live:** `pg_stat_activity` caught genuine `wait_event=DataFileRead` on 23
  distinct backend PIDs across the 10-minute sample, peaking at **27 concurrently active
  backends** in a single snapshot. Critically, the stuck backends were not one query — they
  included collector's own ingest writes (`INSERT INTO canonical_event_keys`, `graph_facts`,
  `graph_nodes`, `raw_archive_index`, `feature_snapshots`, `decisions`,
  `candidate_lifecycle_events`, `model_predictions`) waiting on the exact same wait event as
  core-api's reads. Both `Stores::connect()` callers (`services/core-api/src/main.rs`,
  `services/collector/src/main.rs`) hardcode `max_connections(8)` in
  `crates/storage/src/lib.rs:39` independently, so 27 concurrent backends means real cross-process
  contention on Postgres itself, not one process's pool queueing.
- **App-level, historical:** sqlx's own slow-statement log (`/private/tmp/core-api-creator-risk.log`,
  1s threshold) recorded **15,672** warnings over the prior 19 hours, 729 of them over 10s, peak
  **72.7s**. The damning detail: this included bare `SELECT 1` (`readiness.rs:176`,
  `routes.rs:130/240` — a health-check query touching zero tables) 353 times and bare `COMMIT` 303
  times. A query that touches no data being slow rules out query plans, missing indexes, or any
  single table as the cause — something below the SQL layer is stalling arbitrary backend activity.
- **OS-level, live:** `iostat` showed the external X9 (`disk4`) sustaining **100-300+ MB/s and
  2,000-10,000+ transactions/sec** continuously throughout the sample, while the internal SSD
  (`disk0`) sat at a few hundred tps and a few MB/s the whole time — the X9 is doing all the work.
  Postgres data is 9.2GB, ClickHouse data is 14GB; the internal disk has 108GB free.

**A compounding, independent factor found on the same host:** at the time of capture, physical
memory was 47GB/48GB used with **swap 93.6% full** (17.25GB/18.4GB) and 39.9M/47.3M cumulative
swapins/swapouts, and load average was 12-18 on a 14-core machine. `docker stats` showed ClickHouse
alone at 155% CPU. This means Postgres backends can stall from host-level memory/CPU scheduling
pressure even independent of disk contention — consistent with spikes that show nothing unusual in
a single `pg_stat_activity` snapshot (the stall can be a page fault or scheduling delay, not a
running query). This is a general property of this dev machine's current load (Docker Desktop VM +
native Rust/Python services + a full IDE and browser session), not something specific to
PumpHound's code.

**`CO_BOUGHT_WITH` itself is not newly implicated.** It already has a supporting index
(`graph_facts_asset_mint_edge_source`, migration `015_intelligence_refresh_indexes.sql`) and its
self-join was already bounded to the earliest 200 wallets/mint after an earlier incident (see the
comment above the query in `crates/storage/src/lib.rs:1052`, "blew this join up to hundreds of
millions of intermediate rows and stalled Postgres for 20+ minutes"). In this reproduction it was
one participant competing for saturated disk bandwidth, not a uniquely pathological query plan —
no further query rewrite is indicated by this data.

**What would actually move the needle (not done here, needs a deliberate maintenance window):**

- Move Postgres's (and ideally ClickHouse's) data directory to the internal SSD. It's confirmed
  idle while the X9 is saturated, and both datasets (23GB combined) fit easily in the 108GB free.
  This does trade away some of the "everything lives on one removable drive" portability the X9
  setup has today — but it also *improves* the backup story from "Where the data physically lives"
  above, since primary data and `scripts/backup.sh`'s output would no longer share one physical
  disk.
- Do **not** raise `max_connections(8)` in `crates/storage/src/lib.rs:39` as a fix for this. The
  bottleneck is I/O throughput, not connection slots — more concurrent connections means more
  concurrent random I/O demand on the same saturated disk, which would likely make `DataFileRead`
  contention worse, not better.
- The host's own memory pressure (swap 93.6% full) is worth the user's attention independently of
  any PumpHound change — it can stall any process unpredictably regardless of what this app does.

**Caveat on this measurement:** the reproduction harness itself added continuous load (1 req/s to
`/api/v1/home`, 1 extra `psql` process/s) on top of whatever was already happening, which likely
inflated the observed frequency/severity somewhat above pure organic baseline. This doesn't change
the diagnosis — the `DataFileRead` waits on collector's own ingest tables, the 19-hour historical
slow-query log (which predates this session's monitoring entirely), and the swap/load state are all
independent of the measurement harness — but the exact "3.2s mean, spike every 15-30s" figures
above are an upper bound, not a clean baseline number.
