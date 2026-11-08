#!/usr/bin/env bash
# otp-login-fixture.sh: evidence for the E3.1-j login-fixture OTP flow (W3, the fill_otp path).
#
# The flow under test: a run reaches a one-time-code page over https. Quarry reads the code
# from a RECORDED mailbox through integration-core's executor-only mail.otp.read. After one human
# approval the code goes into the field through trusted input and the page signs in. The evidence
# is what the run leaves behind: the code's digits are in no receipt, no service log and no
# artifact the operator passes in, and the fixture and the mailbox logged the expected requests.
#
# Written, not run (W3 E3.1-j, code only). A RECIPE for a side-by-side stack, never the live
# stack and never a real mailbox. integration-core must read the recorded mailbox through
# MICROSOFT_GRAPH_BASE_URL or GOOGLE_API_BASE_URL, on that stack only. The runbook, the blockers
# and the manual checks are in apps/Ingestion Plane/Quarry-v2/docs/OTP_HANDOFF.md.
#
# Modes:
#   --self-test           helpers, leak counting and the recording rules; no network, no stack
#   --validate-recording  check MAILBOX_RECORDING; prints the message count, never the code
#   --serve-mailbox       the recorded mailbox: Microsoft Graph inbox and Gmail reads answered
#                         from the recording, with an access log (method, path, status only)
#   --serve-fixture       the login fixture over https: the /otp-login pages with the markup of
#                         the quarry-runtime test site, with an access log (method, path, status)
#   (no argument)         verify one run; read only
#
# Verify inputs (environment):
#   EDGE_URL            quarry-edge of the stack under test (required; no default on purpose)
#   RUN_ID              the run that filled the code (required)
#   EDGE_TOKEN_FILE     file holding the bearer of the user who owns RUN_ID (required). It is
#                       copied to a 0600 header file and never printed.
#   MAILBOX_RECORDING   the recording the mailbox served (required). Its fixture_code is the code
#                       scanned for. The code is never printed.
#   OTP_FIXTURE_URL     the https URL the run started on (required)
#   MAILBOX_LOG         access log of --serve-mailbox for this run (optional; NOT-RUN without it)
#   FIXTURE_LOG         access log of --serve-fixture for this run (optional; NOT-RUN without it)
#   LOG_CMD_EDGE, LOG_CMD_IC, LOG_CMD_BROKER, LOG_CMD_MODEL
#                       commands that print one service's log, e.g. "docker logs <container>".
#                       Words are split, not quoted. The whole log is scanned, then discarded.
#   ARTIFACT_DIR        host folder with the run's stored artifacts (optional)
#   RUN_ID_AMBIGUOUS    a run on /otp-login/ambiguous that must not complete a fill (optional)
#
# Serve inputs (environment):
#   MAILBOX_RECORDING, MAILBOX_LOG (required); MAILBOX_PORT (18080), MAILBOX_BIND (127.0.0.1)
#   FIXTURE_LOG, FIXTURE_TLS_KEY, FIXTURE_TLS_CERT (required; dev files kept outside the repo),
#   FIXTURE_PORT (18443), FIXTURE_BIND (127.0.0.1)
#
# Usage:
#   ./scripts/evidence/otp-login-fixture.sh --self-test
#   MAILBOX_RECORDING=<file> ./scripts/evidence/otp-login-fixture.sh --validate-recording
#   MAILBOX_RECORDING=<file> MAILBOX_LOG=<log> ./scripts/evidence/otp-login-fixture.sh --serve-mailbox
#   MAILBOX_RECORDING=<file> FIXTURE_LOG=<log> FIXTURE_TLS_KEY=<key> FIXTURE_TLS_CERT=<cert> \
#     ./scripts/evidence/otp-login-fixture.sh --serve-fixture
#   EDGE_URL=... RUN_ID=... EDGE_TOKEN_FILE=... MAILBOX_RECORDING=... OTP_FIXTURE_URL=... \
#     ./scripts/evidence/otp-login-fixture.sh
set -uo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=lib.sh
. "$HERE/lib.sh"

