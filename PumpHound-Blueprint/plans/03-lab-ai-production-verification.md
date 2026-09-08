# Research Lab, AI and Production Verification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finish replay/research, evidence-grounded AI, reliable always-on operation and the comprehensive verification suite after the product works end to end.

**Architecture:** Historical replay drives the existing domain reducers and paper engine through a virtual clock. AI is a read-only asynchronous consumer. Production packaging preserves the same process boundaries and adds recovery, private connectivity and observable operations.

**Tech Stack:** Rust, Python, Tauri/React, Parquet, MLflow, OpenAI-compatible local/SiliconFlow adapters, OpenTelemetry, Prometheus, Docker Compose.

**Spec:** [Paper/replay](../spec/06-paper-trading-and-replay.md), [AI](../spec/07-ai-investigator.md), [operations](../spec/08-security-operations-and-deployment.md), [acceptance](../acceptance/DEFINITION_OF_DONE.md).

## Global Constraints

- Live and replay share deterministic domain reducers and paper quote math.
- V1 contains no transaction signer or live-money submission path.
- The LLM is optional and cannot change ledger, model or policy state.
- Comprehensive verification begins at task 29, after product milestones 1–8.
- Software completion does not imply profitable trading performance.

---

### Task 21 — Replay engine and timeline UI

**Files:** `crates/chain-source/src/replay.rs`, `crates/paper-core/src/replay.rs`, `services/core-api/src/routes/replays.rs`, `ml/pumphound_ml/replay/{runner,manifest}.py`, `apps/desktop/src/features/replay/{ReplayPage,ReplayControls,ReplayTimeline}.tsx`.

**Consumes:** immutable raw/normalized partitions, pinned config/model/policy versions and deterministic seed.

**Produces:** `run_replay(manifest_path: str) -> ReplayReport`, replay run API and synchronized timeline UI.

- [ ] Validate manifest hashes, time ranges, ordering quality and required model/decoder artifacts before starting.
- [ ] Drive the same reducer and paper engine with a virtual clock; disable real-time wall-clock reads inside domain logic.
- [ ] Implement pause/resume/speed/single-event/single-slot step and checkpoint resume.
- [ ] Distinguish original-observation replay from chain-time-only history and from impact-overlay scenarios.
- [ ] Persist decisions, fills, labels, marks and summary under a new immutable simulation run ID.
- [ ] Render Radar/token/graph/paper screens against the replay clock with a persistent `REPLAY` banner.

`ReplayReport` fields: `manifest_sha256`, `deterministic_result_sha256`, `decision_count`, `fill_count`, `ledger_balanced`, `reservation_violations`, `lot_violations`, `future_dependency_count`, `summary_uri`. The deterministic hash excludes wall-clock job start time and randomly generated presentation IDs.

**Smoke evidence:** run one saved session twice with the same seed and compare deterministic result hash; stepping pauses market/paper state together.

### Task 22 — Strategy comparisons and robustness reports

**Files:** `ml/pumphound_ml/evaluation/{policies,costs,bootstrap,concentration,capacity}.py`, `apps/desktop/src/features/replay/{RunComparison,CostSensitivity,OutlierSensitivity}.tsx`.

**Consumes:** multiple runs over the same manifest and held-out interval.

**Produces:** comparable net/gross strategy reports with uncertainty and failure sensitivity.

- [ ] Compare base-rate/random eligible, momentum, graduation heuristic, wallet-score heuristic and model policy under identical costs/capital/risk constraints.
- [ ] Sweep latency, fees, slippage, transaction-failure rate, maximum order/depth fraction and signal thresholds.
- [ ] Report effect of removing top three, top five and top 1% winning trades; do not hide negative results.
- [ ] Bootstrap by time block and cohort family, not individual correlated trades only.
- [ ] Separate per-trade return sums from bankroll equity return; annualization is disabled on very short samples or labelled speculative.
- [ ] Export machine-readable JSON/Parquet plus a human-readable HTML report with all manifest/version links.

