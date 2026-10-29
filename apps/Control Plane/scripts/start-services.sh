#!/usr/bin/env bash
set -euo pipefail

# Compatibility wrapper. The Control Plane root no longer has a shared .env;
# the service-local runner is the authoritative local Compose entry point.
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
exec "$root/scripts/run-control-plane.sh" up -d --build "$@"
