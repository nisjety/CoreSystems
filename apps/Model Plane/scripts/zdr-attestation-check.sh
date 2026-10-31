#!/usr/bin/env bash
# Check the ZDR attestations in a runtime public policy before it is signed into
# a release candidate: every field present, digest recomputed, review-by not
# passed. Read-only; prints names and dates, never a credential.
#
#   zdr-attestation-check.sh <runtime-public-policy.env> [--today YYYY-MM-DD]
#
# The rules live in release-artifact.sh (the same function the release gate
# runs), so this cannot drift from what the gate accepts.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

usage() {
  echo "usage: zdr-attestation-check.sh <runtime-public-policy.env> [--today YYYY-MM-DD]" >&2
  exit 2
}

[[ $# == 1 || ( $# == 3 && "$2" == "--today" ) ]] || usage

exec "$SCRIPT_DIR/release-artifact.sh" check-zdr-attestation "$@"
