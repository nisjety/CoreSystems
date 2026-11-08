#!/usr/bin/env bash
# e2e-scrape-capture.sh: evidence flows for scrape captures and actions (WS13.8).
#
# Opt-in, not part of ci-test. Run it with `make test-scrape-capture` (in apps/Ingestion Plane)
# against a running stack. Written in W2-U56 and NOT run there (bash -n only). It asserts the
# wire shape CONTRACTS.md section 10 describes; the first failed check stops the run, non-zero.
#
# Needs:
#   E2E_FIXTURE_URL      base URL of the fixture pages, as the EDGE container reaches them
#                        (for example http://host.docker.internal:8099 on Docker Desktop)
#   E2E_TOKEN            a Quarry bearer token, or
#   E2E_COOKIE_JAR       a gateway session cookie jar, with E2E_GATEWAY (default http://localhost:3011);
#                        the token is minted with GET /api/quarry/token
# Optional:
#   E2E_EDGE             the edge base (default http://127.0.0.1:8082)
#   E2E_EXPECT_STITCH=1  the edge runs with QUARRY_EDGE__SCRAPE_STITCH=on (tall-page check; the
#                        tall PNG must fit the ZDR inline cap, raise QUARRY_EDGE__SCRAPE_CAPTURE_INLINE_MAX_BYTES)
#   E2E_START_FIXTURE=1  serve the fixtures from a throwaway nginx container (docker; run it under the docker lock)
#   E2E_FIXTURE_PORT     host port for that container (default 8099)
# Edge settings: QUARRY_EDGE__SCRAPE_CAPTURE=on and QUARRY_EDGE__SCRAPE_ACTIONS=user (dev only).

set -euo pipefail

EDGE="${E2E_EDGE:-http://127.0.0.1:8082}"
GATEWAY="${E2E_GATEWAY:-http://localhost:3011}"
WORK="$(mktemp -d)"
FIXDIR="${E2E_FIXTURE_DIR:-$WORK/fixtures}"
CONTAINER="quarry-e2e-fixture-$$"

cleanup() {
  if [ "${E2E_START_FIXTURE:-0}" = 1 ]; then
    docker stop "$CONTAINER" >/dev/null 2>&1 || true
  fi
  rm -rf "$WORK"
}
trap cleanup EXIT

checks=0
fail() { echo "FAIL: $*" >&2; exit 1; }
pass() { checks=$((checks + 1)); echo "ok   $*"; }

for tool in curl jq xxd base64; do
  command -v "$tool" >/dev/null 2>&1 || fail "missing tool: $tool"
done

# Fixtures: an article and a tall page (9000 CSS px, so a full-page screenshot needs stitching).
mkdir -p "$FIXDIR"
cat > "$FIXDIR/article.html" <<'HTML'
<!doctype html><html><head><title>Article</title></head><body><main>
<h1>Fixture article</h1>
<p>Quarry scrape capture fixture. This paragraph gives the main-content heuristic some text to keep.</p>
<p>A second paragraph, with a few more words, so the markdown is not empty after extraction.</p>
</main></body></html>
HTML
cat > "$FIXDIR/tall.html" <<'HTML'
<!doctype html><html><head><title>Tall</title></head>
<body style="margin:0"><main><h1>Tall fixture</h1>
<div style="height:9000px;background:linear-gradient(#fff,#06c)"><p>Top of the tall page.</p></div>
<p>Bottom of the tall page.</p></main></body></html>
HTML

if [ "${E2E_START_FIXTURE:-0}" = 1 ]; then
  command -v docker >/dev/null 2>&1 || fail "E2E_START_FIXTURE=1 needs docker"
  docker run --rm -d --name "$CONTAINER" -p "${E2E_FIXTURE_PORT:-8099}:80" \
    -v "$FIXDIR:/usr/share/nginx/html:ro" nginx:1.27-alpine >/dev/null
  sleep 2
fi

: "${E2E_FIXTURE_URL:?set E2E_FIXTURE_URL to the fixture base, as the edge container reaches it}"
FIXTURE_BASE="${E2E_FIXTURE_URL%/}"

TOKEN="${E2E_TOKEN:-}"
if [ -z "$TOKEN" ]; then
  [ -n "${E2E_COOKIE_JAR:-}" ] || fail "set E2E_TOKEN, or E2E_COOKIE_JAR for a gateway session"
  # The token field name is read from the response; a different shape fails the check below.
  TOKEN="$(curl -fsS -b "$E2E_COOKIE_JAR" "$GATEWAY/api/quarry/token" | jq -r '.token // .access_token // empty')"
  [ -n "$TOKEN" ] || fail "no token from $GATEWAY/api/quarry/token (check the session and the response shape)"
fi

# req <page> <json object of extra fields>: the request body for a fixture page.
req() {
  jq -cn --arg url "$FIXTURE_BASE/$1" --argjson extra "$2" '{url: $url} + $extra'
}

# scrape <body> <out-file>: POST /v1/scrape, prints the HTTP status ("000" when unreachable).
scrape() {
  curl -sS -o "$2" -w '%{http_code}' --max-time 130 -X POST "$EDGE/v1/scrape" \
    -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' --data "$1" || true
}

