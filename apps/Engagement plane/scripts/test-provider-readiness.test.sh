#!/bin/sh
set -eu

# The temporary curl executable is this same test file. It never contacts a
# network; it checks the request contract before recording the requested URL.
if [ "${0##*/}" = curl ]; then
  header_file=
  endpoint=
  timeout=
  connect_timeout=
  method=
  while [ "$#" -gt 0 ]; do
    case "$1" in
      *synthetic-readiness-token*) echo "bearer leaked into curl arguments" >&2; exit 1 ;;
      --header) header_file=${2#@}; shift 2 ;;
      --max-time) timeout=$2; shift 2 ;;
      --connect-timeout) connect_timeout=$2; shift 2 ;;
      --request) method=$2; shift 2 ;;
      --url) endpoint=$2; shift 2 ;;
      --proto|--proto-redir) shift 2 ;;
      --location|-L) echo "readiness request followed redirects" >&2; exit 1 ;;
      --fail-with-body|--silent|--show-error) shift ;;
      http://*|https://*) endpoint=$1; shift ;;
      *) echo "unexpected curl argument" >&2; exit 1 ;;
    esac
  done
  test "$method" = POST
  test "$timeout" = 20
  test "$connect_timeout" = 5
  test -f "$header_file"
  test "$(stat -f '%Lp' "$header_file" 2>/dev/null || stat -c '%a' "$header_file")" = 600
  test "$(cat "$header_file")" = 'Authorization: Bearer synthetic-readiness-token'
  printf '%s\n' "$header_file" >> "$READINESS_TEST_HEADERS"
  printf '%s\n' "$endpoint" >> "$READINESS_TEST_CALLS"
  printf '%s\n' '{"success":true,"data":{"status":"ready"}}'
  exit 0
fi

root=$(CDPATH= cd "$(dirname "$0")/.." && pwd)
fixture_dir=$(mktemp -d)
cleanup() {
  rm -f "$fixture_dir/bin/curl" "$fixture_dir/bearer" "$fixture_dir/link" \
    "$fixture_dir/calls" "$fixture_dir/headers" "$fixture_dir/output"
  rmdir "$fixture_dir/bin" "$fixture_dir"
}
trap cleanup EXIT HUP INT TERM
mkdir "$fixture_dir/bin"
cp "$root/scripts/test-provider-readiness.test.sh" "$fixture_dir/bin/curl"
chmod 700 "$fixture_dir/bin/curl"
printf '%s\n' synthetic-readiness-token > "$fixture_dir/bearer"
chmod 600 "$fixture_dir/bearer"
export READINESS_TEST_CALLS="$fixture_dir/calls"
export READINESS_TEST_HEADERS="$fixture_dir/headers"

probe() {
  env PATH="$fixture_dir/bin:$PATH" \
    INTEGRATION_CORE_URL=http://localhost:3026 \
    INTEGRATION_BEARER_TOKEN= \
    INTEGRATION_BEARER_TOKEN_FILE="$fixture_dir/bearer" \
    GOOGLE_CONNECTION_ID= META_CONNECTION_ID= \
    "$@" sh "$root/scripts/test-provider-readiness.sh" > "$fixture_dir/output" 2>&1
}

must_refuse() {
  : > "$fixture_dir/calls"
  if probe "$@"; then
    echo "invalid readiness configuration accepted" >&2
    exit 1
  fi
  test ! -s "$fixture_dir/calls"
  if grep -q synthetic-readiness-token "$fixture_dir/output"; then
    echo "readiness error exposed bearer" >&2
    exit 1
  fi
}

probe GOOGLE_CONNECTION_ID=google-one
test "$(cat "$fixture_dir/calls")" = 'http://localhost:3026/api/v1/connections/google-one/readiness'
while IFS= read -r header_path; do test ! -e "$header_path"; done < "$fixture_dir/headers"
: > "$fixture_dir/calls"
probe META_CONNECTION_ID=meta-one
test "$(cat "$fixture_dir/calls")" = 'http://localhost:3026/api/v1/connections/meta-one/readiness'
chmod 400 "$fixture_dir/bearer"
probe GOOGLE_CONNECTION_ID=google-one
chmod 600 "$fixture_dir/bearer"
: > "$fixture_dir/calls"
probe GOOGLE_CONNECTION_ID=google-one META_CONNECTION_ID=meta-one
test "$(wc -l < "$fixture_dir/calls" | tr -d ' ')" = 2

# Preserve legacy shell-provided credentials, but never choose silently between
# them and the preferred mounted-file source.
: > "$fixture_dir/calls"
probe INTEGRATION_BEARER_TOKEN_FILE= INTEGRATION_BEARER_TOKEN=synthetic-readiness-token GOOGLE_CONNECTION_ID=google-one
test -s "$fixture_dir/calls"
must_refuse GOOGLE_CONNECTION_ID=google-one INTEGRATION_BEARER_TOKEN=synthetic-readiness-token
must_refuse
must_refuse GOOGLE_CONNECTION_ID=google/unsafe META_CONNECTION_ID=meta-one
must_refuse GOOGLE_CONNECTION_ID=..
must_refuse GOOGLE_CONNECTION_ID=google-one META_CONNECTION_ID=meta/unsafe
must_refuse GOOGLE_CONNECTION_ID=google-one INTEGRATION_BEARER_TOKEN_FILE="$fixture_dir/missing"
must_refuse GOOGLE_CONNECTION_ID=google-one INTEGRATION_BEARER_TOKEN_FILE="$fixture_dir/bin"
must_refuse GOOGLE_CONNECTION_ID=google-one INTEGRATION_BEARER_TOKEN_FILE= INTEGRATION_BEARER_TOKEN='contains space'
ln -s "$fixture_dir/bearer" "$fixture_dir/link"
must_refuse GOOGLE_CONNECTION_ID=google-one INTEGRATION_BEARER_TOKEN_FILE="$fixture_dir/link"
chmod 644 "$fixture_dir/bearer"
must_refuse GOOGLE_CONNECTION_ID=google-one
chmod 600 "$fixture_dir/bearer"
for bad_token in '' 'contains space' 'line1
line2'; do
  printf '%s' "$bad_token" > "$fixture_dir/bearer"
  must_refuse GOOGLE_CONNECTION_ID=google-one
done
awk 'BEGIN {for (i=0; i<8193; i++) printf "a"}' > "$fixture_dir/bearer"
must_refuse GOOGLE_CONNECTION_ID=google-one
printf '%s\n' synthetic-readiness-token > "$fixture_dir/bearer"

for bad_url in 'http://integration.example' 'https://user@integration.example' \
  'https://integration.example/path' 'https://integration.example?query=value' \
  'https://integration.example#fragment' 'http://localhost:3026@evil.example' \
  'http://localhost:invalid' 'https://' 'https://integration.example:70000' \
  'https://integration.example
other.example'; do
  must_refuse GOOGLE_CONNECTION_ID=google-one INTEGRATION_CORE_URL="$bad_url"
done

probe GOOGLE_CONNECTION_ID=google-one INTEGRATION_CORE_URL=https://integration.example/
probe META_CONNECTION_ID=meta-one INTEGRATION_CORE_URL='http://[::1]:3026'
echo "provider readiness file credential and single-provider contract passed"
