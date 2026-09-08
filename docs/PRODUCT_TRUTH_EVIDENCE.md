# PumpHound Product Truth — Evidence Report

**As of:** 2026-09-08T19:41:03Z (UTC)  
**Mode:** `LIVE_PAPER_SHADOW_ML` · PAPER ONLY  
**Machine captures:** `artifacts/truth-audit/runtime/live-20260908-2038/`  
**Spec:** `docs/PRODUCT_TRUTH_SPEC.md`

This report uses persisted Postgres/ClickHouse facts, live HTTP responses, and `verify-ledger`. It does **not** use a desktop GUI session. Numbers move while paper books are open; treat the digest as a single snapshot, not a live ticker.

---

## Repository state

There is **no git repository** in this workspace (`fatal: not a git repository`). No branch, commits, or remote exist.

A pre-change snapshot exists at `artifacts/truth-audit/baseline/source.tar.gz`.

Components changed for this milestone (no git, listed by function):

- Decoder identity: Pump / PumpSwap event IDs use program-data log ordinals, including ignored non-market events (`pump-events-identity-v2-2026-09-08`).
- PumpSwap mint/decimals on trade `raw_event`; storage `ensure_asset` uses them.
- Integrity quarantine tables and migrations `010`–`014` (`pumphound.entity_integrity_quarantines`). Compose mounts `013`/`014`.
- Canonical paper truth: `GET /api/v1/paper/truth`, `GET /api/v1/paper/traces/{id}`, `GET /api/v1/paper/champion-funnel`, `GET /api/v1/models/truth`. Home `/home` uses `truth::report` with `summary_only`.
- Champion book identity: manifest `policy_version=CHAMPION_SHORT_HORIZON_V1`, `execution_envelope_policy=INFORMATION_VALUE_EXPLORER_V1`. New decisions write `desk.book.key`. Historical champion decisions still say `INFORMATION_VALUE_EXPLORER_V1` and were **not** rewritten.
- Desktop: Trade defaults to ALL ACTIVE BOOKS; tables filter `QUARANTINED` rows from financial lists; Home/Research/System/Models consume the truth APIs.
- This continuation: `verify-ledger` headline `closed_trades` now joins verified positions (same CTE as truth) and reports `closed_trades_without_verified_position` instead of silently disagreeing.

ALPHA-LOOP-20260908 frozen manifests were not edited. Frozen hashes still match `FROZEN_LOOP_MANIFESTS` in `verify_ledger.rs`.

---

## What PumpHound can now demonstrably do

For the **same active-book scope** (9 running, unarchived, strategy-bearing paper books), Home, Trade truth, Strategies, and `verify-ledger` return the **same** open count, closed-position count, closed-trade count, and realized SOL P&L.

A closed paper trade can be opened as a **persisted evidence trace** (decision → order → arrival quote → fill → exit → FIFO P&L). If historical snapshots or labels are missing, Replay says `REPLAY_INCOMPLETE` and lists the exact gaps. It does not invent them.

Champion Short Horizon is a live paper book with **N=0**. The product shows a measurable rejection funnel rather than a fake trade count. There is **no champion** in the authoritative file registry.

When the collector is down, readiness is `SYSTEM RUNNING · TRADING BLOCKED` and `allow_new_entries=false`. When it is up and fresh, the same API is `TRADING READY`. That fail-closed path was observed in this session.

Capital remains simulated. No signing path was added.

---

## Truth contract