# The Node side: one program, MODE picks the role. Inputs come only from the environment.
# The code leaves this program only through MODE=code, which the script captures into a
# variable. Error messages never carry it.
otp_node() {
  cat <<'JS'
'use strict';
const fs = require('fs');
const http = require('http');
const https = require('https');

const env = process.env;

// Mirrors `otpKeyword` in integration-corev2 internal/actions/mail_otp.go.
const OTP_KEYWORD = /(code|otp|passcode|pass code|pin|verification|verify|security|one[- ]time|login|sign[- ]in|kode|engangskode|bekreft|verifiser)/i;

function die(message) {
  process.stderr.write(`${message}\n`);
  process.exit(2);
}

function ageOf(message) {
  return message.age_seconds === undefined ? 60 : message.age_seconds;
}

// The recording: fixture_code and messages. Refuses anything integration-core would not
// extract. Each message is read the way integration-core reads it: subject, a newline, text.
function loadRecording(file) {
  if (!file) die('MAILBOX_RECORDING is not set');
  let doc;
  try {
    doc = JSON.parse(fs.readFileSync(file, 'utf8'));
  } catch (error) {
    die('the recording is missing or is not JSON');
  }
  const code = doc.fixture_code;
  if (typeof code !== 'string' || !/^[0-9]{4,10}$/.test(code)) die('fixture_code must be 4 to 10 digits');
  if (!Array.isArray(doc.messages) || doc.messages.length === 0) die('messages must be a non-empty array');
  doc.messages.forEach((message, index) => {
    const at = `message ${index + 1}`;
    if (!message || typeof message.text !== 'string') die(`${at}: text must be a string`);
    const subject = typeof message.subject === 'string' ? message.subject : '';
    const haystack = `${subject}\n${message.text}`;
    const position = haystack.indexOf(code);
    if (position < 0) die(`${at}: the text does not contain fixture_code`);
    const firstRun = /[0-9]{4,10}/.exec(haystack);
    if (!firstRun || firstRun[0] !== code) die(`${at}: the first digit run must be the code`);
    if (!OTP_KEYWORD.test(haystack.slice(Math.max(0, position - 48), position))) {
      die(`${at}: no OTP keyword within 48 characters before the code`);
    }
    const age = ageOf(message);
    if (!Number.isInteger(age) || age < 30 || age > 540) die(`${at}: age_seconds must be 30 to 540`);
  });
  return doc;
}

// The mailbox: each message is re-dated at every request, so integration-core's window holds.
function mailMessages(recording) {
  const now = Date.now();
  return recording.messages.map((message, index) => ({
    id: `rec-${index + 1}`,
    subject: typeof message.subject === 'string' ? message.subject : '',
    text: message.text,
    at: now - ageOf(message) * 1000,
  }));
}

function serveMailbox() {
  const recording = loadRecording(env.MAILBOX_RECORDING);
  const logFile = env.MAILBOX_LOG;
  if (!logFile) die('MAILBOX_LOG is not set');
  const port = Number(env.MAILBOX_PORT || 18080);
  const bind = env.MAILBOX_BIND || '127.0.0.1';
  const server = http.createServer((req, res) => {
    const path = (req.url || '/').split('?')[0];
    const reply = (status, body) => {
      res.writeHead(status, { 'content-type': 'application/json' });
      res.end(JSON.stringify(body));
      fs.appendFileSync(logFile, `${new Date().toISOString()} ${req.method} ${path} ${status}\n`);
    };
    if (req.method !== 'GET') return reply(405, { error: { code: 'METHOD_NOT_ALLOWED' } });
    const messages = mailMessages(recording);
    // Microsoft Graph: GET {base}/v1.0/me/mailFolders/inbox/messages (the $filter is not applied here).
    if (path === '/v1.0/me/mailFolders/inbox/messages') {
      return reply(200, {
        value: messages.map((m) => ({
          id: m.id,
          subject: m.subject,
          receivedDateTime: new Date(m.at).toISOString(),
          body: { contentType: 'text', content: m.text },
        })),
      });
    }
    // Gmail: GET {base}/gmail/v1/users/me/messages, then GET .../messages/{id}?format=full
    if (path === '/gmail/v1/users/me/messages') {
      return reply(200, messages.length
        ? { messages: messages.map((m) => ({ id: m.id, threadId: m.id })), resultSizeEstimate: messages.length }
        : { resultSizeEstimate: 0 });
    }
    const single = /^\/gmail\/v1\/users\/me\/messages\/([A-Za-z0-9_-]+)$/.exec(path);
    if (single) {
      const message = messages.find((m) => m.id === single[1]);
      if (!message) return reply(404, { error: { code: 'NOT_FOUND' } });
      return reply(200, {
        id: message.id,
        threadId: message.id,
        internalDate: String(message.at),
        payload: {
          mimeType: 'text/plain',
          headers: [{ name: 'Subject', value: message.subject }],
          body: { size: Buffer.byteLength(message.text), data: Buffer.from(message.text, 'utf8').toString('base64url') },
        },
      });
    }
    return reply(404, { error: { code: 'NOT_FOUND' } });
  });
  server.on('error', (error) => die(`the mailbox could not listen: ${error.code}`));
  server.listen(port, bind, () => process.stderr.write(`recorded mailbox listening on ${bind}:${port}\n`));
}

// The /otp-login pages: the markup of quarry-runtime test_site.rs (otp_login_page and
// otp_login_ambiguous). Keep the two in step by hand (OTP_HANDOFF.md, known gaps).
const OTP_PAGE = `<!doctype html><html lang="en"><head><title>Verify sign-in</title></head>
<body><h1>Enter the code we emailed you</h1>
<form method="post" action="/otp-login">
  <label for="code">Verification code</label>
  <input name="code" id="code" required inputmode="numeric" autocomplete="one-time-code">
  <button type="submit" id="verify">Verify</button>
</form>
</body></html>`;

const OTP_AMBIGUOUS_PAGE = `<!doctype html><html lang="en"><head><title>Verify sign-in</title></head>
<body><h1>Enter the code</h1>
<form method="post" action="/otp-login">
  <input name="code" id="code-a" required autocomplete="one-time-code">
  <input name="code" id="code-b" required autocomplete="one-time-code">
  <button type="submit" id="verify">Verify</button>
</form>
</body></html>`;

function serveFixture() {
  const recording = loadRecording(env.MAILBOX_RECORDING);
  const code = recording.fixture_code;
  const logFile = env.FIXTURE_LOG;
  if (!logFile) die('FIXTURE_LOG is not set');
  if (!env.FIXTURE_TLS_KEY || !env.FIXTURE_TLS_CERT) {
    die('FIXTURE_TLS_KEY and FIXTURE_TLS_CERT are required: the fixture is https only');
  }
  const port = Number(env.FIXTURE_PORT || 18443);
  const bind = env.FIXTURE_BIND || '127.0.0.1';
  const tls = { key: fs.readFileSync(env.FIXTURE_TLS_KEY), cert: fs.readFileSync(env.FIXTURE_TLS_CERT) };
  const server = https.createServer(tls, (req, res) => {
    const path = (req.url || '/').split('?')[0];
    const send = (status, body, headers) => {
      res.writeHead(status, { 'content-type': 'text/html; charset=utf-8', ...(headers || {}) });
      res.end(body);
      fs.appendFileSync(logFile, `${new Date().toISOString()} ${req.method} ${path} ${status}\n`);
    };
    if (req.method === 'GET' && path === '/otp-login') return send(200, OTP_PAGE);
    if (req.method === 'GET' && path === '/otp-login/ambiguous') return send(200, OTP_AMBIGUOUS_PAGE);
    if (req.method === 'GET' && path === '/login') {
      return send(200, '<!doctype html><html lang="en"><head><title>Sign in</title></head><body><h1>Sign in</h1></body></html>');
    }
    if (req.method === 'GET' && path === '/dashboard') {
      const signedIn = String(req.headers.cookie || '').split(';').map((part) => part.trim()).includes('auth=alice');
      if (!signedIn) return send(303, '', { location: '/login' });
      return send(200, '<!doctype html><html lang="en"><head><title>Dashboard</title></head><body><h1>Dashboard</h1></body></html>');
    }
    if (req.method === 'POST' && path === '/otp-login') {
      let body = '';
      req.on('data', (chunk) => { body += chunk; });
      req.on('end', () => {
        if (body.length > 1024) return send(413, '<h1>Too large</h1>');
        // The submitted value is compared here. It is never logged and never echoed.
        if (new URLSearchParams(body).get('code') === code) {
          return send(303, '', { location: '/dashboard', 'set-cookie': 'auth=alice; Path=/; HttpOnly' });
        }
        return send(401, '<h1>Invalid code</h1>');
      });
      return undefined;
    }
    return send(404, '<h1>Not found</h1>');
  });
  server.on('error', (error) => die(`the fixture could not listen: ${error.code}`));
  server.listen(port, bind, () => process.stderr.write(`login fixture listening on https://${bind}:${port}\n`));
}

// The receipts of one run (GET /v1/agent/runs/{id}/receipts): counts and flags only.
function summarize() {
  const raw = fs.readFileSync(env.RECEIPTS_FILE, 'utf8');
  let doc;
  try {
    doc = JSON.parse(raw);
  } catch (error) {
    process.stdout.write(JSON.stringify({ error: 'unparsable' }));
    return;
  }
  const receipts = Array.isArray(doc.data) ? doc.data : [];
  const fills = receipts.filter((r) => r && r.action && r.action.type === 'fill_otp_ref');
  const completed = fills.filter((r) => r.outcome && r.outcome.status === 'completed');
  const allowedKeys = ['type', 'snapshot_id', 'generation', 'ref_id', 'approval_grant_id'];
  process.stdout.write(JSON.stringify({
    receipts: receipts.length,
    fill_receipts: fills.length,
    completed_fills: completed.length,
    approval_present: completed.length > 0
      && completed.every((r) => typeof r.action.approval_grant_id === 'string' && r.action.approval_grant_id.trim() !== ''),
    action_keys_ok: fills.length > 0
      && fills.every((r) => Object.keys(r.action).every((key) => allowedKeys.includes(key))),
    redacted_marker_present: raw.includes('[redacted:otp]'),
  }));
}

const mode = env.MODE;
if (mode === 'code') {
  process.stdout.write(loadRecording(env.MAILBOX_RECORDING).fixture_code);
} else if (mode === 'validate') {
  process.stdout.write(`ok messages=${loadRecording(env.MAILBOX_RECORDING).messages.length}\n`);
} else if (mode === 'summarize') {
  summarize();
} else if (mode === 'mailbox') {
  serveMailbox();
} else if (mode === 'fixture') {
  serveFixture();
} else {
  die(`unknown MODE "${mode}"`);
}
JS
}

