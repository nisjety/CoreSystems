#!/usr/bin/env bash
# Test for scripts/convex-image-staleness.sh (finding F10). Uses a stub `docker`
# on PATH and a throwaway convex-core directory; it never touches a real Docker
# daemon or the real convex-core files.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
HELPER="$ROOT_DIR/scripts/convex-image-staleness.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

mkdir -p "$work/root/convex-core" "$work/bin"
touch "$work/root/convex-core/startup.sh" "$work/root/convex-core/nats-subscriber.js"
# Baked files last changed 2026-10-30T12:00:00Z.
touch -d '2026-10-30T12:00:00Z' "$work/root/convex-core/startup.sh" "$work/root/convex-core/nats-subscriber.js"

# Stub docker: `inspect` answers from $STUB_INSPECT (an image ref, or empty to
# fail), `image inspect` from $STUB_CREATED (a timestamp, or empty to fail).
cat >"$work/bin/docker" <<'STUB'
#!/usr/bin/env bash
if [[ "$1" == "inspect" ]]; then
  [[ -n "${STUB_INSPECT:-}" ]] || exit 1
  printf '%s\n' "$STUB_INSPECT"
  exit 0
fi
if [[ "$1" == "image" && "$2" == "inspect" ]]; then
  [[ -n "${STUB_CREATED:-}" ]] || exit 1
  printf '%s\n' "$STUB_CREATED"
  exit 0
fi
exit 1
STUB
chmod +x "$work/bin/docker"

run_helper() { # run_helper -> sets OUT (stderr+stdout) and STATUS
  set +e
  OUT="$(PATH="$work/bin:$PATH" bash -c 'source "$1"; warn_if_convex_image_stale "$2"' _ "$HELPER" "$work/root" 2>&1)"
  STATUS=$?
  set -e
}

# 1. Image older than the baked files: warns, names the file and both services, exits 0.
export STUB_INSPECT="sha256:abc" STUB_CREATED="2026-10-29T08:00:00.123456789Z"
run_helper
[[ "$STATUS" == 0 ]] || fail "a stale image made the helper exit $STATUS"
[[ "$OUT" == *"WARNING"* ]] || fail "stale image did not warn: $OUT"
[[ "$OUT" == *"convex-gateway"* && "$OUT" == *"convex-subscriber"* ]] || fail "warning did not name both services: $OUT"
[[ "$OUT" == *"startup.sh"* || "$OUT" == *"nats-subscriber.js"* ]] || fail "warning did not name the baked file: $OUT"
[[ "$OUT" == *"build convex-gateway convex-subscriber"* ]] || fail "warning lacks the rebuild command: $OUT"

# 2. Image newer than the baked files: silent.
export STUB_CREATED="2026-10-31T08:00:00Z"
run_helper
[[ "$STATUS" == 0 && -z "$OUT" ]] || fail "a fresh image warned (exit $STATUS): $OUT"

# 3. Neither a container nor an image can be inspected: silent, exit 0.
export STUB_INSPECT="" STUB_CREATED=""
run_helper
[[ "$STATUS" == 0 && -z "$OUT" ]] || fail "unavailable docker state was not silent (exit $STATUS): $OUT"

# 4. No container, but the Compose image exists and is stale: still warns.
export STUB_INSPECT="" STUB_CREATED="2026-10-29T08:00:00Z"
cat >"$work/bin/docker" <<'STUB'
#!/usr/bin/env bash
if [[ "$1" == "inspect" ]]; then exit 1; fi
if [[ "$1" == "image" && "$2" == "inspect" ]]; then
  [[ "${*: -1}" == "application-plane-convex-gateway" || "${*: -1}" == "application-plane-convex-subscriber" ]] || exit 1
  printf '%s\n' "$STUB_CREATED"
  exit 0
fi
exit 1
STUB
run_helper
[[ "$STATUS" == 0 && "$OUT" == *"WARNING"* ]] || fail "stale Compose image (no container) did not warn: $OUT"

# 5. No docker at all: silent, exit 0.
set +e
OUT="$(PATH="/nonexistent-bin" /bin/bash -c 'source "$1"; warn_if_convex_image_stale "$2"' _ "$HELPER" "$work/root" 2>&1)"
STATUS=$?
set -e
[[ "$STATUS" == 0 && -z "$OUT" ]] || fail "missing docker was not silent (exit $STATUS): $OUT"

# 6. An unparseable timestamp or a missing baked file is silent too.
cat >"$work/bin/docker" <<'STUB'
#!/usr/bin/env bash
if [[ "$1" == "inspect" ]]; then printf 'sha256:abc\n'; exit 0; fi
printf 'not-a-date\n'
STUB
run_helper
[[ "$STATUS" == 0 && -z "$OUT" ]] || fail "unparseable timestamp was not silent (exit $STATUS): $OUT"
rm -f "$work/root/convex-core/startup.sh" "$work/root/convex-core/nats-subscriber.js"
run_helper
[[ "$STATUS" == 0 && -z "$OUT" ]] || fail "missing baked files were not silent (exit $STATUS): $OUT"

# 7. The runner sources the helper only for up/start/restart without a build.
runner="$ROOT_DIR/scripts/run-application-plane.sh"
grep -q 'convex-image-staleness.sh' "$runner" || fail "run-application-plane.sh does not source the helper"
grep -q '\*" build "\*|\*" --build "\*) ;;' "$runner" || fail "run-application-plane.sh does not skip the check when building"

echo "convex-image-staleness tests passed"
