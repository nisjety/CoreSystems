#!/usr/bin/env bash
# lib.sh: helpers shared by the Quarry evidence probe scripts in this directory.
# Source it; it defines functions only and prints nothing.
#
# Every probe prints one line per check:
#   PASS <name>: <value>
#   FAIL <name>: expected <x>, got <y>
#   NOT-RUN <name>: <why>
# and ends with a summary line `RESULT pass=N fail=N notrun=N` (exit 1 on any FAIL, 0
# otherwise: NOT-RUN is not a failure, but it is not evidence either).
#
# Secrets: a token or cookie is only ever read from a file whose path is in the
# environment, handed to curl through a header file or -b, and never echoed. Response
# bodies are kept in $EVIDENCE_DIR only when the probe says so; the ones that could
# carry content are reduced to status codes and counts.

RESULTS_PASS=0
RESULTS_FAIL=0
RESULTS_NOTRUN=0

ev_pass() { echo "PASS $1: $2"; RESULTS_PASS=$((RESULTS_PASS + 1)); }
ev_fail() { echo "FAIL $1: $2"; RESULTS_FAIL=$((RESULTS_FAIL + 1)); }
ev_notrun() { echo "NOT-RUN $1: $2"; RESULTS_NOTRUN=$((RESULTS_NOTRUN + 1)); }

# ev_check <name> <expected> <actual>: exact string comparison.
ev_check() {
  if [ "$3" = "$2" ]; then ev_pass "$1" "$3"; else ev_fail "$1" "expected $2, got $3"; fi
}

# ev_check_ge <name> <minimum> <actual>: integer comparison.
ev_check_ge() {
  if [ "$3" -ge "$2" ] 2>/dev/null; then ev_pass "$1" "$3 (>= $2)"; else ev_fail "$1" "expected >= $2, got $3"; fi
}

ev_summary() {
  echo "RESULT pass=$RESULTS_PASS fail=$RESULTS_FAIL notrun=$RESULTS_NOTRUN"
  [ "$RESULTS_FAIL" -eq 0 ]
}

# ev_json_get <file> <dot.path>: the JSON value at the path, or __undefined__. Needs node.
ev_json_get() {
  node -e '
    const fs = require("fs");
    let doc;
    try { doc = JSON.parse(fs.readFileSync(process.argv[1], "utf8")); }
    catch { process.stdout.write("__unparsable__"); process.exit(0); }
    const value = process.argv[2].split(".").reduce(
      (node, key) => (node == null ? undefined : node[key]), doc);
    process.stdout.write(value === undefined ? "__undefined__" : JSON.stringify(value));
  ' "$1" "$2"
}

# ev_uuid: a lower-case unique token for probe markers (no external uuidgen needed).
ev_uuid() {
  node -e 'process.stdout.write(require("crypto").randomUUID())'
}

# ev_require <command>...: exit 2 with a message when a tool is missing.
ev_require() {
  local tool
  for tool in "$@"; do
    command -v "$tool" >/dev/null 2>&1 || { echo "missing tool: $tool" >&2; exit 2; }
  done
}

# ev_count_log <marker>: how many lines of the edge/gateway log contain the marker.
# The log source is LOG_CMD (a command printing the log, e.g. "docker logs sbs-edge"),
# merged stdout and stderr, so `docker logs` works as is.
ev_count_log() {
  # shellcheck disable=SC2086
  $LOG_CMD 2>&1 </dev/null | grep -c -F -- "$1" || true
}

# ev_count_log_ci <regex>: same, extended regex, case-insensitive.
ev_count_log_ci() {
  # shellcheck disable=SC2086
  $LOG_CMD 2>&1 </dev/null | grep -c -i -E -- "$1" || true
}

# ev_http <outfile> <curl args...>: runs curl, writes the body to <outfile>, prints the
# HTTP status (000 when the connection failed). Never prints the body.
ev_http() {
  local out=$1
  shift
  curl -sS -o "$out" -w '%{http_code}' --max-time "${CURL_MAX_TIME:-60}" ${CURL_OPTS:-} "$@" 2>/dev/null || echo 000
}
