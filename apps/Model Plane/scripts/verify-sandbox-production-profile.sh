#!/usr/bin/env bash
# verify-sandbox-production-profile.sh — WS06-S10. Reached through
#   ./scripts/verify-sandbox-isolation.sh --production-profile [options]
#
# Verifies the resource confinement of execution-core (F-t1-021) under the PRODUCTION
# container profile, not under `--privileged`: read-only root, tmpfs /tmp 512m, cap_drop
# ALL, no-new-privileges, the scoped seccomp profile, and the container limits
# (mem_limit, cpus, pids_limit, nofile) READ FROM the compose files so the script cannot
# drift from them. Inside that container it runs the exact argv shape execution-core
# emits for a sandboxed spawn:
#
#   prlimit --cpu=<s>:<s+2> --as=<B> --fsize=<B> --nofile=<n> --core=0 -- bwrap <argv> -- <cmd>
#
# and throws five floods at it: fork bomb, memory bomb, CPU spin, file flood (and a tmpfs
# fill), output flood. After each, the liveness canary (`/healthz`) must answer and the
# container must still be running. It also measures what the limits cost real work
# (numpy, pandas, matplotlib, openpyxl, LibreOffice under several `--as` ceilings) and
# prints CALIB lines: those are the numbers to record in the PR description and to feed
# back into EXECUTION_SANDBOX_MAX_ADDRESS_SPACE_MB / EXECUTION_CORE_MEM_LIMIT.
#
# Two targets:
#   standalone (default)  starts the execution-core IMAGE as a throw-away container with
#                         the production flags; PID 1 is a tiny /healthz server (the real
#                         service needs Postgres and signing keys). The floods share its
#                         pids / memory / cpu cgroup exactly as they would in service.
#   --container NAME      runs against an already-running production-profile
#                         execution-core container (its real /healthz is the canary). The floods
#                         share that container's /tmp (512m), pids and memory: use an instance that
#                         is not serving traffic.
#
# What this does NOT drive: execution-core's Rust code paths (the 1 MiB output cap and the
# spawn permit queue are unit-tested in executor.rs); the output flood here emulates the
# reader-side cap with `head -c`. Linux + Docker only.
#
# Where the sandbox writes: its own /tmp is READ-ONLY (the --tmpfs is shadowed by the
# --ro-bind / / that follows it, measured in the deployed container), so every write goes to
# a per-run workspace under the container's /tmp that is bound writable, with HOME, TMPDIR and
# MPLCONFIGDIR inside it: the shape code_interpreter gives each program.
#
# Options:
#   --image REF          image to run (default: $EXECUTION_CORE_IMAGE or model-plane-execution-core)
#   --container NAME     target an existing container instead of starting one
#   --report FILE        write CALIB key=value lines to FILE (default: $SANDBOX_VERIFY_REPORT)
#   --keep               do not remove the standalone container on exit
# Env:
#   SANDBOX_VERIFY_EXTRA_DOCKER_ARGS  extra `docker run` flags (e.g. `--security-opt apparmor=unconfined`
#                                     on hosts whose AppArmor profile forbids bwrap's mounts)
set -uo pipefail
export MSYS_NO_PATHCONV=1 MSYS2_ARG_CONV_EXCL='*'

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEPLOY="$HERE/../deploy"
IMAGE="${EXECUTION_CORE_IMAGE:-model-plane-execution-core}"
TARGET_CT=""
REPORT="${SANDBOX_VERIFY_REPORT:-}"
KEEP=0
while [ $# -gt 0 ]; do
  case "$1" in
    --production-profile) ;;
    --image) IMAGE="$2"; shift ;;
    --container) TARGET_CT="$2"; shift ;;
    --report) REPORT="$2"; shift ;;
    --keep) KEEP=1 ;;
    *) echo "ERROR: unknown option $1"; exit 2 ;;
  esac
  shift
done