| Rule | Definition |
|---|---|
| `book_id` | `simulation_run_id` |
| Active book | `simulation_runs.state='running'` AND portfolio `archived_at IS NULL` AND manifest has `strategy` |
| Trade default scope | ALL ACTIVE PAPER BOOKS, window `all` unless the user filters |
| Closed trade (N) | One fully FIFO-consumed acquisition lot (`SUM(lot_consumptions.base_atomic)=lot.base_atomic`), lot/launch not integrity-quarantined, **and** attached to a verified (non-quarantined) position on the same book+mint |
| Closed position | `paper_positions.status='CLOSED'` on a verified position |
| Realized P&L | Sum of verified-position `realized_pnl_quote_atomic` in SOL lamports (`So1111…1112`, 9 decimals) |
| Quarantine | Source rows stay in the DB. Financial totals exclude them. UI tables hide `QUARANTINED` rows; summary still shows quarantine counts |
| Model authority | File registry `artifacts/models/registry.json` wins. Database-only champion promotion is rejected (`409 AUTHORITATIVE_REGISTRY_REQUIRED`) and was reverted |
| Policy vs envelope | Champion **selection** identity is `CHAMPION_SHORT_HORIZON_V1`. Size/hold/impact still use the explorer **execution envelope**. Reports prefer the manifest over historical `decisions.policy_version` |
| Replay | Reconstructs persisted evidence only. Missing snapshots/tape/labels stay missing |
| Paper only | No live signing |

---

## Contradictions found

### 1. Home showed positions while Trade showed none

**EXPECTED:** Same scope → same counts.  
**ACTUAL (original product):** Home used a multi-book aggregate; Trade defaulted to a single book (often `HEURISTIC_V1`, N=0 fills).  
**ROOT CAUSE:** Scope mismatch, not missing trades.  
**FIX:** Shared `/paper/truth` + Home `summary_only`; Trade default empty book filter = ALL ACTIVE BOOKS.  
**PROOF:** Snapshot below — Home ≡ truth ≡ strategies ≡ verify-ledger.

### 2. Strategy N=0 while “hundreds of trades exist”

**EXPECTED:** Each book’s N is that book’s closed FIFO lots.  
**ACTUAL:** Champion and HEURISTIC_V1 correctly report N=0. Other books have the trades.  
**ROOT CAUSE:** Cross-book totals were being mentally applied to one strategy.  
**FIX:** Per-book identity + sample/edge labels.  
**PROOF:** Champion N=0, buys=0, fills=0; Explorer N=91; shadows hold most of the P&L.

### 3. Registry champion vs Radar “not trained”

**EXPECTED:** One authority for champion status.  
**ACTUAL:** A database-only promotion of `candidate-52f0c7cebb0dbb37` to CHAMPION was later reverted (`database-only promotion reverted because authoritative champion gates remain unresolved`). File registry champions = `{}`.  
**ROOT CAUSE:** DB status was treated as authority.  
**FIX:** Authoritative file registry; promotion API requires it.  
**PROOF:** `verify-ledger` `artifact_champions` and `database_champions` are both empty; model truth role for that version is SHADOW / `MODEL_READY`.

### 4. Home P&L vs book P&L

**EXPECTED:** Sum of book nets = Home realized P&L.  
**ACTUAL (now):** Exact match at −477746296 lamports.  
**ROOT CAUSE (historical):** Mixed scopes and quarantined identity rows in totals.  
**FIX:** Verified-position SQL; quarantine exclusion.  
**PROOF:** Strategy sum = truth = verify-ledger (below).

### 5. Closed trades without Replay

**EXPECTED:** Every verified closed trade has a trace ID, or a precise incompleteness report.  
**ACTUAL:** Traces exist. Historical market snapshots and matured labels are often missing → `REPLAY_INCOMPLETE`.  
**ROOT CAUSE:** Snapshots were not retained for those IDs; labels are not linked at the requested virtual time.  
**FIX:** Trace API lists exact missing evidence. No fabricated tape.  
**PROOF:** Traces `01a0820d-b9a4-7763-ab65-4dd9604d38b9` and `01a0820c-002d-74d2-b942-1fcbf37e4d45`.

### 6. Truth `closed_trades` 251 vs verify-ledger 253 (earlier capture)