# 1. Baseline: markdown inline under ZDR (no capture, no actions).
code="$(scrape "$(req article.html '{"formats":["markdown"],"zdr":true}')" "$WORK/base.json")"
[ "$code" = 200 ] || fail "baseline scrape answered $code (edge reachable? token valid?)"
jq -e '(.data.markdown // "") | length > 0' "$WORK/base.json" >/dev/null \
  || fail "baseline has no inline markdown under ZDR"
pass "baseline: markdown inline under ZDR"

# 2. Screenshot: a PNG inline under ZDR.
code="$(scrape "$(req article.html '{"formats":["screenshot"],"zdr":true}')" "$WORK/png.json")"
[ "$code" = 200 ] || fail "screenshot scrape answered $code (is QUARRY_EDGE__SCRAPE_CAPTURE=on?)"
jq -er '.data.screenshot.data_base64' "$WORK/png.json" | base64 -d > "$WORK/shot.png" \
  || fail "no inline screenshot in the response"
[ "$(xxd -p -l 8 "$WORK/shot.png")" = "89504e470d0a1a0a" ] || fail "screenshot is not a PNG"
pass "screenshot: inline PNG under ZDR"

# 3. PDF: a PDF inline under ZDR.
code="$(scrape "$(req article.html '{"formats":[{"type":"pdf"}],"zdr":true}')" "$WORK/pdf.json")"
[ "$code" = 200 ] || fail "pdf scrape answered $code"
jq -er '.data.pdf.data_base64' "$WORK/pdf.json" | base64 -d > "$WORK/out.pdf" \
  || fail "no inline pdf in the response"
[ "$(head -c 4 "$WORK/out.pdf")" = "%PDF" ] || fail "pdf is not a PDF"
pass "pdf: inline PDF under ZDR"

# 4. Tall page: a stitched screenshot above one capture's 4096 CSS px (only with the stitch switch on).
if [ "${E2E_EXPECT_STITCH:-0}" = 1 ]; then
  code="$(scrape "$(req tall.html '{"formats":["screenshot"],"zdr":true}')" "$WORK/tall.json")"
  [ "$code" = 200 ] || fail "tall screenshot answered $code"
  jq -er '.data.screenshot.data_base64' "$WORK/tall.json" | base64 -d > "$WORK/tall.png" \
    || fail "no inline tall screenshot (over the ZDR inline cap?)"
  # PNG IHDR: width at bytes 16 to 19, height at bytes 20 to 23, big-endian.
  height=$((16#$(xxd -p -s 20 -l 4 "$WORK/tall.png")))
  [ "$height" -gt 4096 ] || fail "tall screenshot is $height px high; expected more than 4096 (stitch off?)"
  pass "stitch: tall page is stitched ($height px high)"
else
  echo "skip stitch: E2E_EXPECT_STITCH is not 1"
fi

# 5. Actions: a wait and a screenshot are accepted by a user token and echoed in the summary.
code="$(scrape "$(req article.html '{"actions":[{"type":"wait","milliseconds":100},{"type":"screenshot"}],"zdr":true}')" "$WORK/act.json")"
[ "$code" = 200 ] || fail "actions scrape answered $code (is QUARRY_EDGE__SCRAPE_ACTIONS=user?)"
jq -e '.data.effective_options.actions.count == 2' "$WORK/act.json" >/dev/null \
  || fail "effective_options.actions.count is not 2"
pass "actions: wait and screenshot accepted, summary echoed"

# 6. A denied type is refused by name.
code="$(scrape "$(req article.html '{"actions":[{"type":"evaluate"}]}')" "$WORK/denied.json")"
[ "$code" = 400 ] || fail "denied action answered $code, expected 400"
grep -q 'action_denied' "$WORK/denied.json" || fail "denied action body does not name action_denied"
pass "denied: evaluate refused by name (action_denied)"

# 7. A request labelled initiated_by agent is refused for actions, whatever the token.
code="$(scrape "$(req article.html '{"actions":[{"type":"wait","milliseconds":10}],"initiated_by":"agent"}')" "$WORK/agent.json")"
[ "$code" = 403 ] || fail "agent-labelled actions answered $code, expected 403"
grep -q 'scrape_interact_scope_required' "$WORK/agent.json" \
  || fail "agent-labelled refusal does not carry scrape_interact_scope_required"
pass "agent label: actions refused (scrape_interact_scope_required)"

# 8. The stream route refuses a capture (it answers 200 with an error event in the stream).
code="$(curl -sS -o "$WORK/stream.txt" -w '%{http_code}' --max-time 60 -X POST "$EDGE/v1/scrape/stream" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  --data "$(req article.html '{"formats":["screenshot"]}')" || true)"
grep -q 'unsupported_in_stream' "$WORK/stream.txt" \
  || fail "stream did not refuse the capture with unsupported_in_stream (HTTP $code)"
pass "stream: capture refused with unsupported_in_stream"

echo "all $checks checks passed"