FAILS=0
ok()   { echo "ok: $*"; }
fail() { echo "FAIL: $*"; FAILS=$((FAILS + 1)); }
warn() { echo "WARN: $*"; }
calib() { echo "CALIB $1=$2"; [ -n "$REPORT" ] && echo "$1=$2" >> "$REPORT"; return 0; }
now_ms() { echo $(( $(date +%s%N) / 1000000 )); }

command -v docker >/dev/null 2>&1 || { echo "ERROR: docker is required"; exit 2; }
[ -n "$REPORT" ] && : > "$REPORT"

# --- the profile, read from the compose files ---------------------------------------
# Value after `:-` of `KEY: ${VAR:-default}` inside the top-level `execution-core:` block.
# A key that is not there is an error, never a silent default: the numbers this prints must
# describe the profile the compose files configure, not a fallback nobody runs.
compose_default() { # file key
  local v
  v=$(awk '/^  execution-core:/{f=1;next} /^  [A-Za-z0-9_-]+:/{f=0} f' "$1" \
      | sed -n 's/^ *'"$2"': *\$[{][A-Za-z_]*:-\([^}]*\)[}].*$/\1/p' | head -1)
  if [ -z "$v" ]; then echo "ERROR: $2 not found in the execution-core block of $(basename "$1")" >&2; return 2; fi
  echo "$v"
}
PROD="$DEPLOY/docker-compose.production.yml"
BASEYML="$DEPLOY/docker-compose.yml"
MEM_LIMIT=$(compose_default "$PROD" mem_limit) || exit 2
CPUS=$(compose_default "$PROD" cpus) || exit 2
PIDS_LIMIT=$(compose_default "$PROD" pids_limit) || exit 2
NOFILE=$(awk '/^  execution-core:/{f=1;next} /^  [A-Za-z0-9_-]+:/{f=0} f' "$PROD" | sed -n 's/^ *soft: *\([0-9][0-9]*\).*$/\1/p' | head -1)
[ -n "$NOFILE" ] || { echo "ERROR: ulimits.nofile.soft not found in the execution-core block of $(basename "$PROD")" >&2; exit 2; }
AS_MB=$(compose_default "$BASEYML" EXECUTION_SANDBOX_MAX_ADDRESS_SPACE_MB) || exit 2
FILE_MB=$(compose_default "$BASEYML" EXECUTION_SANDBOX_MAX_FILE_MB) || exit 2
OPEN_FILES=$(compose_default "$BASEYML" EXECUTION_SANDBOX_MAX_OPEN_FILES) || exit 2
CPU_FACTOR=$(compose_default "$BASEYML" EXECUTION_SANDBOX_CPU_FACTOR) || exit 2
MAX_OUT=$(compose_default "$BASEYML" EXECUTION_SANDBOX_MAX_OUTPUT_BYTES) || exit 2
CPU_GAP=2 # policy.rs CPU_HARD_GAP_SECS

calib profile_mem_limit "$MEM_LIMIT"
calib profile_cpus "$CPUS"
calib profile_pids_limit "$PIDS_LIMIT"
calib sandbox_as_mb "$AS_MB"
calib sandbox_file_mb "$FILE_MB"
calib sandbox_open_files "$OPEN_FILES"
calib sandbox_cpu_factor "$CPU_FACTOR"

# The per-run writable root. code_interpreter runs each program in a per-call workspace under
# the container's /tmp and binds exactly that root writable (WorkspaceWrite, child_env). The
# sandbox's /tmp is read-only, so every write the floods and the calibration make goes here.
# Programs start in it (--chdir), so relative paths land in the workspace, as they do in production.
WS=/tmp/msi-ws
# The argv assemble_argv emits for WorkspaceWrite (sandbox.rs), minus `-- <prog>`, with the cleared
# environment code_interpreter gets (child_env) and the network unshared.
BASE=(--die-with-parent --unshare-pid --unshare-uts --unshare-ipc --new-session --dev /dev --tmpfs /tmp --ro-bind / /
      --clearenv --setenv PATH /usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
      --setenv HOME "$WS" --setenv TMPDIR "$WS/.tmp" --setenv MPLBACKEND Agg --setenv MPLCONFIGDIR "$WS/.mplconfig"
      --setenv PYTHONDONTWRITEBYTECODE 1 --bind "$WS" "$WS" --unshare-net --chdir "$WS")