**EXPECTED:** Same closed-trade definition everywhere.  
**ACTUAL:** A 2-lot gap. Not a rounding error.  
**ROOT CAUSE:** Truth required a **verified position**. `verify-ledger` counted all fully consumed non-quarantined lots on active books, including lots whose **position** is quarantined.  
**FIX (this continuation):** Align verify-ledger to the truth CTE. Report the leftovers as `closed_trades_without_verified_position` (not a PASS failure).  
**PROOF:** Both leftover lots belong to Explorer position `01a08067-5c2f-79f1-8c1e-995dfd91b911`, mint `XRmXfJKcB9GLtbrCtMEtn42fFgmXVB8XT18ehcYpump`, reason `QUARANTINED_LOT_CONTAMINATION`:

- lot `01a08067-5c2e-7010-8b19-060721e9579e`
- lot `01a08068-0d81-7f91-960f-44b1fc6d2fcb`

After the align, snapshot `closed_trades=267` on Home/truth/strategies/verify-ledger, with `closed_trades_without_verified_position=2`.

### 7. Trade 503 while Home KPIs worked

**EXPECTED:** Both screens read the same report.  
**ACTUAL:** Home used `summary_only`; full Trade query 503’d because a LATERAL selected `entry.policy_version` without projecting `d.policy_version`.  
**FIX:** Project `d.policy_version` in `truth.rs`.  
**PROOF:** `/paper/truth?window=all&summary_only=true` and full Home both 200 at capture.

### 8. Champion decisions labelled as explorer

**EXPECTED:** Champion book decisions cite `CHAMPION_SHORT_HORIZON_V1`.  
**ACTUAL:** Historical rows still have `policy_version=INFORMATION_VALUE_EXPLORER_V1` (execution envelope reuse).  
**ROOT CAUSE:** `LearningPolicies::for_book` reused the explorer policy object; persist used that version string.  
**FIX:** New writes use `desk.book.key`; manifest carries both identities. History not rewritten.  
**PROOF:** Strategies report `policy_version=CHAMPION_SHORT_HORIZON_V1` and `execution_envelope_policy=INFORMATION_VALUE_EXPLORER_V1`; `policy_versions` on the champion item still lists the historical explorer string.

---

## Reconciliation (live snapshot)

Captured 2026-09-08T19:41:03Z. Source files: `artifacts/truth-audit/runtime/live-20260908-2038/digest.json`.

| Surface | Open | Closed positions | Closed trades (FIFO lots) | Realized P&L |
|---|---:|---:|---:|---|
| Home `/home` | 62 | 215 | 267 | −477746296 lamports = **−0.477746296 SOL** |
| Trade `/paper/truth?window=all&summary_only=true` | 62 | 215 | 267 | −477746296 |
| Strategies `/paper/strategies` (sum of 9 books) | 62 | — | 267 | −477746296 |
| `verify-ledger` | 62 | 215 | 267 | −477746296 |

Also equal: 9 books; 577 integrity-quarantined positions; 1656 integrity-quarantined orders; closed-today = 215 (all of today’s closed positions in this window). `closed_trade_basis=FULLY_FIFO_CONSUMED_ENTRY_LOT`. Unrealized P&L remains `UNKNOWN` (no complete executable mark series).

### Per-book P&L (Strategies)

| Book | N | Open | Net lamports | Sample | Edge | Authority |
|---|---:|---:|---:|---|---|---|
| CHAMPION_SHORT_HORIZON_V1 | 0 | 0 | 0 | INSUFFICIENT | INCONCLUSIVE | PAPER_ACTIVE |
| INFORMATION_VALUE_EXPLORER_V1 | 91 | 10 | −25456077 | EARLY | NEGATIVE | PAPER_ACTIVE |
| RANDOM_CONTROL_V1 | 7 | 7 | −946912 | INSUFFICIENT | INCONCLUSIVE | CONTROL |
| FULL_ML_SHADOW | 73 | 15 | −163380521 | EARLY | NEGATIVE | SHADOW |
| MARKET_ONLY_SHADOW | 71 | 17 | −174361695 | EARLY | NEGATIVE | SHADOW |
| HEURISTIC_GRAPH_AI_V1 | 10 | 0 | −31285491 | INSUFFICIENT | INCONCLUSIVE | PAPER_ACTIVE |
| RESEARCH_EXPLORATION_V1 | 8 | 7 | −5322780 | INSUFFICIENT | INCONCLUSIVE | PAPER_ACTIVE |
| HEURISTIC_V1 | 0 | 2 | 0 | INSUFFICIENT | INCONCLUSIVE | PAPER_ACTIVE |
| CONTROL_FLOW_V1 | 7 | 4 | −76992820 | INSUFFICIENT | INCONCLUSIVE | CONTROL |
| **Sum** | **267** | **62** | **−477746296** | | | |

