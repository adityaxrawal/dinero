#!/usr/bin/env bash
# K6 fix: real notarization script using xcrun notarytool, called from
# release.yml after tauri-action produces the signed .app/.dmg. Requires
# APPLE_ID / APPLE_PASSWORD (app-specific password) / APPLE_TEAM_ID in the
# environment (the same secrets release.yml already passes to tauri-action).
set -euo pipefail

VERIFY_ONLY=false
if [[ "${1:-}" == "--verify-only" ]]; then
  VERIFY_ONLY=true
fi

BUNDLE_DIR="src-tauri/target/universal-apple-darwin/release/bundle"
DMG_PATH=$(find "$BUNDLE_DIR/dmg" -name "*.dmg" 2>/dev/null | head -n1 || true)

if [ -z "$DMG_PATH" ]; then
  echo "error: no .dmg found under $BUNDLE_DIR/dmg — build first" >&2
  exit 1
fi

if [ "$VERIFY_ONLY" = true ]; then
  echo "Verifying notarization + stapling for $DMG_PATH"
  spctl --assess --type open --context context:primary-signature -v "$DMG_PATH"
  xcrun stapler validate "$DMG_PATH"
  echo "OK: $DMG_PATH is notarized and stapled."
  exit 0
fi

: "${APPLE_ID:?APPLE_ID must be set}"
: "${APPLE_PASSWORD:?APPLE_PASSWORD must be set}"
: "${APPLE_TEAM_ID:?APPLE_TEAM_ID must be set}"

echo "Submitting $DMG_PATH for notarization..."
xcrun notarytool submit "$DMG_PATH" \
  --apple-id "$APPLE_ID" \
  --password "$APPLE_PASSWORD" \
  --team-id "$APPLE_TEAM_ID" \
  --wait

echo "Stapling notarization ticket to $DMG_PATH"
xcrun stapler staple "$DMG_PATH"

echo "Notarization complete: $DMG_PATH"