limits() { # cpu_seconds [as_mb]
  local cpu=$1 as=${2:-$AS_MB}
  echo "--cpu=${cpu}:$((cpu + CPU_GAP)) --as=$((as * 1048576)) --fsize=$((FILE_MB * 1048576)) --nofile=${OPEN_FILES} --core=0"
}

# --- the target container -------------------------------------------------------------
CT="$TARGET_CT"
cleanup() { if [ -z "$TARGET_CT" ] && [ "$KEEP" = 0 ] && [ -n "$CT" ]; then docker rm -f "$CT" >/dev/null 2>&1; fi; }
trap cleanup EXIT

CANARY='import http.server as h
class H(h.BaseHTTPRequestHandler):
    def do_GET(s):
        s.send_response(200); s.end_headers(); s.wfile.write(b"ok")
    def log_message(*a): pass
h.ThreadingHTTPServer(("127.0.0.1", 8083), H).serve_forever()'

if [ -z "$CT" ]; then
  CT="msi-prod-$$"
  # shellcheck disable=SC2086
  if ! docker run -d --name "$CT" --init --read-only \
        --tmpfs /tmp:rw,nosuid,size=512m \
        --cap-drop ALL --security-opt no-new-privileges:true \
        --security-opt "seccomp=$DEPLOY/seccomp-bwrap.json" \
        --memory "$MEM_LIMIT" --memory-swap "$MEM_LIMIT" --cpus "$CPUS" \
        --pids-limit "$PIDS_LIMIT" --ulimit "nofile=${NOFILE}:${NOFILE}" \
        ${SANDBOX_VERIFY_EXTRA_DOCKER_ARGS:-} \
        --entrypoint python3 "$IMAGE" -c "$CANARY" >/dev/null; then
    echo "ERROR: could not start $IMAGE (build it: cd 'apps/Model Plane' && ./scripts/compose.sh build execution-core)"
    exit 2
  fi
fi

