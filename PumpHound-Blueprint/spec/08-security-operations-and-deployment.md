# Security, Operations and Deployment

## 1. Threat model

Assume:

- token metadata and linked content are hostile;
- public RPC data can be delayed, duplicated, omitted or forked;
- third-party API responses can be stale or malformed;
- model output can hallucinate or follow injected text;
- a local desktop can be lost or compromised;
- remote GPU marketplaces are untrusted infrastructure;
- dependency and IDL drift can change decoded meaning;
- future live execution would be a separate, higher-risk system.

## 2. No-wallet boundary

V1 accepts only public wallet addresses. It must not include:

- seed phrase or private-key input;
- keypair-file loading;
- remote-signing integration;
- transaction signing;
- transaction/bundle submission;
- UI copy that implies real funds are being traded.

Quote-only endpoints and unsigned transaction construction for inspection are allowed only if their payloads are never sent and the UI labels them as simulation inputs. Prefer quote-only calls.

## 3. Secrets

- Desktop-managed secrets live in the operating-system credential store; configuration stores only secret references.
- Server secrets are injected through a root-readable environment file or secret manager, never committed.
- Browser code receives no provider key.
- Redact authorization headers, RPC query keys, prompts and connection strings from logs.
- Assign separate development and production provider keys with the least available scope.
- Rotate a key after accidental exposure and record the incident.

Required secret references may include Helius, Birdeye, Pyth and SiliconFlow. Solana public RPC and local model endpoints may be keyless.

## 4. Untrusted content handling

Metadata fetcher requirements:

- allow `https` and explicitly configured decentralized gateways;
- resolve DNS and reject loopback, private, link-local, multicast and cloud-metadata IP ranges before every hop;
- maximum three redirects and revalidate each destination;
- strict response size, content-type and timeout limits;
- store hashes and sanitized extracted fields, never auto-render remote HTML;
- proxy images through a cache that decodes and re-encodes supported formats;
- strip SVG scripts/external references or rasterize before display;
- no automatic site crawling from metadata links.

## 5. Supply chain and protocol drift

- Pin Rust, Python and Node dependencies in lockfiles.
- Pin base container images by digest for remote deployment.
- Vendor official Pump IDLs with upstream URL, commit, retrieval date and SHA-256 in `SOURCE.lock`.
- On startup, display decoder/IDL version.
- Unknown discriminators or incompatible account layouts go to quarantine; never guess.
- Run decoder conformance fixtures before promoting a new IDL/decoder.
- Generate an SBOM and dependency vulnerability report for releases.

## 6. Source reliability

Collector controls:

- durable cursor/checkpoint by source and subscription;
- deterministic event IDs and idempotent writes;
- sequence and slot-gap detection;
- bounded retry with jitter and provider rate-limit awareness;
- fallback from LaserStream to Solana websocket where configured;
- backfill missing ranges through history APIs;
- processed-state rollback handling and canonicality updates;
- dead-letter/quarantine stream with operator-visible reasons.

UI health distinguishes: healthy, delayed, backfilling, degraded, disconnected and inconsistent.

## 7. Access and network boundaries

Local mode binds services to loopback by default. Remote mode:

- exposes no database or NATS port publicly;
- uses a private overlay such as Tailscale/WireGuard or an authenticated TLS gateway;
- uses short-lived application sessions and role-based access;
- rate-limits APIs and websocket clients;
- permits CORS only for the packaged desktop/web origin;
- records administrative configuration changes.

Do not run the stateful production database or hold provider secrets on a marketplace GPU host. Vast.ai may receive a versioned, minimally necessary, encrypted-or-public training dataset and should be treated as disposable.

## 8. Observability

Export OpenTelemetry traces and Prometheus-compatible metrics. Required measurements:

- source events/second, bytes/second and lag by commitment;
- duplicate, decode-failure, gap and rollback counts;
- stream-to-normalized, normalized-to-feature and feature-to-score latency;
- ClickHouse/Postgres/NATS health and disk usage;
- graph queue depth and crawl budget exhaustion;
- model inference latency, missing features and schema rejects;
- paper quote age, simulated failures, costs and reconciliation failures;
- AI latency, schema failure, cache hit and provider spend;
- websocket clients and UI render lag.

Logs are structured JSON with correlation IDs: `event_id`, `launch_id`, `prediction_id`, `decision_id`, `order_id`, `replay_id`. Avoid high-cardinality metrics labels for raw public keys; use logs/traces for those.

## 9. Data lifecycle

Suggested starting retention:

- raw canonical events: indefinitely in compressed Parquet plus hot ClickHouse window;
- normalized trades/launches: indefinitely;
- provider raw payloads: 30–90 days unless needed for decoder audit;
- full LLM prompts/responses: configurable, redacted, default 30 days;
- aggregates/features/predictions/decisions: indefinitely for research reproducibility;
- cached metadata images: bounded by size and last access.

Daily export partition manifests and database backups. Test restore into an isolated environment. A backup is not accepted until a restore and row/hash comparison succeeds.

## 10. Deployment progression

### Local Mac

- Tauri desktop plus local Rust/Python processes.
- Docker Compose for Postgres, ClickHouse and NATS.
- Apple-silicon local LLM; CPU inference for boosted trees.
- One-command start/stop from an operator script after configuration exists.

### Remote always-on research server

- x86-64 CPU VM in a region selected by measured latency to stream/RPC providers, commonly London, Amsterdam or Frankfurt.
- Containerized collector, API, worker, databases and observability.
- Desktop connects privately; all paper trading remains server-side and deterministic.
- GPU is unnecessary for collection or boosted-tree inference.

### Training jobs

- Export immutable feature/label partitions and a manifest.
- Rent a GPU only for temporal/GNN experiments that justify it.
- Verify output hashes and scan artifacts before promotion.
- Deploy promoted XGBoost/LightGBM models to CPU inference.

## 11. Capacity and degradation

The system must shed optional work before losing raw data:

1. pause AI investigations;
2. reduce external enrichment rate;
3. defer historical funding crawls;
4. reduce UI graph refresh frequency;
5. continue journaling and normalizing core program events;
6. if persistence is threatened, stop paper entries and show a hard degraded banner.

Never sample away candidate events without recording that sampling occurred.

## 12. Operational runbooks

The implementation must include executable runbooks for:

- provider disconnect and gap recovery;
- processed-fork rollback;
- unknown Pump instruction/event version;
- ClickHouse disk pressure;
- NATS consumer lag;
- paper-ledger reconciliation failure;
- corrupted replay manifest;
- leaked API key;
- model rollback;
- backup restore;
- full graceful shutdown and restart from checkpoints.

Every runbook names the operator-visible signal, safety action, recovery steps and evidence that recovery succeeded.