WORK=""
cleanup() {
  if [ -n "$WORK" ] && [ -d "$WORK" ]; then
    rm -rf "$WORK"
  fi
}
trap cleanup EXIT

# A private folder for receipts, logs and the header file. Removed on exit. Nothing from it is
# copied anywhere else.
new_work() {
  WORK=$(mktemp -d) || exit 2
  chmod 700 "$WORK"
}

# node_path <path>: the path in a form a native node or curl can open. Under Git Bash this is
# cygpath -m (C:/...); elsewhere the path is returned unchanged.
node_path() {
  if command -v cygpath >/dev/null 2>&1; then
    cygpath -m -- "$1"
  else
    printf '%s' "$1"
  fi
}

# json_field <file> <key>: one top-level value of a JSON file written by MODE=summarize.
json_field() {
  ev_json_get "$(node_path "$1")" "$2"
}

CODE=""
CODE_PATS=()

# The patterns the scans look for: the code as a whole digit run, and its 3+3 split by a space or
# a hyphen (the form integration-core also reads). Set CODE before calling.
code_patterns() {
  CODE_PATS=(-e "$CODE")
  if [ "${#CODE}" -eq 6 ]; then
    CODE_PATS+=(-e "${CODE:0:3} ${CODE:3}" -e "${CODE:0:3}-${CODE:3}")
  fi
}

