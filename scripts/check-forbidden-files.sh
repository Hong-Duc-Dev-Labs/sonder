#!/usr/bin/env bash
# Fails when tracked files include third-party filter tables or decrypted material.
# These files must never enter git or a release bundle.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

offenders=$(git ls-files | grep -Ei \
  -e '(^|/)polyphase_[^/]*$' \
  -e '(^|/)[^/]*filters?[^/]*/(.*/)?[^/]*\.csv$' \
  -e '(^|/)filters_manifest\.json$' \
  -e '\.decrypted$' || true)

if [ -n "$offenders" ]; then
  echo "Forbidden files are tracked by git:" >&2
  echo "$offenders" >&2
  exit 1
fi

# The ignore rules themselves must keep blocking the canonical example.
git check-ignore -q polyphase_2x_linear_phase_precise.csv || {
  echo ".gitignore no longer ignores polyphase_* tables" >&2
  exit 1
}

echo "No forbidden files tracked."
