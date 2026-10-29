#!/usr/bin/env bash
set -euo pipefail
CONCURRENCY=${1:-100}
DURATION=${2:-30}
BASE=${3:-http://localhost}
# Optional: number of warmup requests to prime caches before the main load
# Pass as 4th arg; default=200 (set to 0 to disable warmup)
WARMUP_COUNT=${4:-200}
END1="$BASE:8004/v1/retrieve"
PAY1='{"org_id":"smoke-test","query":"ocean covers earth","top_k":3}'
END2="$BASE:8101/api/v1/reason/batch"
PAY2='{"requests":[{"query":"What is 2 plus 2?","strategy":"chain_of_thought"}]}'
OUTDIR=/tmp/coresystem-load-$(date +%s)
mkdir -p "$OUTDIR"
echo "Running load smoke: concurrency=$CONCURRENCY duration=${DURATION}s -> logs: $OUTDIR"
# Warmup: wait for reasoning service health before launching load waves
echo "Warmup: waiting for reasoning health (up to 15s)" >> "$OUTDIR/run.log"
HEALTH_OK=0
for i in $(seq 1 15); do
  code=$(curl -s -o /dev/null -w "%{http_code}" -m 2 http://localhost:8101/health || echo "000")
  if [[ "$code" == "200" ]]; then
    HEALTH_OK=1
    break
  fi
  sleep 1
done
if [[ "$HEALTH_OK" -ne 1 ]]; then
  echo "Warmup: health check failed; sleeping 5s before starting" >> "$OUTDIR/run.log"
  sleep 5
else
  echo "Warmup: health ok" >> "$OUTDIR/run.log"
  sleep 2
fi

# Cache warmup: issue a set of lightweight reasoning (and occasional retrieval)
# requests to prime LLM caches, embeddings and retrieval caches. This helps
# reduce rare cold misses during the main smoke run.
if [[ "$WARMUP_COUNT" -gt 0 ]]; then
  echo "Cache warmup: running $WARMUP_COUNT requests" >> "$OUTDIR/run.log"
  WARMUP_LOG="$OUTDIR/warmup.log"
  : > "$WARMUP_LOG"
  for i in $(seq 1 $WARMUP_COUNT); do
    # every 5th request also hit retrieval to warm embeddings/index
    if (( i % 5 == 0 )); then
      (curl -s -o /dev/null -w "%{http_code} %{time_total}\n" -X POST -H "Content-Type: application/json" -d "$PAY1" "$END1" ) >> "$WARMUP_LOG" 2>/dev/null &
    fi
    (curl -s -o /dev/null -w "%{http_code} %{time_total}\n" -X POST -H "Content-Type: application/json" -d "$PAY2" "$END2" ) >> "$WARMUP_LOG" 2>/dev/null &
    # throttle small batches to avoid overloading startup
    if (( i % 20 == 0 )); then
      wait
      sleep 1
    fi
  done
  wait
  echo "Cache warmup completed" >> "$OUTDIR/run.log"
  # small pause to allow background workers to settle
  sleep 2
fi
END_AT=$((SECONDS + DURATION))
while [[ $SECONDS -lt $END_AT ]]; do
  echo "Wave at: $(date +%T)" >> "$OUTDIR/run.log"
  # Launch retrieval requests
  for i in $(seq 1 $CONCURRENCY); do
    (curl -s -o /dev/null -w "%{http_code} %{time_total}\n" -X POST -H "Content-Type: application/json" -d "$PAY1" "$END1" ) >> "$OUTDIR/retrieval.log" 2>/dev/null &
  done
  # Launch reasoning requests
  for i in $(seq 1 $CONCURRENCY); do
    (curl -s -o /dev/null -w "%{http_code} %{time_total}\n" -X POST -H "Content-Type: application/json" -d "$PAY2" "$END2" ) >> "$OUTDIR/reasoning.log" 2>/dev/null &
  done
  wait
done
# Summarize
echo "Retrieval results:" > "$OUTDIR/summary.txt"
awk '{print $1" "$2}' "$OUTDIR/retrieval.log" | awk '{count++; if($1>=200 && $1<300) {sum+= $2; ok++; lat[ok]=$2} else {errs++}} END { if(ok>0){as=sum/ok; for(i=1;i<=ok;i++) a[i]=lat[i]; PROCINFO["sorted_in"]="@ind_num_asc"; if(ok>0) {median=a[int(ok/2)]; printf("Requests=%d OK=%d ERR=%d avg=%.3f\n", count, ok, errs, as)} } else {printf("Requests=%d OK=0 ERR=%d\n", count, errs)} }' >> "$OUTDIR/summary.txt"
echo "Reasoning results:" >> "$OUTDIR/summary.txt"
awk '{print $1" "$2}' "$OUTDIR/reasoning.log" | awk '{count++; if($1>=200 && $1<300) {sum+= $2; ok++; lat[ok]=$2} else {errs++}} END { if(ok>0){as=sum/ok; for(i=1;i<=ok;i++) a[i]=lat[i]; PROCINFO["sorted_in"]="@ind_num_asc"; if(ok>0) {median=a[int(ok/2)]; printf("Requests=%d OK=%d ERR=%d avg=%.3f\n", count, ok, errs, as)} } else {printf("Requests=%d OK=0 ERR=%d\n", count, errs)} }' >> "$OUTDIR/summary.txt"
cat "$OUTDIR/summary.txt"
echo "Logs and raw results are in $OUTDIR"
