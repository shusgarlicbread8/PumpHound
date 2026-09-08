# PumpHound

Local-first Solana microcap intelligence terminal. Pump.fun bonding curves and PumpSwap pools, paper trading only.

## What it does

- Ingests live Pump.fun / PumpSwap transactions from Solana.
- Reconstructs point-in-time market state from official program events.
- Shows a dense research terminal (Radar, token tape, graph, paper desk).
- Runs realistic live paper trading. There is no live-money signer or submit path.
- Builds an evidence-preserving wallet/creator graph and optional calibrated models.

## Requirements

- macOS (Apple Silicon preferred)
- Docker Desktop
- Rust stable, Node 22+, pnpm, Python 3.12 via `uv`, `just`

## First run

```bash
cp .env.example .env
# Optional: set PUMPHOUND_HELIUS_API_KEY and PUMPHOUND_LASERSTREAM_ENTITLED=1
just dev
```

Without a Helius LaserStream entitlement the collector prints `LIVE MAINNET / RPC FALLBACK` and uses the configured Solana websocket/RPC.

## Operator commands

| Command | Meaning |
|---|---|
| `just dev` | stores + collector + API + desktop |
| `just capture-live` | stores + collector + API |
| `just stop` | graceful shutdown, keep data |
| `just inspect-launch <mint>` | print canonical snapshot |
| `just verify` | deterministic non-network suite |

## Architecture

See `PumpHound-Blueprint/` for the locked specification. Runtime code lives in `apps/`, `crates/`, `services/` and `ml/`.

Runtime mode is one of `LIVE_WATCH`, `LIVE_PAPER_HEURISTIC`,
`LIVE_PAPER_SHADOW_ML`, `LIVE_PAPER_ML`, or `REPLAY`; `RESEARCH` remains an
offline workflow. Every paper mode is simulated-only. There is no live execution mode.
