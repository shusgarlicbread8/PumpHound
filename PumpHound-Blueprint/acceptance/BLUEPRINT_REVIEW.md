# Blueprint Handoff Review

This deliverable is an implementation specification, not a built or benchmarked PumpHound application.

## Coverage

The pack contains the product/design specification, process/storage choices, protocol/API inventory, wallet/creator/cohort graph, initial feature formulas and labels, model/research workflow, realistic paper accounting, optional AI investigator, operational/security requirements, six contract/configuration files, 32 ordered build tasks and final acceptance criteria.

The planning workflow was adapted to the requested product-first order: working live terminal and paper desk precede advanced graph/ML work; comprehensive verification is the final milestone. Narrow smoke evidence accompanies each functional slice so broken wiring does not accumulate.

## Checks performed on the blueprint

- Local Markdown file references resolve.
- Markdown code fences are balanced.
- Fenced JSON examples parse.
- Both supplied YAML configurations parse.
- Build tasks are numbered exactly 1 through 32 without gaps or duplication.
- Canonical action/mode/time/quantity terminology has been aligned across the specs, API and storage contracts.
- PostgreSQL and ClickHouse statements are separated by engine.
- Source notes distinguish checked interfaces, design choices, preprint findings and implementation-time verification.
- No unfinished plan markers remain.

## What is intentionally not claimed

- No application has been compiled, deployed or connected to a paid data account by creating this pack.
- The SQL files are baseline migration contracts; they have not been run against live PostgreSQL/ClickHouse servers in this planning session.
- Dependency minor versions, official IDL commits and model hashes must be pinned by the coding agent when downloaded. A source list is not a dependency lockfile.
- No provider subscription, GPU rental, account registration or real trade has been performed.
- No prediction accuracy, profitability, complete-network visibility or certain wallet identity is promised.

## Decided defaults

- Personal, local-first Apple Silicon terminal; actual RAM is detected.
- Pump.fun bonding curve and PumpSwap first; quote-aware SOL/USDC support gated by actual protocol state.
- Public/configured RPC path available for development; paid low-latency feeds require measured need and explicit spending approval.
- $1,000 research paper budget, $15 default order notional, configurable conservative limits.
- Tabular models first; local AI optional; GPU training only when justified.
- No signer or real-money submission in v1; future execution is separately specified and authorized.

Begin with [the coding-agent master prompt](../AGENT_MASTER_PROMPT.md) and [the master build sequence](../plans/00-master-build-sequence.md).

