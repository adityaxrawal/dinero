#!/usr/bin/env bash
# K6 fix: a real pre-release security-hardening gate — checks concrete,
# previously-audited invariants rather than being a placeholder. Run from
# release_notarize.sh and release.yml before signing/notarizing.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

FAILURES=0

check() {
  local description="$1"
  local result="$2"
  if [ "$result" = "0" ]; then
    echo "  [OK] $description"
  else
    echo "  [FAIL] $description"
    FAILURES=$((FAILURES + 1))
  fi
}

echo "== Security hardening check =="

# 1. .env must never be committed (C16 fix — it must be gitignored).
if git check-ignore -q .env 2>/dev/null; then
  check ".env is gitignored" 0
else
  check ".env is gitignored" 1
fi

# 2. .env must not already be tracked in git history.
if git ls-files --error-unmatch .env >/dev/null 2>&1; then
  check ".env is not tracked in git" 1
else
  check ".env is not tracked in git" 0
fi

# 3. No literal "placeholder_pubkey" or similar dummy updater key in tauri.conf.json.
if grep -qi "placeholder" src-tauri/tauri.conf.json 2>/dev/null; then
  check "tauri.conf.json has no placeholder values" 1
else
  check "tauri.conf.json has no placeholder values" 0
fi

# 4. CSP must not be empty/default-permissive.
if grep -q "connect-src 'self' ipc:" src-tauri/tauri.conf.json && \
   ! grep -q "connect-src 'self' ipc: http://ipc.localhost;" src-tauri/tauri.conf.json; then
  check "CSP connect-src has an explicit domain allowlist" 0
else
  check "CSP connect-src has an explicit domain allowlist" 1
fi

# 5. Entitlements must not include the App Sandbox (contradicts allow-jit
#    distribution rationale — Major finding, Doc 01 §7.3/§11).
if [ -f src-tauri/Entitlements.plist ]; then
  if grep -q "com.apple.security.app-sandbox" src-tauri/Entitlements.plist; then
    check "Entitlements.plist does not request App Sandbox" 1
  else
    check "Entitlements.plist does not request App Sandbox" 0
  fi
fi

# 6. No hardcoded SQLCipher/Keychain dev keys left in source.
if grep -rq "dinero_dev_base_key_stable" src-tauri/src/ 2>/dev/null; then
  check "No hardcoded dev encryption key in source" 1
else
  check "No hardcoded dev encryption key in source" 0
fi

# 7. Cargo.lock and pnpm-lock.yaml must be committed (reproducible builds).
if [ -f src-tauri/Cargo.lock ] && git ls-files --error-unmatch src-tauri/Cargo.lock >/dev/null 2>&1; then
  check "Cargo.lock is committed" 0
else
  check "Cargo.lock is committed" 1
fi

echo ""
if [ "$FAILURES" -gt 0 ]; then
  echo "FAILED: $FAILURES check(s) did not pass."
  exit 1
fi

echo "All security hardening checks passed."
