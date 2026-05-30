#!/usr/bin/env bash
set -euo pipefail
CONCURRENCY=${1:-100}
DURATION=${2:-100}
BASE=${3:-http://localhost}
WARMUP=${4:-200}
END1="$BASE:8004/v1/retrieve"
PAY1='{"org_id":"smoke-test","query":"ocean covers earth","top_k":3}'
OUTDIR=/tmp/retrieval-stress-$(date +%s)
mkdir -p "$OUTDIR"

echo "Retrieval stress: concurrency=$CONCURRENCY duration=${DURATION}s warmup=$WARMUP -> logs: $OUTDIR"

# optional warmup
if [[ "$WARMUP" -gt 0 ]]; then
  echo "Warmup: sending $WARMUP retrieval requests" >> "$OUTDIR/run.log"
  : > "$OUTDIR/warmup.log"
  for i in $(seq 1 $WARMUP); do
    (curl -s -o /dev/null -w "%{http_code} %{time_total}\n" -X POST -H "Content-Type: application/json" -d "$PAY1" "$END1") >> "$OUTDIR/warmup.log" 2>/dev/null &
    if (( i % 50 == 0 )); then
      wait
      sleep 0.5
    fi
  done
  wait
  echo "Warmup done" >> "$OUTDIR/run.log"
  sleep 2
fi

END_AT=$((SECONDS + DURATION))
while [[ $SECONDS -lt $END_AT ]]; do
  echo "Wave at: $(date +%T)" >> "$OUTDIR/run.log"
  for i in $(seq 1 $CONCURRENCY); do
    (curl -s -o /dev/null -w "%{http_code} %{time_total}\n" -X POST -H "Content-Type: application/json" -d "$PAY1" "$END1") >> "$OUTDIR/retrieval.log" 2>/dev/null &
  done
  wait
done

# summarize
awk '{print $1" "$2}' "$OUTDIR/retrieval.log" | awk '{count++; if($1>=200 && $1<300) {sum+= $2; ok++; lat[ok]=$2} else {errs++}} END { if(ok>0){as=sum/ok; for(i=1;i<=ok;i++) a[i]=lat[i]; PROCINFO["sorted_in"]="@ind_num_asc"; if(ok>0) {median=a[int(ok/2)]; printf("Requests=%d OK=%d ERR=%d avg=%.3f\n", count, ok, errs, as)} } else {printf("Requests=%d OK=0 ERR=%d\n", count, errs)} }' > "$OUTDIR/summary.txt"
python3 - "$OUTDIR" >> "$OUTDIR/summary.txt" <<'PY'
import sys
p=sys.argv[1]
vals=[]
with open(p+'/retrieval.log') as f:
  for line in f:
    parts=line.strip().split()
    if len(parts)>=2 and parts[0].startswith('2'):
      try:
        vals.append(float(parts[1]))
      except:
        pass
if vals:
  vals.sort()
  n=len(vals)
  from statistics import mean
  print(f"n={n} avg={mean(vals):.3f} p50={vals[int(n*0.50)]:.3f} p90={vals[int(n*0.90)]:.3f} p95={vals[int(n*0.95)]:.3f} p99={vals[min(int(n*0.99),n-1)]:.3f}")
PY

cat "$OUTDIR/summary.txt"
echo "Logs: $OUTDIR"