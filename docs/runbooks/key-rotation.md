# Runbook: key rotation / leaked credential

## Signal

- Key pasted into chat, screenshot, log, ticket or git history.
- Provider email about leaked token.
- Unexpected spend versus `PUMPHOUND_MONTHLY_PROVIDER_BUDGET_USD`.
- `.env` committed (release-check must fail).

## Safety

1. Treat the key as burned. Do not “hope nobody saw it.”
2. Do not commit the replacement. Do not put it in `RELEASE_EVIDENCE.md`.
3. Browser/webview must still receive **zero** provider keys.

## Recovery

1. Revoke the leaked key at the provider (Helius, Birdeye, Jupiter, Pyth, SiliconFlow, AI host).
2. Issue a new key with the least scope (read / quote only). Separate dev vs prod.
3. Update the OS credential store or server secret file. Config keeps only `*_REF`.
4. `git log` / `git rev-list` for the secret; if it was committed, rotate **and** rewrite is not done here — follow your org’s secret-scrub process. Never `--force` to main from this product script.
5. Restart collector/API so old env is gone. Confirm logs redact authorization headers.
6. Write an audit_log row: actor, action `key_rotation`, reason, no secret values.

## Done when

- Old key 401s at the provider.
- New key present (doctor says `set`, does not print it).
- Incident time, provider, and scope recorded.
