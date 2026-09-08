# Canonical Domain and Event Contracts

This is the boundary contract for Rust, Python, storage and TypeScript. Generate JSON Schema and TypeScript from the Rust domain definitions. Do not maintain hand-edited copies in each service.

## 1. Primitive representations

| Concept | Rust/core | JSON | Database |
|---|---|---|---|
| public key | validated 32-byte public key | base58 string | text/String |
| signature | validated 64-byte signature | base58 string | text/String |
| atomic quantity | checked `u128` | unsigned decimal string | numeric(39,0)/UInt128 |
| signed cash movement | checked `i128` | signed decimal string | numeric(39,0)/Int128 |
| slot | `u64` | decimal string | bigint/UInt64 |
| transaction/event index | bounded unsigned integer | number | integer/UInt32 |
| time | UTC microsecond instant | RFC3339 with microseconds | timestamptz/DateTime64(6) |
| price/exchange rate | rational or decimal | decimal string + base/quote | Decimal(38,18) when bounded |
| ML feature/probability | finite `f64` | number or explicit missing | Float64/Nullable(Float64) |
| business ID | UUIDv7, or content hash where specified | string | uuid/String |

JSON must never expose an atomic quantity as a JavaScript number. Every parser rejects NaN, Infinity, negative unsigned values and out-of-range integers. ML float values are not reused for reserve or ledger arithmetic.

## 2. Shared TypeScript shape

```ts
type Pubkey = string;
type Signature = string;
type Atomic = string;
type Slot = string;
type Timestamp = string;
type Id = string;
type Commitment = 'processed' | 'confirmed' | 'finalized';
type Canonicality = 'provisional' | 'canonical' | 'orphaned' | 'unknown';
type Mode =
  | 'LIVE_WATCH'
  | 'LIVE_PAPER_HEURISTIC'
  | 'LIVE_PAPER_SHADOW_ML'
  | 'LIVE_PAPER_ML'
  | 'REPLAY'
  | 'RESEARCH';
type Protocol = 'pump_curve' | 'pump_swap';
type Health = 'healthy' | 'delayed' | 'backfilling' | 'degraded' | 'disconnected' | 'inconsistent';

interface AssetAmount {
  mint: Pubkey;
  atomic: Atomic;
  decimals: number;
}
interface ObservationPoint {
  slot: Slot;
  tx_index: number | null;
  event_index: number | null;
  event_time: Timestamp | null;
  observed_at: Timestamp;
  knowledge_cutoff: Timestamp;
  commitment: Commitment;
}
interface Provenance {
  source: string;
  source_record_id: string;
  payload_sha256: string;
  decoder_version: string;
  idl_sha256: string | null;
  raw_partition_uri: string;
}
interface DataQuality {
  grade: 'A' | 'B' | 'C' | 'UNAVAILABLE';
  score: number;
  chain_lag_ms: number | null;
  market_lag_ms: number | null;
  ordering: 'exact' | 'provisional' | 'unknown';
  coverage: 'complete_for_scope' | 'partial' | 'unknown';
  missing_fields: string[];
  stale_components: string[];
  reasons: string[];
}
```

`complete_for_scope` means complete for the configured programs/wallet subscriptions and checked slot range. It never means that PumpHound has observed every Solana transaction or knows every private intent.

## 3. Raw capture and canonical events

`RawChainEnvelope` contains source/session/cursor, receipt timestamp, source slot, blockhash/parent when available, source commitment, raw transaction/account/block payload, payload hash and transport metadata. Account updates include account key, owner, lamports, executable flag, raw data, transaction signature when available and write version.

Canonical event identity:

```text
event_id = sha256(chain + signature + program_id + instruction_path + event_index)
sighting_id = sha256(source + source_session + source_cursor + raw_payload_hash)
```

A transaction can be sighted by multiple providers and commitments without creating another economic trade. Slot/block metadata is versioned separately; an orphaned event is retained with a tombstone/retraction.

