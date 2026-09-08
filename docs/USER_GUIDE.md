# PumpHound user guide

Local-first Solana microcap research terminal. **Paper and replay only.** There is no wallet, signer or live-money path.

## What you can do today

The Rust collector, core API and Tauri desktop source exist. PostgreSQL, ClickHouse and NATS start from Compose. You can start stores, run capture or replay, call `/api/v1` on loopback, and open the desktop with `just dev` once dependencies are installed.

## First start

```bash
cp .env.example .env
# Optional: set PUMPHOUND_HELIUS_API_KEY and PUMPHOUND_LASERSTREAM_ENTITLED=1
bash scripts/doctor.sh
just dev          # stores + collector + API + desktop
# or
bash scripts/start.sh
```

Without a Helius LaserStream entitlement the collector prints `LIVE MAINNET / RPC FALLBACK`.

Replay (synthetic fixture, not mainnet):

```bash
export PUMPHOUND_MODE=REPLAY
bash scripts/load-replay.sh
```

The UI, when it exists, must show a persistent `REPLAY` banner for that mode. Do not present fixture tokens as live launches.

## Modes

| Mode | Meaning |
|---|---|
| `LIVE_WATCH` | Ingest and display. No paper orders. |
| `LIVE_PAPER_HEURISTIC` | `HEURISTIC_V1` controls simulated entries and exits without requiring models. |
| `LIVE_PAPER_SHADOW_ML` | `HEURISTIC_V1` controls simulated trades; available ML outputs are comparison-only shadow records. |
| `LIVE_PAPER_ML` | All four champions plus the compiled ML policy control simulated trades. |
| `REPLAY` | Virtual clock over a pinned manifest. |
| `RESEARCH` | Offline datasets and training. |

There is no live execution mode. A request asking for one returns `422 UNSUPPORTED_EXECUTION_MODE`.

## Paper portfolios

Paper cash is simulated. Funding lots are not a deposit to an exchange. Quotes use integer atomic units. Models are untrained: the API reports `MODEL NOT TRAINED` and heuristic ranks are labelled `HEURISTIC`, never a percentage probability.

The checked-in heuristic policy is `configs/heuristic-v1.json`. It is loaded from
`PUMPHOUND_HEURISTIC_POLICY_PATH`, validated before use, and records its threshold rationale.
These are conservative paper-operation defaults, not a claimed edge.

Run the heuristic paper mode without changing `.env`:

```bash
PUMPHOUND_MODE=LIVE_PAPER_HEURISTIC \
PUMPHOUND_HEURISTIC_POLICY_PATH=configs/heuristic-v1.json \
cargo run -p pumphound-core-api --bin pumphound-core-api
```

All paper modes use simulated capital and arrival-state quotes. None can sign or submit a
transaction.

## Investigations

An optional LLM may only explain **structured evidence** you already stored. It cannot change a prediction, policy, ledger or order. Inferred graph links are **not ownership**.

Optional Helius enrichment is candidate-only, disabled by default, and never gates
`HEURISTIC_V1`. See `docs/CANDIDATE_ENRICHMENT.md`.

## Stop and recover

```bash
just stop
# or
bash scripts/stop.sh
```

Journals and checkpoints stay on disk. See `docs/OPERATIONS.md` and `docs/runbooks/`.

## Safety

- Never paste a seed phrase or private key. The product rejects that input by design.
- Token metadata and linked sites are hostile. Images must be proxied; remote HTML is not rendered.
- Third-party notices and data-use limits: `docs/THIRD_PARTY_NOTICES.md`.