ct() { docker exec "$CT" "$@"; }
# A fresh workspace for the next measurement: the previous one may hold a 512 MiB fill.
ws_reset() { ct sh -c "rm -rf '$WS' && mkdir -p '$WS/.tmp' '$WS/.mplconfig'"; }
healthz() { ct curl -fsS --max-time 3 http://127.0.0.1:8083/healthz >/dev/null 2>&1; }
running() { [ "$(docker inspect -f '{{.State.Running}}' "$CT" 2>/dev/null)" = "true" ]; }
cg() { ct sh -c "cat /sys/fs/cgroup/$1 2>/dev/null || cat /sys/fs/cgroup/$2 2>/dev/null || echo 0" 2>/dev/null | tr -d '\r'; }
pids_now() { cg pids.current pids/pids.current; }
mem_now() { cg memory.current memory/memory.usage_in_bytes; }
oom_kills() { ct sh -c "sed -n 's/^oom_kill //p' /sys/fs/cgroup/memory.events 2>/dev/null | head -1" 2>/dev/null | tr -d '\r'; }
# Run a command in the sandbox: sbx <wall_kill_seconds> "<prlimit flags>" cmd...
sbx() { local t=$1 lim=$2; shift 2; ct timeout -s KILL "$t" prlimit $lim -- bwrap "${BASE[@]}" -- "$@"; }
alive_check() { # label
  if ! running; then fail "$1: the container is no longer running"; return 1; fi
  local i
  for i in 1 2 3 4 5 6 7 8 9 10; do healthz && { ok "$1: /healthz still answers"; return 0; }; sleep 1; done
  fail "$1: /healthz did not answer within 10 s after the flood"; return 1
}

echo "VERIFY execution-core production-profile confinement ($CT)"
calib image "$IMAGE"
healthz || { sleep 3; healthz; } || { echo "ERROR: canary /healthz never came up in $CT"; exit 2; }

# Preflight: the sandbox itself must work under this profile (no --privileged).
ws_reset
if ! sbx 20 "$(limits 5)" true >/dev/null 2>&1; then
  fail "bwrap + prlimit do not work under the production profile (host AppArmor? set SANDBOX_VERIFY_EXTRA_DOCKER_ARGS='--security-opt apparmor=unconfined')"
  echo "FAIL: preflight; nothing further can be measured"; exit 1
fi
ok "preflight: bwrap under prlimit runs with cap_drop ALL, no-new-privileges and the scoped seccomp profile"
BASE_PIDS=$(pids_now); BASE_MEM=$(mem_now)
calib baseline_pids "$BASE_PIDS"; calib baseline_mem_bytes "$BASE_MEM"

# --- 1. fork bomb ----------------------------------------------------------------------
t0=$(now_ms)
sbx 8 "$(limits 20)" sh -c 'f(){ f|f& }; f; wait' >/dev/null 2>&1; rc=$?
t1=$(now_ms)
peak=$(cg pids.peak pids/pids.max_usage)
calib forkbomb_exit "$rc"; calib forkbomb_ms "$((t1 - t0))"; calib forkbomb_pids_peak "$peak"
# The kill takes the sandbox's pid namespace with it; the pids must drain back.
drained=0
for _ in $(seq 1 20); do
  n=$(pids_now); [ "${n:-0}" -le $((BASE_PIDS + 8)) ] && { drained=1; break; }; sleep 1
done
[ "$drained" = 1 ] && ok "fork bomb: pids drained back to baseline (peak ${peak:-?} of $PIDS_LIMIT)" \
                   || fail "fork bomb: ${n:-?} pids still alive 20 s after the kill (baseline $BASE_PIDS)"
alive_check "fork bomb"

# --- 2. memory bomb --------------------------------------------------------------------
OOM0=$(oom_kills)
t0=$(now_ms)
sbx 60 "$(limits 60)" python3 -c $'l=[]\ntry:\n    while True: l.append(b"x"*(64<<20))\nexcept MemoryError:\n    print("MEMERR", len(l)*64); raise SystemExit(3)' >/tmp/msi-mem.out 2>&1
rc=$?
t1=$(now_ms)
OOM1=$(oom_kills)
stop=unknown; [ "$rc" = 3 ] && stop=address_space; [ "$rc" = 137 ] && stop=oom_kill_or_timeout
calib membomb_exit "$rc"; calib membomb_stopped_by "$stop"; calib membomb_ms "$((t1 - t0))"
calib membomb_oom_kills "$(( ${OOM1:-0} - ${OOM0:-0} ))"; calib memory_peak_bytes "$(cg memory.peak memory/memory.max_usage_in_bytes)"
if [ "$rc" = 0 ]; then fail "memory bomb: exited 0 (the allocation loop was never stopped)"; else ok "memory bomb stopped (exit $rc, by $stop)"; fi
alive_check "memory bomb"
asb=$((AS_MB)); case "$MEM_LIMIT" in *g) lim_mb=$(( ${MEM_LIMIT%g} * 1024 )) ;; *m) lim_mb=${MEM_LIMIT%m} ;; *) lim_mb=0 ;; esac
[ "$lim_mb" -gt 0 ] && [ "$asb" -gt "$lim_mb" ] && warn "RLIMIT_AS (${asb} MB) exceeds the container mem_limit (${lim_mb} MB): a touched-pages bomb meets the container OOM first; AS only bounds address space"

