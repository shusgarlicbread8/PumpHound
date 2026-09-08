# PumpHound Product Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver the complete local-first PumpHound intelligence terminal, live paper desk, graph, predictive engine, research lab and optional investigator.

**Architecture:** Rust owns live ingestion, state, API and paper execution. Python owns graph batches and research models. Tauri displays canonical state through REST and sequenced WebSocket messages; Postgres, ClickHouse, JetStream and Parquet preserve provenance.

**Tech Stack:** Tauri 2, React 19, TypeScript, Rust stable, Python 3.12, PostgreSQL 17, ClickHouse 26.x, NATS JetStream 2.x, XGBoost/LightGBM.

**Spec:** [System design](../spec/00-system-design.md), [UX](../spec/01-product-and-ux.md), [architecture](../spec/02-architecture-and-repo.md), all subsystem specs and [contracts](../contracts/01-domain-and-event-contracts.md).

## Global Constraints

- V1 supports Pump.fun bonding curves and PumpSwap canonical pools on Solana mainnet.
- V1 contains no transaction signer or live-money submission path.
- Atomic quantities and slots cross JSON boundaries as decimal strings.
- Observed facts, derived relationships, model predictions and LLM interpretation remain distinct.
- Live and replay share deterministic domain reducers and paper quote math.
- Unknown/stale/partial data stays explicit.
- Create infrastructure and packages only inside the first task that uses them.
- The user's requested sequence overrides test-first defaults: milestones 1–8 build product with narrow smoke checks; milestone 9 performs comprehensive verification.

---

## Execution order

| Order | Milestone | Phase plan | Exit condition |
|---|---|---|---|
| 1 | Live chain slice | [01](01-live-terminal-and-paper.md), tasks 1–3 | real launches/trades persisted and inspectable |
| 2 | Live terminal | [01](01-live-terminal-and-paper.md), tasks 4–7 | Radar → token → evidence works with live updates |
| 3 | Live paper desk | [01](01-live-terminal-and-paper.md), tasks 8–11 | paper buy → realistic fill → position → exit → reconciled ledger |
| 4 | Wallet intelligence | [02](02-graph-and-ml.md), tasks 12–15 | funding/co-trade graph, creator/wallet history and explanations |
| 5 | Predictive engine | [02](02-graph-and-ml.md), tasks 16–20 | point-in-time dataset → calibrated model → live shadow prediction |
| 6 | Strategy lab | [03](03-lab-ai-production-verification.md), tasks 21–23 | deterministic replay and honest policy comparison |
| 7 | AI investigator | [03](03-lab-ai-production-verification.md), tasks 24–25 | evidence-cited local/cloud investigation with dissent |
| 8 | Always-on runtime | [03](03-lab-ai-production-verification.md), tasks 26–28 | private server connection, recovery, backup and operations |
| 9 | Comprehensive verification | [03](03-lab-ai-production-verification.md), tasks 29–32 | acceptance evidence, no unsupported completion claims |

## Milestone tracker

- [x] M1 — Live chain slice (collector + decoders + stores; live capture pending Docker + RPC)
- [x] M2 — Live terminal (Radar/token/system wired to core-api; visual QA pending `just dev`)
- [ ] M3 — Live paper desk (quote/fill/risk present; GUI ticket exists; ledger persistence incomplete)
- [ ] M4 — Wallet intelligence
- [ ] M5 — Predictive engine
- [ ] M6 — Strategy lab
- [ ] M7 — AI investigator
- [ ] M8 — Always-on runtime
- [ ] M9 — Comprehensive verification

No box is pre-checked in this blueprint: this deliverable is a build specification, not an implemented application.

## First coding session

Read the first phase plan and implement its first live slice immediately. The first session's product target is:

```text
provider transaction -> decoded launch/trade -> durable record -> inspection output
```

Then move directly to the terminal. Do not begin with a month of model training, a monorepo generator, a plugin system, enterprise login or an exhaustive test framework.

## Required command interface

Create each command when its feature exists; these are the final operator-facing commands:

| Command | Meaning |
|---|---|
| `just dev` | start dependencies, collector/API and desktop in local mode |
| `just stop` | graceful local shutdown, preserve data/checkpoints |
| `just capture-live` | capture configured live sources without desktop |
| `just inspect-launch <mint>` | print canonical snapshot, evidence and quality |
| `just backfill <manifest>` | resume bounded historical import |
| `just graph-refresh <manifest>` | compute a versioned graph projection |
| `just dataset <config>` | point-in-time features/labels + split manifest |
| `just train <config>` | reproducible baseline/tree training job |
| `just replay <manifest>` | deterministic paper/research run |
| `just verify-ledger <portfolio-id>` | reconcile balances, reservations and lots |
| `just verify` | full non-network deterministic verification suite |
| `just verify-live` | explicit read-only provider smoke checks |
| `just release-check` | package, dependency scan and release evidence |
| `just backup` / `just restore-check <backup>` | backup and isolated restore verification |

Commands must print selected mode and data source. No command named `trade` or `execute-live` exists in v1.

## Scope map

| Requirement | Implemented by |
|---|---|
| GUI layout, navigation, keyboard, accessibility | 4–7, 31 |
| Pump/PumpSwap decode, SOL/USDC, fee/config history | 1–3, 8, 29 |
| real-time ingestion, gaps/forks and history | 1–3, 12, 26, 29 |
| live paper portfolio/risk/ledger | 8–11, 30 |
| wallet/funding/creator/cohort graph and intent | 12–15 |
| market/graph features and honest labels | 16–17 |
| XGBoost/LightGBM/calibration/registry | 18–20 |
| replay, exit comparison, robustness and research UI | 21–23 |
| local AI/SiliconFlow and evidence committee | 24–25 |
| security, privacy, operations and deployment | 6, 24, 26–28, 32 |
| provider configuration/budget/entitlements | 1, 7, 26; [procurement spec](../spec/09-tools-accounts-costs-and-expansion.md) |
| future venues and eventual live execution boundary | [expansion spec](../spec/09-tools-accounts-costs-and-expansion.md) |

## Session handoff file

At the end of a coding session, update `docs/BUILD_STATUS.md` with:

```text
Completed milestone/task IDs:
Current working end-to-end path:
Exact launch command:
Live/replay source and captured date range:
Last observed health/lag:
Next concrete task:
Known blockers and degraded enrichments:
Changed contract/decoder/model versions:
```

Fill every field with actual evidence or `none`. Do not replace the detailed plans with this status file.

## Completion boundary

“Software complete” means the product and acceptance requirements are implemented. “Profitable strategy” is a separate empirical outcome and is not guaranteed by software completion. A successful v1 may correctly conclude that no tested strategy has a defensible net edge.

