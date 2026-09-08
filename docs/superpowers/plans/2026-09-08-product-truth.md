# PumpHound Product Truth Implementation Plan

**Goal:** Reconcile persisted paper activity across product screens and make historical evidence inspectable.
**Architecture:** Preserve existing tables and durable IDs; simulation_run_id is book_id, decision_id is candidate/decision trace root, position_id identifies position traces. Read projections join persisted facts rather than synthesizing events. Existing frozen experiment and policies remain unchanged.
**Tech Stack:** Rust/Axum/SQLx/Postgres, React/TypeScript, Python validation.
**Spec:** docs/PRODUCT_TRUTH_SPEC.md

## Global Constraints
- PAPER ONLY. No signing or live execution.
- ALPHA-LOOP-20260908 frozen; no historical edits.
- Unknown evidence is explicit; reconstruction never grants future knowledge.
- Active book scope: simulation_runs.state='running', portfolio not archived, manifest has strategy.
- Existing files preserved in artifacts/truth-audit/baseline/source.tar.gz; workspace has no Git repository.

## Tasks and ownership
- [x] A. Root: capture runtime/database baseline, define contract and shared truth API; add ledger scope/trace/activity projections and tests.
- [x] B. UI worker: Trade default all active books, filters and counters, position trace/replay journey; target state presentation, Home links, Research executive layer, Health presentation. Own apps/desktop/src only. (Source complete; in-app click-through not run — RAM.)
- [x] C. Model worker: reproduce champion gate failures, instrument durable measurable funnel, target availability/model lineage, tests. Own paper_loop.rs, model_client.rs, new model_truth.rs; coordinate route wiring with root.
- [x] D. Ledger worker: repair strategy metrics exact book identity, sample/edge/authority, financial attribution, Home scope, health quarantine/readiness. Own paper_ledger.rs strategy_reports section, cockpit.rs, readiness.rs and new ledger_metrics.rs if needed.
- [x] E. Root: integrate, run Rust/Python/TypeScript suites, restart API safely, inspect real API and browser journey, write evidence/completeness report. (API/ledger evidence in docs/PRODUCT_TRUTH_EVIDENCE.md; browser journey deferred.)

## Shared interfaces
Root provides GET /api/v1/paper/truth?book_id=&experiment_id=&strategy=&state=&window=all|today returning scope, summary, books, positions, orders, activity. Summary counts and P&L use identical selected book/window scope. Positions include position_id, trade_trace_id=position_id, book_id, experiment_id, strategy_version, policy_version, launch_id, market, status and explicit atomic/mint/decimal money.
Root provides GET /api/v1/paper/traces/{id}?at=ISO_TIMESTAMP accepting position_id or decision_id; returns identity, events, completeness with exact missing evidence, temporal status; no replay job required. Replay reconstructs persisted evidence only.
Model worker provides model_truth public functions and sends exact signatures to root for wiring.
Ledger worker exports canonical active-book SQL through shared predicate documented above; Home portfolio_slice calls root truth summary once interface exists.

## Verification
Add executable fixture invariants for boundaries and live read-only SQL/API verifier for exact book scope, counts/P&L, order/fill/position/feature/model joins, frozen artifact hashes. Run current tests first where reproduction exists; add regression tests before each repair. Capture multiple real closed-book traces and precise missing-evidence results. No completion claim without current runtime and browser evidence.
