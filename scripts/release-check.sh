#!/usr/bin/env bash
# Honest release gate. Does not claim Definition of Done rows that were not run.

set -euo pipefail
. "$(cd "$(dirname "$0")" && pwd)/lib.sh"

errors=0
note_fail() { fail "$1"; errors=$((errors + 1)); }

echo "release-check"

bash "$ROOT/scripts/doctor.sh" || note_fail "doctor"
bash "$ROOT/scripts/deploy-check.sh" || note_fail "deploy-check"

echo "required documents"
for f in \
  "$ROOT/docs/USER_GUIDE.md" \
  "$ROOT/docs/OPERATIONS.md" \
  "$ROOT/docs/API_KEYS.md" \
  "$ROOT/docs/DATA_DICTIONARY.md" \
  "$ROOT/docs/KNOWN_LIMITATIONS.md" \
  "$ROOT/docs/RELEASE_EVIDENCE.md" \
  "$ROOT/docs/THIRD_PARTY_NOTICES.md" \
  "$ROOT/docs/BUILD_STATUS.md" \
  "$ROOT/schemas/openapi/pumphound-v1.yaml" \
  "$ROOT/fixtures/chain/manifest.json" \
  "$ROOT/fixtures/replay/manifest.json"; do
  [[ -f "$f" ]] && ok "$(basename "$f")" || note_fail "missing $f"
done

echo "fixture labels"
if grep -Eqi 'not live mainnet|SYNTHETIC' "$ROOT/fixtures/chain/manifest.json"; then
  ok "chain fixtures labelled synthetic"
else
  note_fail "chain fixture manifest missing synthetic label"
fi

echo "no-wallet boundary (paths only)"
if git -C "$ROOT" grep -nE 'sign_transaction|submitBundle|submit_bundle|SecretKey|private_key' -- \
  ':!PumpHound-Blueprint' ':!docs' ':!tests' ':!scripts' ':!fixtures' ':!schemas' >/tmp/ph-rel 2>/dev/null; then
  if grep -qiE 'sign_transaction|submit_bundle|submitBundle' /tmp/ph-rel; then
    note_fail "possible live execution symbol"
    redact </tmp/ph-rel | head -n 20
  else
    ok "no sign/submit symbols"
  fi
else
  ok "no sign/submit symbols"
fi
rm -f /tmp/ph-rel

echo "secrets not committed"
if [[ -f "$ROOT/.env" ]] && git -C "$ROOT" ls-files --error-unmatch .env >/dev/null 2>&1; then
  note_fail ".env is tracked"
else
  ok ".env is not tracked"
fi

if [[ "${PUMPHOUND_RELEASE_RUN_TESTS:-0}" == "1" ]]; then
  echo "deterministic suite (requested)"
  (cd "$ROOT" && cargo test --workspace --exclude pumphound-desktop) || note_fail "cargo test"
else
  warn "cargo test skipped (set PUMPHOUND_RELEASE_RUN_TESTS=1 to run)"
fi

if [[ -d "$ROOT/apps/desktop" ]]; then
  ok "desktop tree present"
else
  warn "apps/desktop absent — F05/F06/F26 desktop evidence cannot pass"
fi

echo
echo "acceptance: this script does not mark F01–F30 complete."
echo "See docs/RELEASE_EVIDENCE.md and docs/BUILD_STATUS.md for what was actually checked."
if (( errors > 0 )); then
  echo "release-check FAILED errors=$errors"
  exit 1
fi
echo "release-check PASSED (packaging/docs/boundary only)"