**Smoke evidence:** increasing friction worsens the relevant cost-adjusted comparison; report shows identical sample periods and distinguishes bankroll return from sum of trade returns.

### Task 23 — Research datasets, drift and model lifecycle

**Files:** `ml/pumphound_ml/evaluation/{drift,champion_challenger}.py`, `services/core-api/src/routes/research.rs`, `apps/desktop/src/features/models/{DatasetPanel,DriftPanel,PromotionDialog}.tsx`.

**Consumes:** mature live shadow labels, registry and dataset manifests.

**Produces:** ongoing research workflow with explicit promotion decisions.

- [ ] Add dataset inventory, quality/coverage progress, class balance, mature/pending label counts and split visualization.
- [ ] Track feature missingness/distribution drift, calibration drift and policy net-edge decay by window/regime.
- [ ] Offer manual or configured scheduled retraining jobs; challenger models remain shadow-only until explicitly promoted.
- [ ] Quarantine models when features/schema/provider coverage change incompatibly.
- [ ] Keep a champion rollback button and preserve the model used for each historical decision.

**Milestone 6 outcome:** a research session can be reproduced, compared and rejected on evidence without changing the live record.

### Task 24 — Local/cloud AI gateway and evidence packs

**Files:** `services/investigator/{main,providers,tools,evidence,validation,cache}.py`, `services/investigator/prompts/{quant,wallet,creator,risk,contrarian,committee}.md`, `schemas/json/investigation-response-v1.json`.

**Consumes:** read-only evidence APIs, chosen model provider and fixed as-of cutoff.

**Produces:** validated role responses tied to an evidence-pack hash.

- [ ] Implement one OpenAI-compatible HTTP adapter with configurable base URL/model ID and server-side secret references.
- [ ] Support a local llama.cpp/Ollama endpoint, optional SiliconFlow and deterministic no-model summary.
- [ ] Expose only the typed read-only tools listed in the investigator spec; enforce cutoff/size/rate limits in gateway code.
- [ ] Freeze evidence pack before role execution; prompt profiles receive facts and model results, not arbitrary database access.
- [ ] Validate structured response, cited evidence IDs, numeric transcription and permitted stance; mark unsupported narrative unverified.
- [ ] Cache by evidence/prompt/model hash, enforce time/token/spend limits and degrade cleanly on timeout.

**Smoke evidence:** local investigation contains valid clickable citations; provider disconnection leaves market/graph/paper functionality intact.

### Task 25 — Investigator workbench and dissent

**Files:** `apps/desktop/src/features/token/{InvestigationPanel,ClaimCard,AgentDissent,EvidenceCitation}.tsx`, `services/core-api/src/routes/investigations.rs`.

**Consumes:** investigation jobs and validated role outputs.

**Produces:** optional human-readable decision support, never an order side effect.

- [ ] Render observed/derived/predicted/interpreted sections separately.
- [ ] Run specialist roles independently, then Contrarian, then Synthesizer; pass structured results only.
- [ ] Show counter-evidence, gaps and disagreements next to the summary.
- [ ] Link each claim to evidence drawer and frozen as-of time.
- [ ] Allow the user to open a paper ticket manually from the token context; a memo itself never places or modifies an order.

**Milestone 7 outcome:** the user can ask why a launch/cluster is interesting or risky and audit every supported claim.

### Task 26 — Configuration, health and reliable remote runtime

**Files:** `deploy/compose.production.yaml`, `deploy/collector.Dockerfile`, `deploy/core-api.Dockerfile`, `deploy/ml-worker.Dockerfile`, `scripts/{doctor,start,stop,deploy-check}.sh`, `docs/runbooks/{provider-loss,decoder-drift,queue-lag}.md`, `apps/desktop/src/features/settings/ConnectionProfile.tsx`.

**Consumes:** working services and validated configuration/credential references.

**Produces:** local startup diagnostics and privately reachable always-on CPU deployment.