# leak_count <file>: how many lines of the file carry the code. Prints a count, never the code.
# A file that cannot be read is reported as such, so it can never pass as zero.
leak_count() {
  if [ ! -r "$1" ]; then
    printf 'unreadable'
    return
  fi
  local n
  n=$(grep -c -F -w "${CODE_PATS[@]}" -- "$1" 2>/dev/null) || true
  printf '%s' "${n:-0}"
}

# scan_log <name> <command>: runs the command that prints one service's log into WORK and counts
# the code lines in it. The log is not kept. An unset command is NOT-RUN; a failing one is a FAIL.
scan_log() {
  local name=$1 cmd=$2 out="$WORK/log-$1.txt"
  if [ -z "$cmd" ]; then
    ev_notrun "$name log leak scan" "LOG_CMD is not set for this service"
    return
  fi
  # shellcheck disable=SC2086
  if ! $cmd >"$out" 2>&1 </dev/null; then
    ev_fail "$name log could be read" "the LOG_CMD command exited non-zero"
    return
  fi
  ev_check "$name log holds no code digit" "0" "$(leak_count "$out")"
}

# The bearer as a 0600 header file. The token is read from its file and is never printed.
make_auth_header() {
  AUTH_HDR="$WORK/auth.hdr"
  {
    printf 'authorization: Bearer '
    tr -d '\r\n' <"$EDGE_TOKEN_FILE"
    printf '\n'
  } >"$AUTH_HDR"
  chmod 600 "$AUTH_HDR"
}