Explorer FIFO net equals position net (`difference_quote_atomic=0`, `reconciled=true`). Fee breakdown on Explorer is `UNKNOWN_ASSET_OR_COMPONENT` (gross/fees null); that is disclosed, not invented.

Research consumes the same strategy reports. It ranks descriptive paper net P&L, states that books are not a matched experiment, and does not claim lift. With all nets ≤ 0 it says no profitable closed-book result.

---

## Champion loop

Book `01a08188-47cf-7583-a20a-b1eb25caf021` · ALPHA-LEARN · `CHAMPION_SHORT_HORIZON_V1`.

Ledger (funnel capture ~19:40Z): **1653 decisions, 0 buys, 0 orders, 0 fills, 0 positions**.

Independent gate counts on 332 instrumented snapshot evaluations (not a sequential conversion funnel):

| Gate | Passed | Failed |
|---|---:|---:|
| capacity | 332 | 0 |
| freshness | 310 | 22 |
| data_quality | 75 | 257 |
| technical | 141 | 191 |
| liquidity_and_impact | 141 | 191 |
| momentum | 0 | 332 |
| familiarity | 0 | 332 |
| adverse | 0 | 332 |
| prediction_available | 0 | 332 |
| decision_buy | 0 | 332 |

Dominant persisted reasons:

- Evaluation: `data_quality_blocked` (238), `champion_momentum_unavailable` (65), `market_data_stale` (22).
- Prediction: `technical_gate_not_passed` (191), `missing_champion:short_momentum_2m_v1` (101), `prediction_control_authority_unverified` (38).
- Two leftover rows still cite the old URL `http://127.0.0.1:8091/predict/shadow` from an earlier misconfig. Current env is `/predict` (ml-worker is **not** running).

The champion cannot trade until (1) an authoritative **file-registry** champion exists for `short_momentum_2m_v1`, (2) prediction serving is up, and (3) momentum / data-quality / technical / familiarity gates pass. Serving alone is not enough.

---

## Trade trace

**ID:** `01a0820d-b9a4-7763-ab65-4dd9604d38b9`  
Book RESEARCH_EXPLORATION_V1 (`01a07e6e-07c5-7893-b9cf-b3beceac0e8e`) · mint `j7UGFg5jUVFyUxRXGztng7Ka9VXigyM3ZCTw5cNpump` · integrity **VERIFIED**.

Lifecycle persisted (37 events): LAUNCH → FEATURE_SNAPSHOT → MODEL_PREDICTION → STRATEGY_DECISION → ORDER_CREATED → ARRIVAL_QUOTE → FILL (10_000_000 lamports in) → POSITION_OPENED → EXIT_DECISION (×6) → EXIT_FILL (9_596_965 lamports out) → POSITION_CLOSED → REALIZED_PNL **−403035 lamports**.

Second verified closed trade: `01a0820c-002d-74d2-b942-1fcbf37e4d45` (HEURISTIC_GRAPH_AI_V1, 65 events, realized **+2179634** lamports). Entry fill 50_000_000 in, exit 52_179_634 out.

---

## Replay

Both traces are **REPLAY_INCOMPLETE**.

Research trade missing:

- Historical market snapshots `01a0820c-7948-7b33-b447-931d5fc3587b`, `01a08228-60ed-7551-bbd8-9bbba5d63a1a`
- Executable snapshot / price overlay
- Certified tape continuity
- Matured label at the requested virtual time