```ts
interface EventEnvelope<T> {
  schema_version: 1;
  event_id: Id;
  revision: number;
  action: 'UPSERT' | 'RETRACT';
  chain: 'solana-mainnet';
  protocol: Protocol;
  program_id: Pubkey;
  signature: Signature;
  slot: Slot;
  blockhash: string | null;
  tx_index: number | null;
  instruction_path: number[];
  event_index: number;
  block_time: Timestamp | null;
  observed_at: Timestamp;
  computed_at: Timestamp;
  available_from_slot: Slot;
  commitment: Commitment;
  canonicality: Canonicality;
  transaction_succeeded: boolean;
  provenance: Provenance;
  payload: T;
}

interface LaunchCreated {
  kind: 'launch.created';
  launch_id: Id;
  mint: Pubkey;
  creator: Pubkey;
  market: Pubkey;
  quote_mint: Pubkey;
  base_decimals: number;
  quote_decimals: number;
  supply_atomic: Atomic;
  metadata_uri: string | null;
  protocol_flags: Record<string, boolean | string>;
}
interface TradeExecuted {
  kind: 'trade.executed';
  launch_id: Id | null;
  mint: Pubkey;
  market: Pubkey;
  trader: Pubkey;
  fee_payer: Pubkey;
  side: 'buy' | 'sell';
  base_amount_atomic: Atomic;
  quote_amount_atomic: Atomic;
  quote_mint: Pubkey;
  fee_components: FeeComponent[];
  reserve_snapshot_id: Id | null;
  instruction_name: string;
}
interface TransferObserved {
  kind: 'transfer.observed';
  source_owner: Pubkey | null;
  destination_owner: Pubkey | null;
  source_account: Pubkey;
  destination_account: Pubkey;
  amount: AssetAmount;
  transfer_role: 'external' | 'trade_settlement' | 'fee' | 'rent' | 'unknown';
}
interface LaunchMigrated {
  kind: 'launch.migrated';
  launch_id: Id;
  mint: Pubkey;
  old_market: Pubkey;
  new_market: Pubkey;
  quote_mint: Pubkey;
}
interface ProtocolStateChanged {
  kind: 'protocol.state_changed';
  account: Pubkey;
  config_snapshot_id: Id;
  affected_markets: Pubkey[];
}
interface FeeComponent {
  kind: 'protocol' | 'creator' | 'lp' | 'network' | 'priority' | 'tip' | 'rent';
  asset: AssetAmount;
  schedule_version: string;
  charged_on_failure: boolean;
}
type ChainEvent = EventEnvelope<LaunchCreated | TradeExecuted | TransferObserved | LaunchMigrated | ProtocolStateChanged>;
```

Liquidity deposit/withdraw, completion-without-migration and balance/account snapshots are additional tagged payloads in the same envelope, with account IDs, before/after atomic balances and instruction identity. Add their concrete fields from the vendored IDL at the decoder boundary; preserve all raw fields in the raw record, not a free-form UI payload.

## 4. Ordering, transaction atomicity and reconciliation

1. Deduplicate sightings, then decode each transaction once per decoder version.
2. Apply only successful transaction economic events. Failed transactions remain evidence for failure/cost estimates.
3. Order by `(slot, tx_index, instruction_path, event_index)` when transaction index is known.
4. Apply the transaction's economic events atomically to the reducer. Do not expose a halfway-applied transaction snapshot.
5. Do not count a decoded trade and its settlement transfers as separate purchases.
6. Account updates reconcile state; they do not blindly apply another reserve delta already applied by the trade event.
7. If transaction order is unknown, show a provisional tape and withhold execution-grade quotes until a consistent snapshot/order is established.
8. Preserve a bounded checkpoint/journal so retractions can restore state and replay the surviving branch.
9. Reconcile recent processed events to canonical block membership; slot progression alone does not prove that every observed transaction survived.

Standard websocket fallback often lacks transaction index. Fetch block membership/order when possible. Never substitute arrival order while claiming exact replay.

## 5. Time semantics: online and historical

There are two clocks and one cutoff:

- **Economic time:** when a transaction belongs on chain.
- **Knowledge time:** when the system could know a fact or derived feature.
- **Observation cutoff:** the chosen decision-time view of both.

For live decisions, every dependency must have been observed and computed before `decision_at`.

For historical replay, `computed_at` records the real time the replay computation ran, while `knowledge_cutoff` is virtual decision time. Every input is filtered by simulated historical availability. Do not reject a replay merely because its computation ran today; do not admit a future wallet label merely because its underlying wallet existed earlier.

Use a bitemporal query condition equivalent to:

```sql
event_slot <= :cutoff_slot
AND known_at <= :knowledge_cutoff
AND (superseded_at IS NULL OR superseded_at > :knowledge_cutoff)
```

For backfilled data whose original observation time is unavailable, label the run `historical_chain_time_only`. It may evaluate chain-state models, but must not claim measured live observation latency.

## 6. Market, features and predictions

