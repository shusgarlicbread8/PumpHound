# PumpHound Build Blueprint

PumpHound is a local-first Solana microcap intelligence terminal. It watches Pump.fun bonding-curve launches and their PumpSwap migration, reconstructs market state from the chain, builds a persistent temporal wallet graph, estimates outcome probabilities, and tests decisions through a realistic live paper-trading engine.

This pack is written for a coding agent. It is deliberately implementation-first:

- Build a working live GUI before broad testing or model experimentation.
- Use raw on-chain events as truth; enrichment APIs are optional evidence.
- Keep every prediction and graph conclusion point-in-time correct.
- Never let an LLM calculate prices, generate core features, or decide fills.
- Do not add live-money signing or transaction submission in the first product.
- Do not scaffold empty services, fake screens, placeholder buttons, or unused abstractions.

## Locked product decision

PumpHound v1 is an intelligence and paper-trading product, not a sniper bot.

The first supported venues are:

1. Pump.fun bonding curve
2. PumpSwap canonical pools

The architecture exposes a `LaunchpadAdapter` boundary so other Solana launchpads and DEXs can be added without rewriting the graph, feature, prediction, paper, or GUI layers.

## Read in this order

1. [AGENT_MASTER_PROMPT.md](AGENT_MASTER_PROMPT.md)
2. [spec/00-system-design.md](spec/00-system-design.md)
3. [spec/01-product-and-ux.md](spec/01-product-and-ux.md)
4. [spec/02-architecture-and-repo.md](spec/02-architecture-and-repo.md)
5. [spec/03-data-sources-and-protocols.md](spec/03-data-sources-and-protocols.md)
6. [contracts/01-domain-and-event-contracts.md](contracts/01-domain-and-event-contracts.md)
7. [contracts/02-storage-schema.sql](contracts/02-storage-schema.sql)
8. [contracts/03-api-and-stream-contracts.md](contracts/03-api-and-stream-contracts.md)
   - [ClickHouse DDL](contracts/02b-clickhouse-schema.sql)
   - [Default paper policy](contracts/04-default-policy.yaml)
   - [Initial training configuration](contracts/05-training-config.yaml)
9. The remaining subsystem specs
10. [plans/00-master-build-sequence.md](plans/00-master-build-sequence.md)
11. The phase plan matching the current milestone
12. [acceptance/DEFINITION_OF_DONE.md](acceptance/DEFINITION_OF_DONE.md)

## Folder map

| Path | Purpose |
|---|---|
| `spec/00-system-design.md` | Canonical scope, invariants, product boundaries |
| `spec/01-product-and-ux.md` | User flows, screens, interaction and visual language |
| `spec/02-architecture-and-repo.md` | Processes, storage, repository, technology choices |
| `spec/03-data-sources-and-protocols.md` | Pump/PumpSwap decoding, Solana streams, enrichment |
| `spec/04-wallet-graph-and-intent.md` | Graph semantics, clustering, reputation and intent inference |
| `spec/05-features-labels-and-models.md` | Point-in-time features, labels, XGBoost training and scoring |
| `spec/06-paper-trading-and-replay.md` | Fill model, fees, latency, portfolio, replay and risk |
| `spec/07-ai-investigator.md` | Local/SiliconFlow evidence-grounded explanation layer |
| `spec/08-security-operations-and-deployment.md` | Secrets, resilience, Mac and server deployment |
| `spec/09-tools-accounts-costs-and-expansion.md` | Complete tool/account inventory, budget, local models and expansion gates |
| `spec/10-feature-and-label-dictionary.md` | Exact initial formulas, windows, labels and quality definitions |
| `contracts/` | Copyable types, SQL and external API/stream contracts |
| `plans/` | Ordered implementation plans with exact deliverables |
| `acceptance/` | End-to-end definition of done and validation gates |
| `sources/` | Primary-source technical and research references |

See [the blueprint handoff review](acceptance/BLUEPRINT_REVIEW.md) for what was checked and what the coding agent must verify during implementation.

## Build order at a glance

| Milestone | Visible outcome |
|---|---|
| 1. Live chain slice | Real Pump/PumpSwap events in ClickHouse and a diagnostic CLI |
| 2. Live terminal | Radar, token detail, trade tape, curve state and system health in Tauri |
| 3. Paper desk | Live simulated orders, fills, positions, P&L and risk controls |
| 4. Wallet graph | Funding/co-trade graph, creator history, clusters and evidence explorer |
| 5. Predictive engine | Point-in-time datasets, calibrated XGBoost models and immutable predictions |
| 6. Strategy lab | Replay, walk-forward comparison, threshold sweeps and robustness reports |
| 7. Investigator | Evidence-cited local/cloud AI memos that cannot place trades |
| 8. Production runtime | 24/7 CPU server, remote desktop connection, observability and backups |
| 9. Verification | Decoder fixtures, deterministic replay, accounting invariants, load and UI QA |

## Non-goals for v1

- Private-key import, live order signing, automatic execution or copy trading
- Ethereum or non-Solana chains
- A social feed, chat room, token launch flow or wallet custody
- Reinforcement learning before a trustworthy supervised/paper dataset exists
- Kubernetes, Kafka, a separate graph database or a feature-store product
- Claims that a wallet cluster proves common ownership or criminal intent

## Definition of the moat

The valuable asset is not a green/red token score. It is the versioned, point-in-time history connecting:

```text
wallets ↔ funding ↔ launch cohorts ↔ trades ↔ creators ↔ tokens ↔ outcomes
```

Every visible conclusion must remain auditable back to exact transactions, slots, feature values, model version and decision policy.

## Safety line

All v1 trading actions are explicitly labelled `PAPER`. Order creation is accepted only in
`LIVE_PAPER_HEURISTIC`, `LIVE_PAPER_SHADOW_ML`, `LIVE_PAPER_ML`, or `REPLAY`;
watch/research modes cannot place orders. A future live executor would be a separate service and
security review, not a hidden flag in this codebase.