# fetch_receipts <run id> <out file>: GET the run's receipts; prints the HTTP status.
fetch_receipts() {
  ev_http "$2" -H "@$(node_path "$AUTH_HDR")" "$EDGE_URL/v1/agent/runs/$1/receipts"
}

# summarize_receipts <receipts file> <summary file>: counts and flags only; never the receipts.
summarize_receipts() {
  otp_node | MODE=summarize RECEIPTS_FILE="$(node_path "$1")" node - >"$2" 2>/dev/null || true
}

verify_run() {
  local st n
  ev_require curl node grep
  : "${EDGE_URL:?set EDGE_URL to quarry-edge of the stack under test}"
  : "${RUN_ID:?set RUN_ID to the run that filled the code}"
  : "${EDGE_TOKEN_FILE:?set EDGE_TOKEN_FILE to a file holding the bearer of the run owner}"
  : "${MAILBOX_RECORDING:?set MAILBOX_RECORDING to the recording the mailbox served}"
  : "${OTP_FIXTURE_URL:?set OTP_FIXTURE_URL to the https URL the run started on}"
  [ -r "$EDGE_TOKEN_FILE" ] || { echo "EDGE_TOKEN_FILE is not readable" >&2; exit 2; }
  new_work
  make_auth_header
  CODE=$(otp_node | MODE=code MAILBOX_RECORDING="$(node_path "$MAILBOX_RECORDING")" node -) || CODE=""
  if [ -z "$CODE" ]; then
    echo "the recording could not be read; run --validate-recording" >&2
    exit 2
  fi
  code_patterns

  case "$OTP_FIXTURE_URL" in
    https://*) ev_pass "precondition: the fixture URL is https" "yes" ;;
    *) ev_fail "precondition: the fixture URL is https" "no; the executor refuses http outside tests (D-E31c-3)" ;;
  esac
  st=$(ev_http "$WORK/health.json" "$EDGE_URL/health")
  ev_check "precondition: quarry-edge answers /health" "200" "$st"
  if [ "$st" != "200" ]; then
    ev_summary
    exit 1
  fi

  st=$(fetch_receipts "$RUN_ID" "$WORK/receipts.json")
  ev_check "0 receipts of the run readable" "200" "$st"
  if [ "$st" != "200" ]; then
    ev_summary
    exit 1
  fi
  summarize_receipts "$WORK/receipts.json" "$WORK/summary.json"
  ev_check "1 fill_otp_ref receipts on the run" "1" "$(json_field "$WORK/summary.json" fill_receipts)"
  ev_check "2 the fill completed" "1" "$(json_field "$WORK/summary.json" completed_fills)"
  ev_check "3 the completed fill names its sensitive approval grant" "true" "$(json_field "$WORK/summary.json" approval_present)"
  ev_check "4 the fill action holds ids only (no value, code or text)" "true" "$(json_field "$WORK/summary.json" action_keys_ok)"
  ev_check "5 the receipts carry the [redacted:otp] marker" "true" "$(json_field "$WORK/summary.json" redacted_marker_present)"
  ev_check "6 no code digit in the run's receipts" "0" "$(leak_count "$WORK/receipts.json")"

  if [ -n "${MAILBOX_LOG:-}" ] && [ -r "$MAILBOX_LOG" ]; then
    n=$(grep -c -E ' GET /(v1\.0/me/mailFolders/inbox/messages|gmail/v1/users/me/messages) 200$' "$MAILBOX_LOG" 2>/dev/null) || true
    ev_check_ge "7 the recorded mailbox answered the inbox reads" 1 "${n:-0}"
    ev_check "8 no code digit in the mailbox log" "0" "$(leak_count "$MAILBOX_LOG")"
  else
    ev_notrun "7 recorded mailbox reads" "MAILBOX_LOG is not set or not readable"
  fi

  if [ -n "${FIXTURE_LOG:-}" ] && [ -r "$FIXTURE_LOG" ]; then
    n=$(grep -c -E ' POST /otp-login 303$' "$FIXTURE_LOG" 2>/dev/null) || true
    ev_check_ge "9 the login fixture accepted a code (POST /otp-login 303)" 1 "${n:-0}"
    n=$(grep -c -E ' GET /dashboard 200$' "$FIXTURE_LOG" 2>/dev/null) || true
    ev_check_ge "10 the run reached the signed-in page (GET /dashboard 200)" 1 "${n:-0}"
    ev_check "11 no code digit in the fixture log" "0" "$(leak_count "$FIXTURE_LOG")"
  else
    ev_notrun "9 login fixture requests" "FIXTURE_LOG is not set or not readable"
  fi

  scan_log "edge" "${LOG_CMD_EDGE:-}"
  scan_log "integration-core" "${LOG_CMD_IC:-}"
  if [ -n "${LOG_CMD_IC:-}" ] && [ -r "$WORK/log-integration-core.txt" ]; then
    n=$(grep -c -F 'integration.mail.otp.read' "$WORK/log-integration-core.txt" 2>/dev/null) || true
    if [ "${n:-0}" -ge 1 ] 2>/dev/null; then
      ev_pass "audit event in the integration-core log" "$n line(s)"
    else
      ev_notrun "audit event in the integration-core log" "not printed there; the audit goes to the events publisher, check the stream"
    fi
  fi
  scan_log "broker" "${LOG_CMD_BROKER:-}"
  scan_log "model" "${LOG_CMD_MODEL:-}"

  if [ -n "${ARTIFACT_DIR:-}" ] && [ -d "$ARTIFACT_DIR" ]; then
    n=$(grep -r -l -a -F -w "${CODE_PATS[@]}" -- "$ARTIFACT_DIR" 2>/dev/null | wc -l | tr -d ' ') || true
    ev_check "artifacts: no stored file holds a code digit (text scan)" "0" "${n:-0}"
  else
    ev_notrun "artifacts leak scan" "ARTIFACT_DIR is not set or is not a folder"
  fi

  if [ -n "${RUN_ID_AMBIGUOUS:-}" ]; then
    st=$(fetch_receipts "$RUN_ID_AMBIGUOUS" "$WORK/receipts-ambiguous.json")
    ev_check "12 ambiguous-page run: receipts readable" "200" "$st"
    summarize_receipts "$WORK/receipts-ambiguous.json" "$WORK/summary-ambiguous.json"
    ev_check "12 ambiguous-page run: no fill completed" "0" "$(json_field "$WORK/summary-ambiguous.json" completed_fills)"
    ev_check "13 ambiguous-page run: no code digit in its receipts" "0" "$(leak_count "$WORK/receipts-ambiguous.json")"
  else
    ev_notrun "12 ambiguous-page run" "RUN_ID_AMBIGUOUS is not set"
  fi

  echo "manual, not scripted (OTP_HANDOFF.md section 7): approval card text, model transcript, screenshots after the fill, replay refusal, audit stream"
  ev_summary
}