Graph trade missing: seven historical snapshot IDs plus the same tape/label limitations.

Replay is an evidence reconstruction, not a re-simulation. Completeness failures are exact, not a blank screen.

---

## P&L audit

- Unit: SOL lamports, 9 decimals, mint `So11111111111111111111111111111111111111112`.
- Headline −0.477746296 SOL is entirely from verified positions.
- 577 quarantined positions / 1656 quarantined orders are excluded from that total (mostly historical `POOL_ALIAS_AS_MINT` launch identity plus lot/order mismatches). Source rows remain.
- Two fully consumed Explorer lots sit on a `QUARANTINED_LOT_CONTAMINATION` position and are counted only as `closed_trades_without_verified_position=2`.
- Shadow books (FULL_ML_SHADOW + MARKET_ONLY_SHADOW) are most of the loss (~−0.338 SOL). They are SHADOW authority, not champion results.
- Explorer: EARLY N=91, NEGATIVE −0.025456077 SOL, FIFO↔position reconciled. Fees not separately allocated (`UNKNOWN_ASSET_OR_COMPONENT`).
- Protocol/creator fees are embedded in fill prices (see research trace fee components). Network/priority fees are not modeled.
- Native SOL fee sentinel note remains in audit caveats: historical schedule label says 125bps; quote implementation applies 95+100bps. Historical records unchanged.

Independent check: sum of nine `net_pnl_quote_atomic` values = −477746296 = Home/truth/verify-ledger.

---

## Models

Authoritative registry: `artifacts/models/registry.json`.

| Target | Active role | Version | Live state | Held-out (n) | Promotion |
|---|---|---|---|---|---|
| short_momentum_2m_v1 | SHADOW | candidate-52f0c7cebb0dbb37 | MODEL_READY · 1496 preds · mean familiarity 0.62 | roc_auc 0.465 [0.43, 0.49] n=1102 | blocked: `minimum_matured_samples`, `dataset_marked_non_promotable` |
| adverse_move_2m_v1 | SHADOW | candidate-bf3a956e983bed89 | DATA_QUALITY_BLOCKED · 122 preds · missingness 0.66 | roc_auc 0.559 n=1102 | same + quality |
| expected_mfe_5m_v1 | SHADOW | candidate-2e685df8406b7ae0 | DATA_QUALITY_BLOCKED · 122 preds | roc_auc 0.488 n=1202 | same |
| exit_hazard_60s_v1 | SHADOW | shadow-06e30877ad02489d | DATA_QUALITY_BLOCKED · 8370 preds · familiarity 0.22 | roc_auc 0.557; PR-AUC 0.015 n=3955 | held-out metric gates fail |

No CHAMPION. Challengers exist as CANDIDATE rows with `INSUFFICIENT_LIVE_EVIDENCE`. `graduated_by_6h_v1` / `severe_collapse_1h_v1` / `up_50_before_down_20_15m_v1` have no registry artifact.

Strategy consumer of a champion prediction: **none** (`missing_champion:short_momentum_2m_v1`). Shadow versions are referenced on 1450 paper decisions (momentum/adverse/mfe) and 9331 (exit hazard) — observe-only.

---

## Health

After collector soak (process 77448):

- Headline: **SYSTEM RUNNING · TRADING READY**
- Layers: process RUNNING · data READY · trading READY
- `allow_new_entries=true`, idle ~0.8s, lag ~7ms, decoder `pump-events-identity-v2-2026-09-08`
- Earlier in this session, with collector down: **TRADING BLOCKED**, data UNAVAILABLE, `allow_new_entries=false`

Quarantine (15m window at readiness capture): 9443 / 10591 recent ≈ 89% rate, state DEGRADED. **0** unattributed market events, **0** other unknown events, **0** HOT markets affected, `NO_MATERIAL_HOT_IMPACT_DETECTED`. Causes are known PumpSwap discriminators with `pump_swap_identity_unresolved` on logs-only (non-hot) transactions. Lifetime integrity quarantines in Postgres: 2617 active rows; source preserved.

