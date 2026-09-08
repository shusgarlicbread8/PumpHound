# Third-party notices

PumpHound is an independent research terminal. It is **not** affiliated with Pump.fun, PumpSwap, Solana, Helius or any listed vendor. This file is an inventory, not legal advice.

## Paper only

The product does not sign, submit or bundle transactions. Simulated fills are not trades on a venue. No notice below grants a right to execute live.

## Inferred intent is not ownership

Wallet graph edges, cohort scores and “linked” labels are statistical or heuristic. They do not prove common control, identity or fraud. Do not treat them as KYC, sanctions or beneficial-ownership findings.

## Protocol sources

| Component | Source | Notes |
|---|---|---|
| Pump / PumpSwap / pump_fees IDLs | [pump-fun/pump-public-docs](https://github.com/pump-fun/pump-public-docs) commit `9c82f61cb711b044a17f770ab8ce9f9bdf78f333` | Hashes in `vendor/pump-idl/SOURCE.lock`. Re-vendor; do not guess unknown layouts. |
| Solana RPC/WebSocket | Public or operator-supplied | Data can be delayed, duplicated, omitted or forked. |
| Helius LaserStream / history | Optional paid API | Entitlement is not implied by a key existing. Follow Helius terms. |
| Birdeye, Jupiter, DexScreener, Pyth | Optional | Use only under each provider’s current terms and rate limits. Jupiter is quote/inspection only. |

## Runtime

| Software | Role | License (typical; confirm upstream) |
|---|---|---|
| PostgreSQL 17 | Transactional store / paper ledger | PostgreSQL License |
| ClickHouse 26.x | Event/feature store | Apache-2.0 |
| NATS 2.x | JetStream bus | Apache-2.0 |
| Rust toolchain and crates | Collector, API, domain | Mixed MIT/Apache-2.0; see `Cargo.lock` |
| Python 3.12, uv, scientific stack | Research plane (when implemented) | Mixed; record in the SBOM at release |
| Prometheus, OpenTelemetry Collector | Metrics/traces | Apache-2.0 |
| Docker images | Deployment | Image licenses + OS packages |
| Tauri / React / Vite (planned) | Desktop | Mixed MIT/Apache-2.0 |

Generate an SBOM and vulnerability report before a public or shared binary release. Pin container images by digest for remote deploy.

## Model and data use

- Training exports are operator-owned research artifacts. Do not upload production DSNs or user secrets to Vast.ai.
- LLM prompts may contain public keys and decoded facts. Redact secrets. Default retention 30 days if prompts are stored.
- Cached metadata images: bounded, re-encoded; no automatic site crawl.

## Cost display

Provider list prices change. Show operator-configured budget (`PUMPHOUND_MONTHLY_PROVIDER_BUDGET_USD`) and measured spend when that telemetry exists. Do not treat planning envelopes as invoices.