- [ ] Implement doctor checks for disk/RAM/ports/dependencies/clock/provider entitlements without dumping secrets.
- [ ] Pin compatible dependency/container versions and capture them in release metadata.
- [ ] Bind databases/JetStream privately, add resource limits/health checks and graceful shutdown deadlines.
- [ ] Add remote connection profile over a private authenticated channel; never expose raw DB/NATS ports.
- [ ] Implement retry, dead-letter inspection and gap backfill runbooks with UI-visible status transitions.
- [ ] Measure provider RTT, jitter and stream lag in candidate regions; select location from measurements rather than GPU bandwidth claims.

**Smoke evidence:** desktop reads a remote live paper session through the private channel; restarting collector resumes capture without duplicated economic events.

### Task 27 — Observability, backups and retention

**Files:** `deploy/observability/{prometheus.yaml,otel-collector.yaml}`, `scripts/{backup,restore-check,export-partitions}.sh`, `docs/runbooks/{restore,disk-pressure,ledger-divergence,model-rollback}.md`.

**Consumes:** service metrics/logs, databases and artifact manifests.

**Produces:** operational visibility, recoverable data and reproducible exports.

- [ ] Add the exact metric families and correlation IDs from the operations spec.
- [ ] Implement daily partition exports with counts/hashes and backup manifests; keep exports independent of database retention.
- [ ] Provide isolated restore with schema/data/hash comparison and a resumed replay.
- [ ] Add storage-pressure thresholds; shed AI/enrichment before core capture and freeze new paper entries when persistence is threatened.
- [ ] Expose backup age, restore-check result, usage budget and data retention settings in System.

**Smoke evidence:** restore a backup into an isolated database/artifact directory and reproduce one historical run.

### Task 28 — Desktop packaging and operating documentation

**Files:** `apps/desktop/src-tauri/tauri.conf.json`, `scripts/package-desktop.sh`, `docs/{USER_GUIDE,OPERATIONS,API_KEYS,DATA_DICTIONARY}.md`, `docs/runbooks/key-rotation.md`.

**Consumes:** finished screens, configuration and runtime commands.

**Produces:** runnable Mac application and concrete operating guide.

- [ ] Package a local macOS application with explicit connection/startup settings; do not require a distribution-signing account for personal development.
- [ ] Document first start, provider configuration, live/replay distinction, paper portfolio funding, investigation, replay and model promotion.
- [ ] Document known limitations: partial history, inferred intent, simulation counterfactual limits and no live execution.
- [ ] Include third-party notices, model-license records and API data-use restrictions.
- [ ] Expose version, decoder hash, model/policy versions and support export with secret redaction.

**Milestone 8 outcome:** all requested product areas are built and operable. Now perform the broad verification below.

### Task 29 — Protocol, time and deterministic regression suite

**Files:** `crates/protocol-pump/tests/fixtures.rs`, `crates/protocol-pumpswap/tests/fixtures.rs`, `crates/market-state/tests/reconcile.rs`, `tests/python/test_replay_contract.py`, `fixtures/chain/manifest.json`, `fixtures/replay/manifest.json`.

**Consumes:** actual captured transactions and completed domain/replay implementation.

**Produces:** decoder, ordering, idempotency, fork and availability regression evidence.

- [ ] Create a documented fixture corpus for each instruction/event/account version supported, including SOL/USDC, migration, virtual quote reserves, loaded addresses and failed swaps.
- [ ] Compare decoded output and integer quote arithmetic against pinned official reference implementations and independently decoded event/account observations.
- [ ] Assert duplicate sightings do not duplicate trades; interrupted batch publication resumes safely; retraction restores the canonical branch.
- [ ] Assert features never reference future knowledge and fallback ordering does not claim exactness when unresolved.
- [ ] Run deterministic replay twice and compare content-derived output, not wall-clock presentation metadata.

```python
from pumphound_ml.replay.runner import run_replay

def test_replay_is_deterministic():
    a = run_replay("fixtures/replay/manifest.json")
    b = run_replay("fixtures/replay/manifest.json")
    assert a.deterministic_result_sha256 == b.deterministic_result_sha256
    assert a.future_dependency_count == 0
```

