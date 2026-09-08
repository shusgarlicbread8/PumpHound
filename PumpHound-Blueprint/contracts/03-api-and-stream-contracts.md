# REST, WebSocket and Worker Contracts

Base path: `/api/v1`. The Rust API owns validation and OpenAPI generation. Browser clients never call chain/data/AI providers directly.

## 1. General rules

- Decimal strings for atomic amounts and slots; RFC3339 UTC timestamps.
- Every mutable command includes `client_request_id` or an `Idempotency-Key`.
- Key reuse with a different payload returns `409 IDEMPOTENCY_CONFLICT`.
- Cursor pagination, default 100 and maximum 500 rows. Graph responses have separate node/edge budgets.
- `as_of_slot` and `knowledge_cutoff` are accepted on historical evidence queries.
- Each snapshot response includes `as_of`, `quality`, `snapshot_id` and `stream_cursor`.
- `stream_cursor` belongs to one server stream epoch; client detects a changed epoch and rehydrates.
- `404` unknown subject, `409` incompatible state, `422` invalid contract, `429` rate limit, `503` required component unavailable.
- Read requests can return partial data with `quality.grade != A`; never return a fabricated success payload.

Error envelope:

```json
{
  "error": {
    "code": "QUOTE_STALE",
    "message": "The market changed after this quote expired.",
    "retryable": true,
    "request_id": "019...",
    "details": {"quote_id": "019...", "expired_at": "2026-09-07T12:00:00.000000Z"}
  }
}
```

## 2. Endpoint catalog

| Method/path | Request | Response / required behaviour |
|---|---|---|
| `GET /health` | none | readiness, mode, uptime, decoder/model versions |
| `GET /system/status` | none | per-source lag/coverage, queues, DB/disk, model/AI status |
| `GET /system/capabilities` | none | enabled providers and entitlements, no secret values |
| `GET /search` | q/type/cursor | token/wallet/creator/signature resolution and canonical links |
| `GET /settings` | none | non-secret settings and secret-present flags |
| `PATCH /settings` | expected revision/validated changes | audited configuration revision; secrets via native reference flow |
| `GET /launches` | age/venue/quote/filter/sort/cursor | Radar rows, pinned as-of and cursor |
| `GET /launches/{id}` | as-of optional | header, market snapshot, score/provenance summaries |
| `GET /launches/{id}/trades` | time range/cursor | canonical tape, revisions/commitment labels |
| `GET /launches/{id}/candles` | interval/from/to | OHLCV with quote mint, no-trade gap markers |
| `GET /launches/{id}/holders` | as-of/cursor | program-excluded and cluster-adjusted holdings |
| `GET /launches/{id}/features` | window/as-of/schema | FeatureVector and definitions |
| `GET /launches/{id}/predictions` | window/model/as-of | immutable PredictionBundles |
| `GET /launches/{id}/decisions` | portfolio/cursor | decisions, abstentions, gate reasons |
| `GET /launches/{id}/graph` | as-of/depth/filter/budget | bounded graph + hidden-node counts |
| `GET /wallets/{address}` | as-of | balances/history coverage, reputation, label evidence |
| `GET /wallets/{address}/activity` | range/cursor | trades/transfers/launches with evidence |
| `GET /wallets/{address}/funding` | as-of/max_depth | bounded paths, hub labels, crawl status |
| `GET /creators/{address}` | as-of | launch outcomes, linked cohorts, reputation posterior |
| `GET /clusters/{id}` | version/as-of | memberships, lineage, metrics and evidence |
| `GET /evidence/{id}` | as-of optional | fact/derived/predicted/interpreted record + raw pointer |
| `POST /paper/quotes` | market/side/input/slippage/portfolio | PaperQuote, cost breakdown, risk preview |
| `POST /paper/orders` | PaperOrderRequest | accepted/rejected order, never live execution |
| `POST /paper/orders/{id}/cancel` | request ID | cancel only before simulated arrival |
| `GET /paper/portfolios` | none | paper/replay portfolios |
| `POST /paper/portfolios` | name/funding lots/policy | portfolio, initial ledger movements and active run |
| `GET /paper/portfolios/{id}` | run optional | cash, positions, orders, marks and breaker state |
| `GET /paper/portfolios/{id}/ledger` | run/cursor | immutable movements/lots/reconciliation status |
| `GET /paper/portfolios/{id}/performance` | range/run | net/gross metrics, cost and outlier attribution |
| `POST /paper/portfolios/{id}/breaker-reset` | reason/request ID | audited reset under policy rules |
| `POST /policies` | complete validated configuration | immutable policy version and hash |
| `POST /paper/portfolios/{id}/policy` | policy version/request ID | audited active-version switch |
| `GET /models` | target/status | registry and model cards |
| `POST /models/train` | dataset manifest/config | queued job ID; no automatic promotion |
| `POST /models/{version}/promote` | shadow/champion/reason | validated compatibility + audit event |
| `POST /models/{version}/retire` | reason | retained artifact and retirement state |
| `POST /replays` | pinned manifest or range/config | replay job/run ID |
| `POST /replays/{id}/control` | pause/resume/step/speed | current virtual clock/state |
| `GET /replays/{id}` | none | progress, manifest hash, output references |
| `POST /investigations` | subject/as-of/roles/provider | investigation job ID |
| `GET /investigations/{id}` | none | role outputs, citations, validation and dissent |
| `GET /jobs/{id}` | none | state, progress, error code and resumable cursor |
| `POST /jobs/{id}/cancel` | request ID | cooperative cancellation |
| `GET /watchlists` | none | lists and item counts |
| `POST /watchlists` | name | saved list |
| `PUT /watchlists/{id}/items/{subject}` | subject type | idempotent add |
| `DELETE /watchlists/{id}/items/{subject}` | subject type | idempotent remove |
| `GET /alerts` | cursor/ack status | alert events with evidence |
| `POST /alert-rules` | condition/cooldown/name | validated versioned rule |
| `POST /alerts/{id}/acknowledge` | request ID | acknowledgement |
| `GET /exports/{job_id}` | none | authenticated local/remote artifact download |