self_test() {
  local f s good bad printed
  ev_require node grep
  new_work
  CODE="864209"
  code_patterns
  f="$WORK/sample.txt"
  s="$WORK/summary.json"

  printf 'no digits of interest here\n' >"$f"
  ev_check "self-test: a clean file holds no code digit" "0" "$(leak_count "$f")"
  printf 'receipt text 864209 in a log\n' >"$f"
  ev_check "self-test: a plain code is counted" "1" "$(leak_count "$f")"
  printf 'split 864 209 across the field\n' >"$f"
  ev_check "self-test: a 3 and 3 split is counted" "1" "$(leak_count "$f")"
  printf 'the longer run 1864209 is another number\n' >"$f"
  ev_check "self-test: a longer digit run is not the code" "0" "$(leak_count "$f")"
  ev_check "self-test: an unreadable file is reported, not passed" "unreadable" "$(leak_count "$WORK/absent.txt")"

  good="$WORK/good.json"
  cat >"$good" <<'JSON'
{"data":[
 {"receipt_id":"r1","action":{"type":"navigate","url":"https://fixture.example/otp-login"},"outcome":{"status":"completed","latency_ms":12}},
 {"receipt_id":"r2","action":{"type":"fill_otp_ref","snapshot_id":"s1","generation":1,"ref_id":"e1","approval_grant_id":"g1"},"outcome":{"status":"completed","latency_ms":40},"observation":{"url":"https://fixture.example/otp-login","policy_denials":["[redacted:otp] credential_delivered: page reads withheld until navigation"]}}
],"meta":{"request_id":"self-test"}}
JSON
  summarize_receipts "$good" "$s"
  ev_check "self-test: one fill receipt" "1" "$(json_field "$s" fill_receipts)"
  ev_check "self-test: the fill completed" "1" "$(json_field "$s" completed_fills)"
  ev_check "self-test: the approval grant is named" "true" "$(json_field "$s" approval_present)"
  ev_check "self-test: the action holds the id set only" "true" "$(json_field "$s" action_keys_ok)"
  ev_check "self-test: the redaction marker is present" "true" "$(json_field "$s" redacted_marker_present)"

  bad="$WORK/bad.json"
  cat >"$bad" <<'JSON'
{"data":[{"receipt_id":"r3","action":{"type":"fill_otp_ref","snapshot_id":"s1","generation":1,"ref_id":"e1","approval_grant_id":"g1","value":"x"},"outcome":{"status":"completed","latency_ms":5}}]}
JSON
  summarize_receipts "$bad" "$s"
  ev_check "self-test: a value key on the fill is refused" "false" "$(json_field "$s" action_keys_ok)"

  cat >"$WORK/rec-good.json" <<'JSON'
{"fixture_code":"864209","messages":[{"subject":"Your sign-in code","text":"Your verification code is 864209. It expires in 10 minutes.","age_seconds":60}]}
JSON
  cat >"$WORK/rec-no-keyword.json" <<'JSON'
{"fixture_code":"864209","messages":[{"subject":"Your order","text":"Order number 864209 has shipped."}]}
JSON
  cat >"$WORK/rec-first-run.json" <<'JSON'
{"fixture_code":"864209","messages":[{"subject":"Code","text":"Order 1234 and verification code 864209"}]}
JSON
  validates() {
    otp_node | MODE=validate MAILBOX_RECORDING="$(node_path "$1")" node - >/dev/null 2>&1
  }
  if validates "$WORK/rec-good.json"; then
    ev_pass "self-test: a good recording validates" "yes"
  else
    ev_fail "self-test: a good recording validates" "no"
  fi
  if validates "$WORK/rec-no-keyword.json"; then
    ev_fail "self-test: a code with no OTP keyword is refused" "accepted"
  else
    ev_pass "self-test: a code with no OTP keyword is refused" "refused"
  fi
  if validates "$WORK/rec-first-run.json"; then
    ev_fail "self-test: a first digit run that is not the code is refused" "accepted"
  else
    ev_pass "self-test: a first digit run that is not the code is refused" "refused"
  fi
  printed=$(otp_node | MODE=validate MAILBOX_RECORDING="$(node_path "$WORK/rec-good.json")" node - 2>&1) || true
  ev_check "self-test: validate prints no code digit" "0" "$(printf '%s' "$printed" | grep -c -F -w "$CODE" || true)"
  ev_summary
}

