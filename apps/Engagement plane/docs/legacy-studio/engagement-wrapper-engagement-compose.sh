#!/bin/sh
# Reuse the vetted per-core env/secret loading, overlays and persistent volumes.
set -eu
engagement_dir=$(CDPATH= cd "$(dirname "$0")/.." && pwd)
exec sh "$engagement_dir/../Studio Plane/scripts/studio-compose.sh" "$@"
