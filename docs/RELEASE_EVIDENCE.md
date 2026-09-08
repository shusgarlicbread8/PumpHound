# Release evidence

This file is the handoff report required by `acceptance/DEFINITION_OF_DONE.md`. It separates software that exists from checks that were actually run.

**Date:** 2026-09-07  
**Mode:** paper / watch / replay only. Real-money execution is absent.

## 1. What software works end to end

- Local Compose starts PostgreSQL, ClickHouse and NATS on loopback (`just deps`).
- `pumphound-collector` and `pumphound-core-api` compile and run against those stores.
- Official Pump / PumpSwap / fees IDLs are vendored with `SOURCE.lock` hashes.
- Domain, protocol, market-state, feature-core, paper-core and API contracts exist as Rust crates.
- Operator scripts, production Compose (private DB network), OpenAPI, fixtures and runbooks exist.
- Desktop source exists under `apps/desktop`. ML worker / investigator are packaging stubs.
- No screenshot evidence was captured in this operations pass.

There is **no** demonstrated live Radar screenshot or 24h soak in this tree.

## 2. What was checked, and where

| Check | Environment | Result |
|---|---|---|
| Workspace layout vs locked architecture | this repo | crates/services/db/apps/desktop present |
| Paper tables | `db/postgres/001_core.sql` | present; `002_paper.sql` not added |
| Fixture schema | `scripts/load-replay.sh` | JSON array of `RawTransaction` labelled synthetic |
| Deploy lint | `scripts/deploy-check.sh` | packaging constraints (loopback API, internal DBs) |
| Doctor | `scripts/doctor.sh` | disk/RAM/ports/deps/clock; secrets not printed |
| Definition of Done F01–F30 | — | **not claimed** |
| Performance table | — | **not measured** |

Run `just release-check` after install. Set `PUMPHOUND_RELEASE_RUN_TESTS=1` to include `cargo test --workspace`.

## 3. Unavailable dependencies / enrichments

- Helius LaserStream entitlement not configured by default.
- Birdeye, Jupiter, Pyth, DexScreener, SiliconFlow unused.
- Local LLM unused (investigator stub).
- No packaged `PumpHound.app` from this pass (`scripts/package-desktop.sh` is the path).

## 4. Strategy evidence

**None.** Models are untrained. Do not read heuristic rank as a probability or as a profitable system.

## 5. Start, stop, recover, reproduce

```bash
cp .env.example .env
bash scripts/doctor.sh
just deps
just capture-live          # or PUMPHOUND_MODE=REPLAY bash scripts/load-replay.sh
just stop
just backup
just restore-check data/exports/<day>
```

Production: `docker compose -f deploy/compose.production.yaml up -d` after setting `POSTGRES_PASSWORD` and `CLICKHOUSE_PASSWORD`. Connect the future desktop only over a private path.

## 6. No live signer

Application code must not accept seed phrases, load keypairs, sign or submit. Release-check greps for `sign_transaction` / `submit_bundle`. Quote-only Jupiter, if added later, stays unsigned.

## Expected `release/` layout (when a build exists)

```text
release/
  PumpHound.app or packaged installer
  RELEASE_EVIDENCE.md
  KNOWN_LIMITATIONS.md
  source-and-dependency-manifest.json
  metrics/performance.json
  metrics/strategy-validation.json
  screenshots/          # implemented product only, no mockups
  replay/manifests/
  backup/restore-check.json
```

No secrets or private notes in `release/`.
