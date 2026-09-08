# PumpHound Definition of Done

This file is the final product acceptance contract. It is not a demand to build a large test framework before the GUI. Implement milestones 1–8 first with their narrow smoke evidence; execute the broad checks in milestone 9.

## 1. Functional acceptance

| ID | Requirement | Required evidence |
|---|---|---|
| F01 | live Pump launches and PumpSwap activity captured | real signatures/slots, stored counts, source lag |
| F02 | official event/account versions decoded | pinned IDLs + fixture comparison + quarantine behaviour |
| F03 | atomic, ordered, idempotent market reconstruction | replay including duplicate/reconnect/retraction scenarios |
| F04 | quote-asset-aware state and fees | SOL and USDC cases, integer quote/fee breakdown |
| F05 | Radar updates without refresh | actual app capture with new launch and stable filter |
| F06 | token chart/tape/curve/holders work | linked token screen and data/provenance inspection |
| F07 | evidence and metadata are safe/usable | source drawer, blocked hostile URI and neutral fallback |
| F08 | watchlists/alerts/settings persist | restart evidence, cooldown and acknowledgement |
| F09 | live manual paper buy and sell | GUI order → arrival → fill → position → exit |
| F10 | fees/latency/impact/failure modeled | scenario report, atomic slippage rejection and cost attribution |
| F11 | ledger/reservations/lots reconcile | zero reconciliation, overspend and lot violations |
| F12 | portfolio breakers and exits work | controlled trigger and audited reset; exits remain active |
| F13 | automated paper policies preserve decisions | accepted and abstained immutable records/version IDs |
| F14 | funding graph preserves evidence | bounded path, hub discount, source transfers |
| F15 | recurrent cohorts and lineage | repeated launch evidence and historical membership view |
| F16 | creator/wallet reputation is point-in-time | old cutoff excludes later outcome/label |
| F17 | graph UI is interactive and bounded | expand/filter/collapse/scrub/inspect scenario |
| F18 | features and labels are reproducible | dataset manifest, formulas, dependency hashes, censored labels |
| F19 | baselines and tree models train | native bundle + held-out/calibration reports |
| F20 | live predictions are immutable and versioned | saved prediction before policy decision; schema-reject case |
| F21 | model promotion/rollback is explicit | registry audit and replay preserving old versions |
| F22 | replay shares the live domain engine | same-seed deterministic result hash |
| F23 | strategy lab exposes fragility | costs/latency/top-winner/cluster/time-block sensitivity |
| F24 | optional AI is evidence-grounded | validated citations/counter-evidence/dissent |
| F25 | AI outage does not stop core product | disconnected-model scenario with continuing capture/paper |
| F26 | Mac app and remote CPU runtime are operable | exact launch/install commands and private connection |
| F27 | recovery and backup are real | restarted cursor, restored DB and reproduced run |
| F28 | no live signer/submission path | code/dependency/endpoint audit and rejected mode request |
| F29 | optional API failures are visible | per-provider degraded state, no fabricated numeric values |
| F30 | source/license/cost inventory is complete | notices, pinned source manifest, entitlement and spend display |

## 2. Performance acceptance

Targets are engineering objectives to measure on the user's actual Mac and selected feed, not claims about provider guarantees. Include hardware, captured workload, provider, region and sample duration in every report.

| Measurement | Initial target |
|---|---:|
| received canonical event → hot market state | p95 <250 ms under measured normal load |
| feature-ready → tabular prediction persisted | p95 <100 ms excluding queue/backfill |
| API snapshot response, warm cache | p95 <300 ms |
| new state → visible screen delta | p95 <500 ms after backend state update |
| sustained live source lag | target <2s; otherwise visible degradation and entry gate |
| Radar interaction | responsive at 10,000 rows through virtualization |
| graph initial render | ≤300 nodes/800 edges, responsive pan/select |
| deterministic replay | no dropped canonical records at 5× measured peak replay rate |
| long run | 24h soak without unbounded memory/queue growth |
| accounting | zero negative spendable balances/lot over-consumption |

If a target is missed, report measured values and affected functionality. Do not mark a slower but truthful data source as low-latency. Do not drop records to achieve a latency number.

## 3. Scientific acceptance versus software acceptance

The model UI is complete even if the model has no edge. It must honestly show:

- chronological held-out performance;
- class base rates and sample sizes;
- probability calibration;
- graph incremental value;
- prediction/abstention coverage;
- executable cost-adjusted outcomes;
- block/cluster-aware uncertainty;
- top-winner and regime sensitivity;
- fixed infrastructure costs separately from trading net P&L.

No universal trade-count threshold proves profitability. Sufficient evidence depends on dependence, tail risk, uncertainty, turnover, regimes and effect size. Thousands of observations are useful but are not a magic live-money authorization.

Promotion to model-driven **paper** mode may proceed with a clearly labelled research candidate. Promotion to real execution is outside this acceptance contract.

## 4. Critical invariants

- A duplicate network delivery never creates a second economic trade.
- A trade and its settlement transfer are not counted twice.
- A processed fork can be retracted without deleting raw history.
- A live decision sees only information actually available at that time.
- Historical replay uses virtual knowledge time, not today's labels.
- Unknown data never becomes a safe/zero-risk score.
- Monetary values do not transit through JavaScript binary floats.
- Every fill's debit/credit/fee/lot movements reconcile in its quote assets.
- Simulated arrival cannot precede decision and configured latency.
- Unsupported account/token/fee layout disables that quote, not raw collection.
- An LLM cannot change a prediction, policy, ledger or order.
- All real network calls remain read-only/quote-only; no signing/submission.

## 5. Release evidence layout

The coding agent delivers:

```text
release/
  PumpHound.app or packaged installer
  RELEASE_EVIDENCE.md
  KNOWN_LIMITATIONS.md
  source-and-dependency-manifest.json
  metrics/performance.json
  metrics/strategy-validation.json
  screenshots/
  replay/manifests/
  backup/restore-check.json
```

Do not include secrets or private user notes. Screenshots must be of the implemented product, not design mockups. Any fixture or synthetic scenario must be labelled as such; live evidence names its source/date range.

## 6. Final handoff statement

The release report states separately:

1. what software works end to end;
2. what was actually checked and on which environment;
3. what dependencies or enrichments remain unavailable;
4. whether strategy results are positive, negative or inconclusive;
5. how to start, stop, recover and reproduce a run;
6. that real-money execution is absent.

