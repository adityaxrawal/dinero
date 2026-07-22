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

# 8. TASK-OPS-008: hardened runtime + notarization must remain a mandatory
#    gate, not an optional/skippable step -- Apple's notarization service
#    itself refuses to notarize a binary that isn't built with the hardened
#    runtime, so a real `notarize.sh --verify-only` call in release.yml is
#    the practical, testable proxy for "hardened runtime was actually on"
#    (there is no signed .app bundle to inspect with `codesign` in this
#    static, no-build check).
if grep -q "notarize.sh --verify-only" .github/workflows/release.yml 2>/dev/null; then
  check "release.yml has a mandatory post-build notarization verification step" 0
else
  check "release.yml has a mandatory post-build notarization verification step" 1
fi

# 9. TASK-OPS-008: no disallowed entitlements or DYLD-injection flags.
#    `com.apple.security.cs.disable-library-validation` would allow
#    arbitrary unsigned dylibs (including a malicious DYLD_INSERT_LIBRARIES
#    payload) to load into this signed binary; `com.apple.security.get-task-allow`
#    would let another process attach a debugger to it in production.
if [ -f src-tauri/Entitlements.plist ]; then
  if grep -q "com.apple.security.cs.disable-library-validation\|com.apple.security.get-task-allow" src-tauri/Entitlements.plist; then
    check "Entitlements.plist has no disallowed entitlements (library-validation-disable / get-task-allow)" 1
  else
    check "Entitlements.plist has no disallowed entitlements (library-validation-disable / get-task-allow)" 0
  fi
fi
if grep -rq "DYLD_INSERT_LIBRARIES" src-tauri/src/ 2>/dev/null; then
  check "no DYLD_INSERT_LIBRARIES reference anywhere in source" 1
else
  check "no DYLD_INSERT_LIBRARIES reference anywhere in source" 0
fi

# 10. TASK-OPS-008: Licensing Backend / desktop-app privacy-boundary
#     separation -- the Licensing Backend must never import a Gmail/OAuth
#     or transaction-processing module (it has no business touching either),
#     and vice versa the desktop app's reconciliation/analytics layer must
#     never import a licensing-only secret constant.
if [ -d licensing-backend/api ] && [ -d licensing-backend/lib ]; then
  # Specific desktop-domain terms only -- deliberately not the bare word
  # "reconciliation" (the Licensing Backend has its own, unrelated
  # billing/subscription reconciliation job, `jobs/billing_reconciliation.ts`)
  # or "gmail" (a doc comment in errors.ts legitimately *mentions* GMAIL_*
  # error codes to say they don't apply here).
  if command grep -rEq "googleapis\.com/gmail|reconciliation_clusters|reconciliation_engine|transaction_observations|OAuth2Client" licensing-backend/api licensing-backend/lib 2>/dev/null; then
    check "Licensing Backend never imports Gmail/OAuth/transaction-processing modules" 1
  else
    check "Licensing Backend never imports Gmail/OAuth/transaction-processing modules" 0
  fi
else
  check "Licensing Backend never imports Gmail/OAuth/transaction-processing modules (skipped: licensing-backend/ not present in this checkout)" 0
fi
if command grep -rq "JWT_PRIVATE_KEY_PEM\|RAZORPAY_KEY_SECRET" src-tauri/src/reconciliation/ src-tauri/src/extraction/ 2>/dev/null; then
  check "desktop reconciliation/extraction layer never references licensing-only secrets" 1
else
  check "desktop reconciliation/extraction layer never references licensing-only secrets" 0
fi

echo ""
if [ "$FAILURES" -gt 0 ]; then
  echo "FAILED: $FAILURES check(s) did not pass."
  exit 1
fi

echo "All security hardening checks passed."
