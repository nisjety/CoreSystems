#!/usr/bin/env bash
# Runs the Quarry-v2 workspace tests the way the Quarry CI job does (cargo test --workspace, CI=true, no database), inside Docker
# because Quarry builds only there. Builds the test image from deploy/compose/Dockerfile.test under a throwaway tag, runs the
# tests in a throwaway container with a throwaway cache volume (so a failed run can be rerun warm), then removes the container,
# the tag and the volume. The test exit code is the real cargo exit code (not tail's). usage: run_quarry_tests.sh <outfile>
set -uo pipefail
OUT="$1"
TAG="tmp-agent-quarry-test:local"
VOL="tmp-agent-quarry-test-target"
cd "C:/dev/CoresSystem/apps/Ingestion Plane/Quarry-v2"
export MSYS_NO_PATHCONV=1
echo "build start" > "$OUT"
docker build -f deploy/compose/Dockerfile.test -t "$TAG" . >> "$OUT" 2>&1 || { echo "BUILD FAILED" >> "$OUT"; echo "RESULT build-failed" >> "$OUT"; exit 2; }
echo "build done; test start" >> "$OUT"
docker run --rm -e CI=true -e CARGO_PROFILE_DEV_DEBUG=0 -e CARGO_INCREMENTAL=0 -v "$VOL:/src/target" "$TAG" 'export PATH=/usr/local/cargo/bin:$PATH; command -v cargo; cargo test --workspace --no-fail-fast > /tmp/test.log 2>&1; rc=$?; cat /tmp/test.log; exit $rc' > "$OUT.tests" 2>&1
rc=$?
echo "test exit $rc" >> "$OUT"
docker image rm "$TAG" >/dev/null 2>&1 && echo "image removed" >> "$OUT"
[ "$rc" = "0" ] && docker volume rm "$VOL" >/dev/null 2>&1 && echo "cache volume removed" >> "$OUT"
echo "RESULT exit-$rc" >> "$OUT"
exit $rc