Hot-set upgrades during soak reached revision counts such as `hot_upgrades=10/10` (collector logs). Quote assets are not Hot Set members.

System UI splits process / data / trading and shows recent rate vs lifetime count vs HOT impact. That is the intended Health contract.

---

## Verification

| Check | Result |
|---|---|
| `pumphound-protocol-pump` | 8/8 (prior in this milestone) |
| `pumphound-protocol-pumpswap` | 5/5 |
| `pumphound-core-api` unit tests | 54 + paper_loop tests (prior) |
| `apps/desktop` `tsc --noEmit` | exit 0 |
| `./target/debug/verify-ledger` | **PASS**, all violation counts 0, frozen ALPHA-LOOP intact, champions `{}` |
| SQL orphan-lot query | 2 lots, both `QUARANTINED_LOT_CONTAMINATION` on one Explorer position |
| `GET /home` vs `GET /paper/truth` vs `GET /paper/strategies` vs verify-ledger | exact count and P&L match at 19:41:03Z |
| `GET /paper/traces/{id}` ×2 | VERIFIED lifecycle + REPLAY_INCOMPLETE |
| `GET /paper/champion-funnel` | 0 buys / 0 fills; gates as above |
| `GET /models/truth` | no champion; per-target states |
| `GET /api/v1/system/readiness` collector down → up | BLOCKED → READY |
| Desktop browser / Tauri journey | **not run** (RAM: ~70MB free Mach pages; Docker/Cursor already large). UI claims above are from source + API, not a click-through |

Commands used for the snapshot:

```bash
curl -sS http://127.0.0.1:8080/api/v1/home
curl -sS 'http://127.0.0.1:8080/api/v1/paper/truth?window=all&summary_only=true'
curl -sS http://127.0.0.1:8080/api/v1/paper/strategies
./target/debug/verify-ledger
```

---

## Remaining gaps

### BLOCKERS (for a champion that actually trades)

1. No champion in the **authoritative file registry**. Do not SQL-promote.
2. `pumphound-ml-worker` is not running (`8091`). Do not start it until RAM is free; xgboost/mlflow previously contributed to process deaths.
3. Even with serving, `prediction_available` / momentum / familiarity / data-quality still fail on the instrumented set.

### DEFERRED

- In-IDE / Tauri browser journey of Home → Trade → Trace → Replay → Research → System.
- PumpSwap logs-only identity (`pump_swap_identity_unresolved`) on non-hot pools — high quarantine *rate*, no measured HOT impact.
- Explorer (and some shadow) fee component allocation `UNKNOWN_ASSET_OR_COMPONENT`.
- Historical champion `decisions.policy_version` strings (left intact).
- Truth API still *can* return quarantined rows in the full `positions` array; UI filters them. Summary already excludes them from P&L.
- Snapshot/tape retention so Replay can reach `REPLAY_COMPLETE` on new trades.
- health `uptime_seconds` reflects the current API process, not collector uptime.

### TECHNICAL DEBT

- Two historical funnel rows with `/predict/shadow`.
- `GET /api/v1/strategies` is 404; canonical paths are `/paper/strategies` and `/research/strategies`.
- Full `/paper/truth` without `summary_only` is a multi-megabyte payload.
- Workspace has no git remote; baseline is a tarball.

---

## Next engineering objective

**Stand up ml-worker only after RAM is free, then measure a live champion funnel in which `prediction_available` is a real pass/fail — not “8091 down” — without promoting any model that is not in the authoritative file registry.**

Champion trading is blocked by **no file-registry champion**, not by the missing worker alone. The worker is the next *measurable* input; promotion remains a separate, evidence-gated act.

---

## Resources

- Disk at report time: ~53 GiB free on the data volume. No additional disk is required for this milestone.
- RAM/swap is the constraint. core-api (~26 MB RSS) and collector (~40 MB RSS) are the live processes. Do not start Tauri, a second API, or ml-worker in this session without freeing memory first.