case "${1:-}" in
  --self-test)
    self_test
    ;;
  --validate-recording)
    ev_require node
    : "${MAILBOX_RECORDING:?set MAILBOX_RECORDING to the recording JSON}"
    otp_node | MODE=validate MAILBOX_RECORDING="$(node_path "$MAILBOX_RECORDING")" node -
    ;;
  --serve-mailbox)
    ev_require node
    : "${MAILBOX_RECORDING:?set MAILBOX_RECORDING to the recording JSON}"
    : "${MAILBOX_LOG:?set MAILBOX_LOG to a fresh access-log path for this run}"
    otp_node | MODE=mailbox \
      MAILBOX_RECORDING="$(node_path "$MAILBOX_RECORDING")" MAILBOX_LOG="$(node_path "$MAILBOX_LOG")" \
      MAILBOX_PORT="${MAILBOX_PORT:-18080}" MAILBOX_BIND="${MAILBOX_BIND:-127.0.0.1}" node -
    ;;
  --serve-fixture)
    ev_require node
    : "${MAILBOX_RECORDING:?set MAILBOX_RECORDING to the recording JSON}"
    : "${FIXTURE_LOG:?set FIXTURE_LOG to a fresh access-log path for this run}"
    : "${FIXTURE_TLS_KEY:?set FIXTURE_TLS_KEY to the dev TLS key, kept outside the repo}"
    : "${FIXTURE_TLS_CERT:?set FIXTURE_TLS_CERT to the dev certificate}"
    otp_node | MODE=fixture \
      MAILBOX_RECORDING="$(node_path "$MAILBOX_RECORDING")" FIXTURE_LOG="$(node_path "$FIXTURE_LOG")" \
      FIXTURE_TLS_KEY="$(node_path "$FIXTURE_TLS_KEY")" FIXTURE_TLS_CERT="$(node_path "$FIXTURE_TLS_CERT")" \
      FIXTURE_PORT="${FIXTURE_PORT:-18443}" FIXTURE_BIND="${FIXTURE_BIND:-127.0.0.1}" node -
    ;;
  "")
    verify_run
    ;;
  *)
    echo "usage: $0 [--self-test | --validate-recording | --serve-mailbox | --serve-fixture]" >&2
    exit 2
    ;;
esac