Run: `cargo test -p protocol-pump -p protocol-pumpswap -p market-state` and `uv run pytest tests/python/test_replay_contract.py -q`.

### Task 30 — Paper accounting, costs and policy verification

**Files:** `crates/paper-core/tests/{quotes,ledger,latency,risk,exits}.rs`, `tests/python/test_paper_reconciliation.py`.

**Consumes:** completed paper engine and controlled replay fixtures.

**Produces:** evidence for correct cash/fees/lots/reservations and realistic outcomes.

- [ ] Exercise exact rounding, overflow, decimals, SOL settlement, USDC, token-account rent and unsupported token extensions.
- [ ] Assert gross input = reserve input + included fees; network/priority costs appear exactly once.
- [ ] Exercise slippage rejection, no-liquidity, stale quote, landed failure, dropped transaction and concurrent order reservations.
- [ ] Assert no atomic partial fills, no negative spendable cash and no lot over-consumption.
- [ ] Exercise every breaker and exit precedence; disabling entries does not disable open-position exits.
- [ ] Confirm fork corrections create new records/runs without deleting original decisions/fills.

```python
from pumphound_ml.replay.runner import run_replay

def test_paper_invariants():
    report = run_replay("fixtures/replay/manifest.json")
    assert report.ledger_balanced
    assert report.reservation_violations == 0
    assert report.lot_violations == 0
```

Run: `cargo test -p paper-core` and `uv run pytest tests/python/test_paper_reconciliation.py -q`.

### Task 31 — Desktop, integration, load and AI verification

**Files:** `apps/desktop/e2e/{live-terminal,paper,graph,replay,accessibility}.spec.ts`, `tests/integration/{provider-outage,stream-resume,metadata-security}.ts`, `tests/python/test_investigator_grounding.py`, `scripts/load-replay.sh`.

**Consumes:** built app/API, deterministic fixture environment and optional read-only live credentials.

**Produces:** screen recordings/screenshots, integration results and measured performance report.

- [ ] Use Playwright against the same React frontend in its local development host, and run packaged Tauri smoke checks for native startup/keychain/notifications.
- [ ] Verify Radar → token → evidence → paper buy/sell, graph time scrub, model card and replay controls.
- [ ] Exercise websocket disconnect, expired cursor, epoch change, slow client, provider outage and metadata failures.
- [ ] Check keyboard focus, colour-independent risk indicators, 1280×800 and 1920×1080 layouts, and reduced motion.
- [ ] Replay at 1×, 5× and 10× measured peak load; record p50/p95/p99 lag, queue/disk growth, CPU/RAM and dropped-record count.
- [ ] Exercise malicious metadata instructions, fake citations and unsupported numbers against the investigator; no ungrounded claim affects a policy.

Run: `pnpm --filter @pumphound/desktop exec playwright test`, `uv run pytest tests/python/test_investigator_grounding.py -q`, and `just verify-live` only when read-only credentials are configured.

### Task 32 — Release gate and honest handoff

**Files:** `docs/RELEASE_EVIDENCE.md`, `docs/KNOWN_LIMITATIONS.md`, `docs/BUILD_STATUS.md`, `scripts/release-check.sh`.

**Consumes:** all milestone outputs and verification evidence.

**Produces:** packaged product plus a truthful completion report.

- [ ] Run formatting/type/build/security/dependency checks and the full deterministic suite.
- [ ] Match every acceptance row to a command, screenshot, artifact or measured result.
- [ ] Verify a clean installation/start and a backup restore; list exact platform/dependency versions.
- [ ] Separate software readiness from strategy evidence; show unprofitable or inconclusive research results plainly.
- [ ] Confirm no private-key/sign/send/submit path exists and provider permissions are read-only where configurable.
- [ ] Deliver the app, run commands, source/version manifest, evidence report, known limitations and operational guide.

No live-money stage begins automatically after this gate. It requires a separate explicit user request and a separate execution/security specification.