# --- 3. CPU spin -----------------------------------------------------------------------
WALL=2; CPU=$((WALL * CPU_FACTOR))
t0=$(now_ms)
sbx 60 "$(limits "$CPU")" sh -c 'while :; do :; done' >/dev/null 2>&1; rc=$?
t1=$(now_ms); ms=$((t1 - t0))
calib cpuspin_exit "$rc"; calib cpuspin_ms "$ms"; calib cpuspin_cpu_limit_s "$CPU"
case "$rc" in
  152|24|137) [ "$ms" -le $(( (CPU + CPU_GAP + 6) * 1000 )) ] \
      && ok "CPU spin killed after ${ms} ms (exit $rc; SIGXCPU is 24, bwrap reports 128+24)" \
      || fail "CPU spin took ${ms} ms to die (limit ${CPU}s + ${CPU_GAP}s gap)" ;;
  *) fail "CPU spin ended with exit $rc after ${ms} ms, not a CPU-limit signal" ;;
esac
alive_check "CPU spin"

# --- 4. file flood (one big file) and tmpfs fill (many files) --------------------------
ws_reset
out=$(sbx 60 "$(limits 60)" sh -c 'dd if=/dev/zero of="$1/flood" bs=1M count=200 2>/dev/null; echo "rc=$?"; wc -c < "$1/flood"' _ "$WS" 2>&1 | tr -d '\r' | tr '\n' ' ')
size=$(echo "$out" | sed -n 's/.* \([0-9][0-9]*\) *$/\1/p')
frc=$(echo "$out" | sed -n 's/.*rc=\([0-9]*\).*/\1/p')
calib fileflood_rc "${frc:-?}"; calib fileflood_bytes "${size:-?}"
if [ -n "$size" ] && [ "$size" -le $((FILE_MB * 1048576)) ] && [ "${frc:-0}" != 0 ]; then
  ok "file flood stopped at ${size} bytes (limit $((FILE_MB * 1048576)); dd exit ${frc}, 153 = SIGXFSZ)"
else
  fail "file flood was not bounded by --fsize (output: $out)"
fi
alive_check "file flood"

ws_reset
OOM0=$(oom_kills); t0=$(now_ms)
sbx 25 "$(limits 60)" sh -c 'i=0; while [ $i -lt 100 ]; do dd if=/dev/zero of="$1/f$i" bs=1M count=60 2>/dev/null || break; i=$((i+1)); done; echo FILES=$i' _ "$WS" >/tmp/msi-fill.out 2>&1
rc=$?; t1=$(now_ms); OOM1=$(oom_kills)
calib tmpfsfill_exit "$rc"; calib tmpfsfill_ms "$((t1 - t0))"; calib tmpfsfill_oom_kills "$(( ${OOM1:-0} - ${OOM0:-0} ))"
alive_check "tmpfs fill"

# --- 5. output flood (reader-side cap emulation of the 1 MiB read cap) ------------------
ws_reset
M0=$(mem_now); t0=$(now_ms)
got=$(ct sh -c 'MAX=$1; shift; timeout -s KILL 30 "$@" | head -c "$MAX" | wc -c' _ "$MAX_OUT" \
        prlimit $(limits 30) -- bwrap "${BASE[@]}" -- sh -c 'yes x | head -c 104857600' 2>/dev/null | tr -d '\r ')
t1=$(now_ms); M1=$(mem_now)
calib outputflood_read_bytes "${got:-?}"; calib outputflood_ms "$((t1 - t0))"; calib outputflood_mem_delta_bytes "$((M1 - M0))"
if [ "$got" = "$MAX_OUT" ] && [ $((t1 - t0)) -lt 20000 ] && [ $((M1 - M0)) -lt 268435456 ]; then
  ok "output flood: reader stopped at ${MAX_OUT} bytes in $((t1 - t0)) ms, memory delta $((M1 - M0)) bytes"
else
  fail "output flood: read ${got:-?} bytes (cap $MAX_OUT) in $((t1 - t0)) ms, memory delta $((M1 - M0)) bytes"
fi
alive_check "output flood"

