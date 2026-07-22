#!/usr/bin/env bash
# TASK-OPS-001: fails closed if the embedded license public-key artifact is
# missing or malformed before a release build proceeds -- release.yml runs
# this before tauri-action so a broken/absent key never reaches a signed,
# notarized, publicly-distributed build. Run alongside
# security_hardening_check.sh, not merged into it: this check is about a
# single release-blocking artifact, not the broader hardening posture.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

KEY_PATH="src-tauri/keys/license_public.pem"

if [ ! -f "$KEY_PATH" ]; then
  echo "FAIL: embedded license public key is missing at $KEY_PATH" >&2
  exit 1
fi

if [ ! -s "$KEY_PATH" ]; then
  echo "FAIL: embedded license public key at $KEY_PATH is empty" >&2
  exit 1
fi

if ! openssl rsa -pubin -in "$KEY_PATH" -noout 2>/dev/null; then
  echo "FAIL: $KEY_PATH is not a valid RSA public key (openssl could not parse it)" >&2
  exit 1
fi

# The known placeholder key committed for local dev/test (per
# src-tauri/keys/README.md: "no real Licensing Backend counterpart... must
# be replaced before release"). Shipping this in a real, tagged release
# would mean every user's license JWT fails signature verification --
# functionally as broken as the key being missing outright, so this fails
# closed the same way. Detected by exact byte match: a real production key
# swapped in later naturally stops matching, so this check needs no update
# once the real key lands.
if [ -f "$SCRIPT_DIR/.placeholder_license_public.pem.sha256" ]; then
  EXPECTED_HASH=$(cat "$SCRIPT_DIR/.placeholder_license_public.pem.sha256")
  ACTUAL_HASH=$(shasum -a 256 "$KEY_PATH" | awk '{print $1}')
  if [ "$EXPECTED_HASH" = "$ACTUAL_HASH" ]; then
    echo "FAIL: $KEY_PATH is still the known placeholder dev key (see src-tauri/keys/README.md) -- replace with the real production key before release" >&2
    exit 1
  fi
fi

echo "OK: embedded license public key is present and valid: $KEY_PATH"

# TASK-OPS-001: release metadata (build number, git commit, embedded
# license-key version, notarization-ticket status placeholder filled in
# after the notarize step runs) -- so support can trace exactly which
# binary was distributed without needing any user financial or local-DB
# data (Doc 30's own wording).
BUILD_NUMBER="${GITHUB_RUN_NUMBER:-local-$(date +%s)}"
GIT_COMMIT="${GITHUB_SHA:-$(git rev-parse HEAD)}"
LICENSE_KEY_VERSION=$(shasum -a 256 "$KEY_PATH" | awk '{print $1}' | cut -c1-12)
GENERATED_AT=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

cat > release-metadata.json <<EOF
{
  "build_number": "$BUILD_NUMBER",
  "git_commit": "$GIT_COMMIT",
  "license_key_version": "$LICENSE_KEY_VERSION",
  "notarization_ticket_status": "pending",
  "generated_at": "$GENERATED_AT"
}
EOF
echo "Wrote release-metadata.json:"
cat release-metadata.json
