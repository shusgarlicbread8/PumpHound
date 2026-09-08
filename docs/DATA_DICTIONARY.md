# Data dictionary

Canonical types live in `crates/domain`. JSON uses decimal **strings** for atomic amounts and slots. JavaScript numbers are rejected for money.

## Time

| Field | Meaning |
|---|---|
| `slot` / `block_time` | Economic time on chain |
| `observed_at` | When PumpHound received the fact |
| `computed_at` | When a derived value was computed (wall clock, including replay compute) |
| `knowledge_cutoff` | Latest information admitted to this view. Replay uses a **virtual** cutoff |
| `available_from_slot` | First slot at which the fact could be known |

A live decision may only use inputs observed and computed before `decision_at`. Replay must not admit future wallet labels just because the wallet existed earlier.

## Identifiers

```text
event_id    = sha256(chain + signature + program_id + instruction_path + event_index)
sighting_id = sha256(source + source_session + source_cursor + raw_payload_hash)
```

Business IDs are UUIDv7. Duplicate deliveries of the same signature must not create a second economic trade.

## Quality

| Grade | Meaning |
|---|---|
| A | Required program scope, exact order, fresh state, required graph/history |
| B | Core market exact/fresh; optional enrichment missing |
| C | Provisional order, stale, or incomplete required scope — display/watch only |
| UNAVAILABLE | No trustworthy state; no quote or prediction-based entry |

Quality is **not** token safety.

## Chain events

Tagged payloads: `launch.created`, `trade.executed`, `transfer.observed`, `launch.migrated`, `launch.completed`, `liquidity.changed`, `protocol.state_changed`, `decode.quarantined`. Failed transactions stay as evidence; they do not apply economic reserve deltas.

A decoded trade and its settlement transfers are not two purchases.

## Stores (see DDL)

PostgreSQL (`db/postgres/001_core.sql`) owns launches, markets, graph aggregates, model registry, **paper ledger** (portfolios, orders, reservations, fills, movements, lots), watchlists, settings. Paper tables are already in `001_core.sql`; there is no `002_paper.sql`.

ClickHouse (`db/clickhouse/001_events.sql`) owns append-only raw sightings, canonical revisions, snapshots, features, predictions, labels. Query the latest revision and exclude `RETRACT` / orphaned rows. Do not sum revision tables.

## Features and labels

Formulas: `PumpHound-Blueprint/spec/10-feature-and-label-dictionary.md`. Division by zero is null plus a missingness flag unless the dictionary says otherwise. `curve_progress` is a display metric, not proof of graduation.

Initial labels: `graduated_by_6h_v1`, `up_50_before_down_20_15m_v1`, `severe_collapse_1h_v1`, `net_return_15m_v1`, `exit_hazard_60s_v1`. Censor when the window is incomplete. Quiet is not collapse.

## Paper quantities

```text
gross_input  = reserve_input + fees taken from input
net_output   = reserve_output - fees taken from output
```

Fees included in gross input are not debited again. SOL fees are not subtracted from USDC output. Lot consumption cannot exceed acquisition. Spendable cash cannot go negative.

## Graph

Edges are evidence scores, not ownership. Hub-adjusted overlap is not “the same person.” UI copy must say inferred / linked, never owned.

## API snapshots

Every snapshot includes `as_of`, `quality`, `snapshot_id` and `stream_cursor`. Partial reads with `quality.grade != A` are allowed; fabricated success payloads are not.
