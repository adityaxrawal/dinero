#!/usr/bin/env bash
# Doc 30 TASK-DESK-007: builds the app + .dmg (Tauri's own bundler,
# `bundle.targets: ["app", "dmg"]` in tauri.conf.json) and verifies the
# resulting DMG's structure -- the .app is present and the /Applications
# symlink is present at the positions `bundle.macOS.dmg`'s (unmodified,
# already-correct-by-default) appPosition/applicationFolderPosition
# schema defaults place them at. Distinct from `scripts/notarize.sh`
# (signing/notarization verification) and `scripts/release_notarize.sh`
# (the full local sign+notarize+staple orchestration) -- this script's
# job is just producing and structurally validating the DMG artifact
# itself, runnable without any Apple Developer credentials.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

SKIP_BUILD=false
if [[ "${1:-}" == "--skip-build" ]]; then
  # For re-verifying an already-built DMG without rebuilding (e.g. CI
  # re-running just this check after a prior job already built it).
  SKIP_BUILD=true
fi

if [ "$SKIP_BUILD" = false ]; then
  echo "== Building app + DMG =="
  pnpm tauri build
fi

BUNDLE_DIR="src-tauri/target/release/bundle"
if [ ! -d "$BUNDLE_DIR/dmg" ]; then
  # A universal-binary build (release.yml/release_notarize.sh) puts the
  # bundle under a target-triple-specific directory instead.
  BUNDLE_DIR="src-tauri/target/universal-apple-darwin/release/bundle"
fi

DMG_PATH=$(find "$BUNDLE_DIR/dmg" -name "*.dmg" 2>/dev/null | head -n1 || true)
if [ -z "$DMG_PATH" ]; then
  echo "error: no .dmg found under $BUNDLE_DIR/dmg — build failed or produced nothing" >&2
  exit 1
fi
echo "DMG produced: $DMG_PATH"

echo "== Verifying DMG structure =="
MOUNT_POINT=$(mktemp -d)
hdiutil attach "$DMG_PATH" -mountpoint "$MOUNT_POINT" -nobrowse -quiet

cleanup() {
  hdiutil detach "$MOUNT_POINT" -quiet 2>/dev/null || true
  rmdir "$MOUNT_POINT" 2>/dev/null || true
}
trap cleanup EXIT

APP_BUNDLE=$(find "$MOUNT_POINT" -maxdepth 1 -name "*.app" | head -n1 || true)
if [ -z "$APP_BUNDLE" ]; then
  echo "error: no .app bundle found inside the mounted DMG" >&2
  exit 1
fi
echo "  [OK] .app bundle present: $(basename "$APP_BUNDLE")"

if [ -L "$MOUNT_POINT/Applications" ]; then
  echo "  [OK] /Applications symlink present"
else
  echo "error: no /Applications symlink found inside the mounted DMG" >&2
  exit 1
fi

echo ""
echo "DMG structure verified: $DMG_PATH"
echo "NOTE: this only verifies structure (app + Applications symlink present)."
echo "It does not verify code signing/notarization (scripts/notarize.sh) or"
echo "first-launch Gatekeeper behavior on a clean machine, which remains a"
echo "manual release-checklist item -- see docs/release-checklist.md"
echo "(dmg_first_launch_clean_machine_verification)."
