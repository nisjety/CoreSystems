#!/usr/bin/env bash
# =============================================================
# pprof-profile.sh — Control Plane live profiling + load generation
#
# Usage:
#   ./pprof-profile.sh [duration_seconds]
#
# Prerequisites:
#   - brew install go hey (or: go install github.com/rakyll/hey@latest)
#   - docker compose up running with PPROF_ENABLED=true
#   - go tool pprof installed (comes with Go)
#
# Profiles collected (30s CPU each):
#   user-core   → localhost:6060
#   org-core    → localhost:6061
#   billing-core → localhost:6062
# =============================================================
set -euo pipefail

DURATION=${1:-30}
OUTPUT_DIR="./pprof-profiles/$(date +%Y%m%d_%H%M%S)"
mkdir -p "$OUTPUT_DIR"

INTERNAL_KEY="${INTERNAL_API_KEY:-dev-super-secret-internal-api-key}"

echo "═══════════════════════════════════════════════════"
echo "  Control Plane pprof — ${DURATION}s profile window"
echo "  Output: $OUTPUT_DIR"
echo "═══════════════════════════════════════════════════"

# ── Sanity check: confirm pprof endpoints are up ────────────────
for svc_port in "user-core:6060" "org-core:6061" "billing-core:6062"; do
  svc="${svc_port%%:*}"
  port="${svc_port##*:}"
  if ! curl -sf "http://localhost:${port}/debug/pprof/" > /dev/null 2>&1; then
    echo "⚠️  WARNING: $svc pprof not reachable at :$port — is PPROF_ENABLED=true and container running?"
  else
    echo "✅ $svc pprof reachable at :$port"
  fi
done

echo ""
echo "── Step 1/3: Generate realistic load ───────────────"
echo "   Running health + read endpoints for ${DURATION}s concurrently..."

# Generate load on all 3 services in parallel during the profile window
(
  # user-core: health + GET /api/v1/me/session-context (unauthenticated = fast path test)
  hey -z "${DURATION}s" -c 5 -q 20 \
      -H "X-Internal-Api-Key: ${INTERNAL_KEY}" \
      http://localhost:3012/health &

  # org-core: health + org listing
  hey -z "${DURATION}s" -c 5 -q 20 \
      -H "X-Internal-Api-Key: ${INTERNAL_KEY}" \
      http://localhost:8080/health &

  # billing-core: health
  hey -z "${DURATION}s" -c 5 -q 20 \
      -H "X-Internal-Api-Key: ${INTERNAL_KEY}" \
      http://localhost:3014/health &

  wait
) 2>&1 | grep -E "(Requests|Duration|Latency|Error|Slowest|Fastest)" || true

echo ""
echo "── Step 2/3: Capture pprof profiles ────────────────"

collect_profile() {
  local svc=$1
  local port=$2
  local kind=$3  # cpu | heap | goroutine | allocs | mutex | block
  local url="http://localhost:${port}/debug/pprof/${kind}"
  local outfile="${OUTPUT_DIR}/${svc}_${kind}.pprof"

  local extra=""
  if [[ "$kind" == "cpu" ]]; then
    extra="?seconds=${DURATION}"
    echo "  ⏳ Collecting ${DURATION}s CPU profile from $svc..."
  fi

  if curl -sf "${url}${extra}" -o "$outfile" 2>/dev/null; then
    local size
    size=$(du -sh "$outfile" 2>/dev/null | cut -f1)
    echo "  ✅ $svc $kind → $outfile ($size)"
  else
    echo "  ⚠️  $svc $kind: not available (service may lack recent traffic)"
  fi
}

# Collect CPU profiles (blocking for DURATION seconds each — run in parallel)
echo "  Collecting CPU profiles in parallel (${DURATION}s each)..."
collect_profile "user-core"    "6060" "cpu"      &
collect_profile "org-core"     "6061" "cpu"      &
collect_profile "billing-core" "6062" "cpu"      &
wait

# Collect instant-snapshot profiles
for kind in heap goroutine allocs mutex block; do
  collect_profile "user-core"    "6060" "$kind"
  collect_profile "org-core"     "6061" "$kind"
  collect_profile "billing-core" "6062" "$kind"
done

echo ""
echo "── Step 3/3: Quick text analysis ───────────────────"

for svc in user-core org-core billing-core; do
  cpu_file="${OUTPUT_DIR}/${svc}_cpu.pprof"
  heap_file="${OUTPUT_DIR}/${svc}_heap.pprof"

  if [[ -f "$cpu_file" ]]; then
    echo ""
    echo "▶ ${svc} — CPU top-10:"
    go tool pprof -top -nodecount=10 "$cpu_file" 2>/dev/null | head -20 || echo "  (no CPU data — service may have been idle)"
  fi

  if [[ -f "$heap_file" ]]; then
    echo ""
    echo "▶ ${svc} — Heap top-10 (inuse_space):"
    go tool pprof -top -nodecount=10 -inuse_space "$heap_file" 2>/dev/null | head -20 || echo "  (no heap data)"
  fi
done

echo ""
echo "═══════════════════════════════════════════════════"
echo "  Raw profiles saved to: $OUTPUT_DIR"
echo ""
echo "  To explore interactively:"
echo "    go tool pprof -http=:8081 $OUTPUT_DIR/user-core_cpu.pprof"
echo "    go tool pprof -http=:8082 $OUTPUT_DIR/org-core_cpu.pprof"
echo "    go tool pprof -http=:8083 $OUTPUT_DIR/billing-core_cpu.pprof"
echo ""
echo "  To view goroutine traces:"
echo "    go tool pprof -http=:8084 $OUTPUT_DIR/user-core_goroutine.pprof"
echo "═══════════════════════════════════════════════════"
