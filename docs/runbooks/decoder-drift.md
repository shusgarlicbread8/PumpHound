# Runbook: decoder drift / unknown instruction

## Signal

- Rising `quarantined` count on collector or `/system/status`.
- Payloads `kind: decode.quarantined` with `discriminator_hex`.
- Startup assertion: vendored program ID ≠ `PUMPHOUND_PUMP_PROGRAM` / `PUMPHOUND_PUMPSWAP_PROGRAM`.
- Official IDL commit moved; `SOURCE.lock` hashes no longer match a fresh download.

## Safety

1. **Never guess** an unknown discriminator or account layout.
2. Quarantine the instruction; keep the raw sighting.
3. Disable quote/entry for affected markets (`incompatible_decoder_action: FREEZE_NEW_ENTRIES`). Collection of raw events continues.
4. Do not silently bump `decoder_version` in old rows.

## Recovery

1. Compare `vendor/pump-idl/SOURCE.lock` to upstream `pump-public-docs`.
2. If the IDL changed: vendor the new files, record URL, commit, retrieval date and SHA-256.
3. Run decoder conformance against `fixtures/chain/` plus any captured corpus. Unknown fixtures must still quarantine.
4. Bump `DECODER_VERSION` / lock notes. Display the new version on `/api/v1/health`.
5. Promote only after fixture comparison. Old quarantined rows stay quarantined unless a new decoder revision explicitly upgrades them with a new `revision`.

## Done when

- New known events decode to the same integer reserves as an independent decode of the official event bytes.
- Unknown bytes still quarantine.
- Health shows the new `decoder_version`.
- Paper quotes remain disabled on incompatible layouts.
