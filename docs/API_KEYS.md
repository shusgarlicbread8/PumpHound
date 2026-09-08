# API keys and entitlements

Keys are **secret references**, not webview configuration. Desktop (when present) stores them in the OS credential store. Servers inject them from a root-readable environment file or secret manager. `.env` is gitignored. `.env.example` has no production credentials.

Doctor reports only **present / absent**.

## Required for some kind of chain read

| Name | Required? | Failure behaviour |
|---|---|---|
| Solana RPC / WebSocket | Yes (public fallback allowed) | Live watch with visible lag/coverage limits |
| `PUMPHOUND_HELIUS_API_KEY` | Recommended | Without it: `LIVE MAINNET / RPC FALLBACK` |
| `PUMPHOUND_LASERSTREAM_ENTITLED=1` | Only if the Helius plan actually includes LaserStream | Do not set this to 1 to “make the GUI look fast” |

Helius Developer / Business prices change; confirm the dashboard. Plan cards are not a substitute for entitlement.

## Optional enrichment

| Name | Product | If absent |
|---|---|---|
| `PUMPHOUND_BIRDEYE_KEY` | Independent price/cross-check | Degrade Birdeye; no fabricated quotes |
| `PUMPHOUND_JUPITER_KEY` | Quote-only `/swap/v2` style responses | Skip Jupiter quotes |
| `PUMPHOUND_PYTH_KEY` | SOL/USD reference | Quote-native values or another reference |
| DexScreener | Public limits | Cross-check only, never launch truth |

Jupiter calls, if added, are **quote/build inspection only**. Payloads are never signed or submitted.

## Optional investigator

| Name | Notes |
|---|---|
| `PUMPHOUND_AI_PROVIDER` | Explicitly `local`, `siliconflow`, or `none` (safe default) |
| `PUMPHOUND_AI_BASE_URL` / `PUMPHOUND_AI_MODEL` | Local OpenAI-compatible endpoint and model; used only with `local` |
| `PUMPHOUND_AI_KEY` | Optional local endpoint key |
| `PUMPHOUND_SILICONFLOW_BASE_URL` | Defaults to `https://api.siliconflow.com/v1` |
| `PUMPHOUND_SILICONFLOW_MODEL` | Configurable; current recommendation is `deepseek-ai/DeepSeek-V4-Flash` |
| `PUMPHOUND_SILICONFLOW_API_KEY` | SiliconFlow secret; never expose it to desktop or logs |

The local base URL may remain configured while SiliconFlow is selected; provider selection, not variable presence, controls routing. SiliconFlow outages degrade to a deterministic advisory and cannot stop capture or alter paper policy.

Run one low-token connectivity request without printing the key:

```sh
PUMPHOUND_AI_PROVIDER=siliconflow \
PUMPHOUND_SILICONFLOW_API_KEY='your-key-from-your-secret-store' \
PUMPHOUND_SILICONFLOW_MODEL='deepseek-ai/DeepSeek-V4-Flash' \
python services/investigator/smoke_siliconflow.py
```

## Store references

`PUMPHOUND_POSTGRES_DSN`, `PUMPHOUND_CLICKHOUSE_*`, `PUMPHOUND_NATS_URL` and their `*_REF` counterparts. Production Compose overrides DSNs to Docker DNS names and still reads passwords from the environment, not from git.

## Rotation

Separate development and production keys. Least scope. After any paste into chat, logs or a screenshot: rotate, record the incident, follow `docs/runbooks/key-rotation.md`.

## What you must never configure

- Seed phrases, keypair files, remote signers, Jito bundle auth for submission.
- Production database credentials on Vast.ai or any disposable GPU.
