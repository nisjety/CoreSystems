#!/bin/sh
# Canonical root compose, restricted to Quarry-v2 + Integration Core and their
# local infrastructure. Reuses the existing protected provider projection.
set -eu
root=$(CDPATH= cd "$(dirname "$0")/.." && pwd)
export INTEGRATION_LOCAL_CANONICAL=1
exec sh "$root/../Engagement plane/scripts/integration-local.sh" "$@"
