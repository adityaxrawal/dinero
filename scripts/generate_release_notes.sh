#!/usr/bin/env bash
# TASK-OPS-001: generates real release notes from commit history since the
# previous tag, replacing the static "See CHANGELOG for details." placeholder
# (no CHANGELOG file exists in this repo). Redacts any line that could
# plausibly be a leaked secret/token/credential or reference a crash log or
# user-specific path -- release notes are public, on the GitHub Release page.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

CURRENT_TAG="${1:?usage: generate_release_notes.sh <current-tag>}"
PREVIOUS_TAG=$(git tag --sort=-creatordate | grep -A1 "^${CURRENT_TAG}$" | tail -n1 || true)

if [ -z "$PREVIOUS_TAG" ] || [ "$PREVIOUS_TAG" = "$CURRENT_TAG" ]; then
  RANGE="$CURRENT_TAG"
  echo "## Dinero $CURRENT_TAG"
  echo ""
  echo "First tagged release."
else
  RANGE="${PREVIOUS_TAG}..${CURRENT_TAG}"
  echo "## Dinero $CURRENT_TAG"
  echo ""
  echo "Changes since $PREVIOUS_TAG:"
  echo ""
fi

# Subject lines only (not full commit bodies, which are more likely to
# contain incidental internal detail) -- then redact anything that looks
# like a leaked credential, a local file path, or a crash-log reference.
git log "$RANGE" --pretty=format:"- %s" 2>/dev/null \
  | grep -viE "api[_-]?key|secret|token|password|credential|\.log:|/Users/|crash.?report" \
  || echo "- Internal changes."

echo ""
echo "---"
if [ -f release-metadata.json ]; then
  BUILD_NUMBER=$(command grep -o '"build_number": *"[^"]*"' release-metadata.json | cut -d'"' -f4)
  GIT_COMMIT=$(command grep -o '"git_commit": *"[^"]*"' release-metadata.json | cut -d'"' -f4)
  echo "Build \`$BUILD_NUMBER\` from commit \`${GIT_COMMIT:0:12}\`."
fi