```ts
interface MarketSnapshot {
  snapshot_id: Id;
  market: Pubkey;
  mint: Pubkey;
  quote_mint: Pubkey;
  protocol: Protocol;
  as_of: ObservationPoint;
  canonicality: Canonicality;
  virtual_base_atomic: Atomic;
  virtual_quote_atomic: Atomic;
  real_base_atomic: Atomic;
  real_quote_atomic: Atomic;
  effective_base_atomic: Atomic;
  effective_quote_atomic: Atomic;
  complete: boolean;
  migrated_market: Pubkey | null;
  config_snapshot_id: Id;
  quality: DataQuality;
}
interface FeatureVector {
  feature_snapshot_id: Id;
  launch_id: Id;
  observation_window_seconds: number;
  as_of: ObservationPoint;
  schema_version: string;
  values: Record<string, number | null>;
  dependency_ids: Id[];
  quality: DataQuality;
}
interface PredictionBundle {
  prediction_id: Id;
  feature_snapshot_id: Id;
  model_version: string;
  calibrated: boolean;
  computed_at: Timestamp;
  outputs: Record<string, {p?: number; p10?: number; p50?: number; p90?: number}>;
  explanation_id: Id | null;
  abstention_reason: string | null;
}
```

Unknown reserve/config states make a quote unavailable; do not fill missing state with zero. Model outputs include the label/horizon in the key and the full target definition in registry metadata.

## 7. Decisions, orders and fills

```ts
interface PaperOrderRequest {
  client_request_id: Id;
  portfolio_id: Id;
  launch_id: Id | null;
  market: Pubkey;
  side: 'buy' | 'sell';
  input: AssetAmount;
  output_mint: Pubkey;
  max_slippage_bps: number;
  quote_id: Id;
  policy_version: string;
  decision_id: Id;
}
interface PaperQuote {
  quote_id: Id;
  market_snapshot_id: Id;
  input: AssetAmount;
  expected_output: AssetAmount;
  minimum_output: AssetAmount;
  fee_components: FeeComponent[];
  price_impact_bps: number;
  expires_at: Timestamp;
  quality: DataQuality;
}
type PaperOrderStatus =
  | 'ACCEPTED' | 'QUEUED' | 'FILLED' | 'CANCELLED'
  | 'REJECTED_SLIPPAGE' | 'REJECTED_LIQUIDITY' | 'REJECTED_STALE_STATE'
  | 'SIMULATED_TX_FAILURE' | 'RISK_REJECTED';
interface DecisionRecord {
  decision_id: Id;
  mode: Mode;
  launch_id: Id;
  prediction_id: Id | null;
  heuristic_version: string | null;
  policy_version: string;
  feature_snapshot_id: Id;
  decision_at: Timestamp;
  knowledge_cutoff: Timestamp;
  action: 'WATCH' | 'ABSTAIN' | 'PAPER_BUY' | 'PAPER_SELL';
  reason_codes: string[];
  risk_snapshot_id: Id | null;
}
interface PaperFill {
  fill_id: Id;
  order_id: Id;
  simulation_run_id: Id;
  arrival_slot: Slot;
  arrival_at: Timestamp;
  input: AssetAmount;
  output: AssetAmount;
  fee_components: FeeComponent[];
  market_snapshot_id: Id;
  canonicality: Canonicality;
}
```

`AcceptedPaperOrder`, `PaperOrderResult`, `PredictionRequest` and `PointInTimeContext` are internal Rust wrappers over these canonical types. Define them in `crates/domain/src/` with their exact dependencies; they must not introduce another quantity/time representation.

Watch/abstention decisions without a portfolio may have no risk snapshot. Any paper-buy/paper-sell command must reference a valid portfolio/run risk snapshot before admission.

On a fork affecting a paper fill, append a `paper.correction` record referencing the affected fill and recompute a new simulation branch. Preserve the originally observed result for latency research. The GUI shows provisional and canonical-reconciled performance separately. Finalized reporting uses the reconciled branch, never silently mutated historical fills.

## 8. Durable publication and idempotency

Postgres control/ledger transactions write an outbox entry in the same transaction. A publisher acknowledges that entry after JetStream accepts it. Every consumer has an idempotency key unique to its effect.

ClickHouse is append-oriented and not a unique-key transactional database. Use a replayable raw journal/batch manifest and deterministic IDs; consumers/read views resolve the highest revision. Do not rely on eventual `ReplacingMergeTree` merges as immediate deduplication or as a cross-database transaction.

Required subjects:

| Subject | Payload | Consumer |
|---|---|---|
| `chain.normalized.v1` | canonical event revision | core-api, graph worker |
| `market.snapshot.v1` | MarketSnapshot reference | features, UI publisher |
| `feature.ready.v1` | FeatureVector reference | scorer |
| `prediction.ready.v1` | PredictionBundle reference | policy/UI |
| `decision.created.v1` | DecisionRecord | paper/UI |
| `paper.order.v1` | order state transition | ledger/UI |
| `graph.updated.v1` | subject/version/evidence IDs | feature/UI |
| `health.changed.v1` | component status | system/UI |

Store payloads or durable references before publication. A consumer acknowledges only after its own durable effect commits.