No endpoint takes a private key, signs a transaction, or submits a bundle. A request containing an unsupported execution mode returns `422 UNSUPPORTED_EXECUTION_MODE`.

## 3. Paper order request example

```json
{
  "client_request_id": "01900000-0000-7000-8000-000000000001",
  "portfolio_id": "01900000-0000-7000-8000-000000000002",
  "launch_id": "01900000-0000-7000-8000-000000000003",
  "market": "<validated market public key from the token response>",
  "side": "buy",
  "input": {
    "mint": "So11111111111111111111111111111111111111112",
    "atomic": "10000000",
    "decimals": 9
  },
  "output_mint": "<validated token mint from the token response>",
  "max_slippage_bps": 100,
  "quote_id": "01900000-0000-7000-8000-000000000004",
  "policy_version": "paper-conservative-v1",
  "decision_id": "01900000-0000-7000-8000-000000000005"
}
```

The angle-bracket fields above describe runtime values, not hard-coded sample keys. The UI always copies these IDs from API responses, never from display symbols.

## 4. WebSocket protocol

Endpoint: `/api/v1/stream`. Authenticate via the packaged app's session mechanism; no long-lived bearer token in a URL. On connection the client sends:

```json
{"type":"subscribe","request_id":"r1","topics":["radar","system"],"resume":{"epoch":"e1","sequence":"1839"}}
```

Server messages:

```ts
interface StreamMessage<T> {
  type: 'snapshot' | 'delta' | 'event' | 'heartbeat' | 'resync_required' | 'error';
  schema_version: 1;
  epoch: string;
  sequence: string;
  topic: string;
  sent_at: string;
  as_of_slot: string | null;
  payload: T;
}
```

Topics:

```text
radar
launch:{launch_id}
wallet:{public_key}
graph:{subject_id}
paper:{portfolio_id}
replay:{run_id}
job:{job_id}
alerts
system
```

Send heartbeats every five seconds. Keep a bounded resume buffer. When resume history expired, emit `resync_required` with affected topics. Client fetches REST snapshot and atomically swaps it in before applying messages newer than the returned stream cursor.

The server allocates sequence numbers per authenticated client subscription stream, not from a hidden global stream that produces false gaps when other topics are filtered out. Epoch changes on stream-state loss/restart.

Batch Radar updates to at most 10 Hz and price/graph view updates to the screen's budget. Persist every underlying event even if UI deltas are coalesced. Never coalesce away order status, alerts or retractions.

## 5. Worker boundary

The Python worker is an authenticated internal HTTP service, not public:

| Method/path | Contract |
|---|---|
| `POST /internal/predict` | ordered FeatureVector + requested model version → PredictionBundle |
| `POST /internal/jobs` | job kind + immutable manifest/config → job ID |
| `GET /internal/jobs/{id}` | progress/result/error |
| `GET /internal/models/{version}/schema` | exact ordered feature/target schema |
| `GET /internal/health` | loaded model versions/queue/memory |

Use bounded queues, explicit timeouts and circuit breakers. A model timeout produces a named heuristic fallback or abstention; it must not re-use another token's or an incompatible window's score.

## 6. Read models

Radar row contract:

```ts
interface RadarRow {
  launch_id: string;
  mint: string;
  symbol: string | null;
  age_seconds: number;
  protocol: 'pump_curve' | 'pump_swap';
  quote_mint: string;
  price_quote: string | null;
  executable_depth_quote: string | null;
  curve_progress: number | null;
  buyers: number;
  independent_clusters: number | null;
  flow_60s_quote: string | null;
  graduation_6h: number | null;
  collapse_1h: number | null;
  heuristic_rank: number | null;
  prediction_id: string | null;
  stance: 'WATCH' | 'WAIT' | 'AVOID' | 'PAPER_CANDIDATE';
  paper_position_id: string | null;
  flags: Array<{code: string; evidence_id: string; severity: 'info'|'warning'|'critical'}>;
  quality: DataQuality;
  as_of: ObservationPoint;
}
```

Before trained models exist, probability fields remain null and the GUI displays `MODEL NOT TRAINED`; a heuristic rank is labelled `HEURISTIC`, never a percentage probability.
