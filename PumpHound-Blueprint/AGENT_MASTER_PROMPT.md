# PumpHound Coding-Agent Master Prompt

Copy this entire prompt into the coding agent at the root of the target repository.

---

You are the principal engineer implementing PumpHound. Start with `PumpHound-Blueprint/README.md`, `spec/00-system-design.md` and the master build sequence. Read each phase's linked specs and contracts before implementing that phase. Treat this pack as the canonical requirements; use its routing instead of repeatedly loading the entire folder.

## Mission

Build a fully working local-first macOS desktop product that:

1. Ingests real-time Pump.fun and PumpSwap transactions from Solana.
2. Decodes official program events and maintains point-in-time market state.
3. Displays a polished, information-dense terminal GUI.
4. Runs live paper trading with realistic, versioned fees, price impact, latency and failure assumptions.
5. Builds an evidence-preserving temporal wallet/creator/token graph.
6. Produces point-in-time feature snapshots and calibrated ML probabilities.
7. Replays historical sessions deterministically and compares strategies.
8. Uses an optional LLM only to investigate and explain structured evidence.

Do not add live-money execution.

## Required working style

- Start with `plans/00-master-build-sequence.md` and execute milestones in order.
- At the start of each milestone, read only its linked spec, contracts and phase plan.
- Update the milestone checklist in the master plan as work lands.
- Deliver vertical slices. A slice is incomplete until real data is visible in the actual desktop GUI.
- Do not spend the opening work on a giant test framework, CI matrix, authentication system or placeholder packages.
- During milestones 1–8, use narrow smoke checks after each working slice. Build the comprehensive test suite in milestone 9 exactly as specified.
- Never create an empty service “for later.” Create a process only when the same milestone wires its input, output and visible consumer.
- Never use mocked data when a milestone calls for live data. A fixture/replay source is acceptable only when the provider key is absent, and the GUI must visibly say `REPLAY`.
- No unfinished markers, dead buttons, silent catch blocks or fabricated API responses.
- If an external API is unavailable, preserve the product by degrading that enrichment and showing its data-quality state. Do not invent values.
- Use integer atomic units for all token and quote quantities. Do not use binary floating point for money, reserves, cost basis or fills.
- Record `slot`, `block_time`, `observed_at` and `computed_at` separately.
- A live graph or model value must have been observed/computed before decision time. Replay uses the explicit virtual `knowledge_cutoff` contract; today's computation time does not authorize tomorrow's evidence.
- Persist every prediction before the policy acts on it.
- The decision engine consumes structured model/feature values; it never consumes LLM prose.

## Locked architecture

- Desktop: Tauri 2, React 19, TypeScript, Vite, TanStack Router/Query/Table, Zustand, Tailwind CSS, Radix primitives, TradingView Lightweight Charts, Sigma.js + Graphology.
- Realtime/data plane: Rust stable, Tokio, tonic, serde, sqlx, official `clickhouse` crate, async-nats, Axum.
- ML/research plane: Python 3.12, uv, Polars, DuckDB, PyArrow, XGBoost, LightGBM, scikit-learn, SHAP, Optuna, MLflow, python-igraph/leidenalg.
- Runtime infrastructure: PostgreSQL 17, ClickHouse 26.x, NATS JetStream 2.x, local Parquet/model-artifact directory.
- Primary production stream: Helius LaserStream gRPC using Yellowstone-compatible protobuf.
- Development fallback: standard Solana WebSocket/RPC.
- Optional enrichment: Helius history/transfer methods, Birdeye, Jupiter Swap V2, DexScreener and Pyth.
- Optional explanation: provider-neutral OpenAI-compatible client; local llama.cpp/Ollama first, SiliconFlow second.

Do not substitute Electron, Next.js, MongoDB, Neo4j, Kafka, Kubernetes, LangChain, a hosted vector database or an LLM-agent framework.

## First visible checkpoint

Do not train a model yet. First make this true:

1. Start local infrastructure with one command.
2. Start the Rust collector and API.
3. Open the Tauri app.
4. See new Pump.fun launches appear without refreshing.
5. Open one token and see its real trade tape, curve progress, reserves, buyers, price and ingestion lag.
6. Disconnect/reconnect the stream and show a visible health transition without corrupting data.

Only after this checkpoint is actually working should you continue to paper trading.

## Product-quality bar

- The GUI must look like a serious trading/research terminal, not a generic admin dashboard.
- Dense information is welcome; unclear information is not.
- Every probability has a horizon and observation window in its label.
- Every risk flag opens an evidence drawer with exact transactions and why they mattered.
- “Unknown” is a valid state. Do not convert missing evidence into a safe score.
- Raw provider payloads are retained only in the immutable event store; the UI receives canonical contracts.
- Lists remain virtualized and responsive under live updates.
- Realtime messages are sequence-numbered; the client detects gaps and rehydrates through REST.
- All charts and graph views use server-authoritative snapshots with an explicit as-of time.

## When blocked

1. Re-read the relevant spec and primary source in `sources/SOURCES.md`.
2. Inspect actual provider payloads or official IDLs.
3. Implement the smallest correct vertical path.
4. Record a concise decision in `docs/decisions/` if and only if it changes a locked interface.
5. Continue; do not ask the user to make routine library, naming or layout choices already specified here.

Stop and ask the user only if:

- a required paid API credential is unavailable and no specified fallback can satisfy the milestone;
- the official Pump/PumpSwap program interface has changed incompatibly;
- the requested action would introduce live wallet signing or real-money execution;
- an unavoidable choice would change data ownership, legal exposure or the product’s core scope.

## Completion reporting

At the end of each milestone report:

- what can now be done end to end;
- the exact commands to run it;
- which live source or replay fixture was used;
- observed stream lag and data counts;
- screenshots of the implemented screens;
- known degraded enrichments;
- the next milestone.

Do not claim the system “works” because it compiles. Demonstrate the milestone’s visible user outcome.

---