# --- 6. calibration: what do the limits cost real work? --------------------------------
REAL=$'import resource\nimport numpy as np, pandas as pd\nimport matplotlib\nmatplotlib.use("Agg")\nimport matplotlib.pyplot as plt\nimport openpyxl, docx, reportlab, pypdf, PIL\ndf = pd.DataFrame({"a": np.arange(1_000_000), "b": np.random.rand(1_000_000)})\ng = df.groupby(df.a % 100).b.mean()\nfig, ax = plt.subplots(); g.plot(ax=ax); fig.savefig("p.png")\nwb = openpyxl.Workbook(); wb.active.append([1, 2]); wb.save("t.xlsx")\nprint("OK", int(open("/proc/self/status").read().split("VmPeak:")[1].split()[0]) // 1024)'
min_pass=""
for as in 512 768 1024 1536 2048 3072 4096 6144; do
  ws_reset
  o=$(sbx 120 "$(limits 120 "$as")" python3 -c "$REAL" 2>&1 | tail -1 | tr -d '\r')
  case "$o" in OK*) calib "realwork_as_${as}mb" "pass(vmpeak=${o#OK })"; [ -z "$min_pass" ] && min_pass=$as ;; *) calib "realwork_as_${as}mb" fail ;; esac
done
calib realwork_min_as_mb "${min_pass:-none}"
if [ -z "$min_pass" ]; then fail "calibration: the numpy/pandas/matplotlib/openpyxl job fails at every tested --as ceiling"
elif [ "$min_pass" -gt "$AS_MB" ]; then fail "calibration: the real-work job needs --as >= ${min_pass} MB but the default is ${AS_MB} MB (false positives for numpy/pandas/matplotlib jobs)"
else ok "calibration: real work runs from --as ${min_pass} MB; default ${AS_MB} MB leaves a factor of $(( AS_MB / min_pass ))"; fi

# The render layer's own call (office_verify/check.py): the prewarmed profile copied into the
# workspace, `--outdir` created first (check.py makes it too), the workspace as the cwd.
lo_cmd='mkdir -p "$1/out" && cp -r /opt/verevon/lo-profile "$1/lo-profile" && printf "a,b\n1,2\n" > "$1/t.csv" && soffice --headless --norestore -env:UserInstallation=file://"$1"/lo-profile --convert-to pdf --outdir "$1/out" "$1/t.csv" >/dev/null 2>&1 && test -s "$1/out/t.pdf" && echo OK'
lo_pass=""
for as in 2048 4096 8192 16384; do
  ws_reset
  o=$(sbx 180 "$(limits 180 "$as")" sh -c "$lo_cmd" _ "$WS" 2>&1 | tail -1 | tr -d '\r')
  [ "$o" = OK ] && { calib "libreoffice_as_${as}mb" pass; [ -z "$lo_pass" ] && lo_pass=$as; } || calib "libreoffice_as_${as}mb" fail
done
calib libreoffice_min_as_mb "${lo_pass:-none}"
if [ -z "$lo_pass" ]; then warn "LibreOffice render did not run under any tested --as (check the image's profile at /opt/verevon/lo-profile)"
elif [ "$lo_pass" -gt "$AS_MB" ]; then warn "LibreOffice render needs --as >= ${lo_pass} MB (> default ${AS_MB} MB): the Office verifier render layer would be refused; raise the ceiling for that caller or lower the layer's footprint"
else ok "LibreOffice render runs from --as ${lo_pass} MB"; fi

# Resource evidence next to the script output: what the container used after the floods.
calib container_stats_end "$(docker stats --no-stream --format '{{.MemUsage}} | cpu {{.CPUPerc}} | pids {{.PIDs}}' "$CT" 2>/dev/null | tr -d '\r')"
ct rm -rf "$WS" >/dev/null 2>&1
echo
if [ "$FAILS" -gt 0 ]; then echo "FAIL: ${FAILS} production-profile check(s) failed"; exit 1; fi
echo "PASS — execution-core confinement holds under the production profile; record the CALIB lines in the PR description."
