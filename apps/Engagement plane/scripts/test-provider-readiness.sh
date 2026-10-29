#!/usr/bin/env sh
set -eu

: "${INTEGRATION_CORE_URL:?set the Integration Core URL}"

integration_url=${INTEGRATION_CORE_URL%/}
case "$integration_url" in
  https://*) curl_protocol=https ;;
  http://*) curl_protocol=http ;;
  *) printf '%s\n' "INTEGRATION_CORE_URL must use HTTPS or loopback HTTP" >&2; exit 2 ;;
esac
authority=${integration_url#*://}
if ! printf '%s\n' "$authority" | awk '
  NR != 1 {invalid = 1}
  /^[A-Za-z0-9][A-Za-z0-9.-]*(:[0-9]+)?$/ || /^\[[0-9A-Fa-f:]+\](:[0-9]+)?$/ {
    host = $0
    if (host ~ /^\[/) sub(/^\[[^]]+\]/, "", host)
    else sub(/^[^:]+/, "", host)
    if (host != "") {
      sub(/^:/, "", host)
      if (host + 0 < 1 || host + 0 > 65535) exit 1
    }
    valid = 1
  }
  END {if (!valid || invalid) exit 1}
'; then
  printf '%s\n' "INTEGRATION_CORE_URL must be an origin with a valid host and optional port" >&2
  exit 2
fi
if [ "$curl_protocol" = http ]; then
  case "$authority" in
    localhost|localhost:*|127.0.0.1|127.0.0.1:*|\[::1\]|\[::1\]:*) ;;
    *) printf '%s\n' "HTTP is allowed only for loopback development" >&2; exit 2 ;;
  esac
fi

google_connection_id=${GOOGLE_CONNECTION_ID:-}
meta_connection_id=${META_CONNECTION_ID:-}
if [ -z "$google_connection_id" ] && [ -z "$meta_connection_id" ]; then
  printf '%s\n' "Set GOOGLE_CONNECTION_ID or META_CONNECTION_ID for an active connection" >&2
  exit 2
fi
for connection_id in "$google_connection_id" "$meta_connection_id"; do
  case "$connection_id" in
    .|..|*[!A-Za-z0-9._-]*) printf '%s\n' "Connection IDs contain unsupported characters" >&2; exit 2 ;;
  esac
done

bearer_file=${INTEGRATION_BEARER_TOKEN_FILE:-}
bearer_token=${INTEGRATION_BEARER_TOKEN:-}
unset INTEGRATION_BEARER_TOKEN
if [ -n "$bearer_file" ]; then
  if [ -n "$bearer_token" ]; then
    printf '%s\n' "Use only INTEGRATION_BEARER_TOKEN_FILE, without INTEGRATION_BEARER_TOKEN" >&2
    exit 2
  fi
  if [ ! -f "$bearer_file" ] || [ -L "$bearer_file" ]; then
    printf '%s\n' "INTEGRATION_BEARER_TOKEN_FILE must be a regular protected file" >&2
    exit 2
  fi
  file_mode=$(stat -f '%Lp' "$bearer_file" 2>/dev/null || stat -c '%a' "$bearer_file")
  case "$file_mode" in
    400|600) ;;
    *) printf '%s\n' "INTEGRATION_BEARER_TOKEN_FILE must have mode 0400 or 0600" >&2; exit 2 ;;
  esac
  file_size=$(stat -f '%z' "$bearer_file" 2>/dev/null || stat -c '%s' "$bearer_file")
  if [ "$file_size" -gt 8192 ]; then
    printf '%s\n' "INTEGRATION_BEARER_TOKEN_FILE must contain at most 8 KiB" >&2
    exit 2
  fi
  bearer_token=$(head -c 8193 "$bearer_file")
fi
case "$bearer_token" in
  ""|*[!A-Za-z0-9._~+/=-]*)
    printf '%s\n' "Provide one verified org-scoped bearer in INTEGRATION_BEARER_TOKEN_FILE" >&2
    exit 2
    ;;
esac
if [ "${#bearer_token}" -gt 8192 ]; then
  printf '%s\n' "Integration bearer must contain at most 8 KiB" >&2
  exit 2
fi

umask 077
header_file=$(mktemp "${TMPDIR:-/tmp}/marketing-readiness-header.XXXXXX")
chmod 600 "$header_file"
cleanup() {
  rm -f "$header_file"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM
printf 'Authorization: Bearer %s\n' "$bearer_token" >"$header_file"
unset bearer_token

probe() {
  provider_label=$1
  connection_id=$2
  printf '%s\n' "Testing $provider_label read-only connection..."
  curl --fail-with-body --silent --show-error \
    --proto "=$curl_protocol" \
    --proto-redir "=$curl_protocol" \
    --connect-timeout 5 \
    --max-time 20 \
    --request POST \
    --header "@$header_file" \
    --url "$integration_url/api/v1/connections/$connection_id/readiness"
  printf '\n'
}

if [ -n "$google_connection_id" ]; then probe "Google Search Console" "$google_connection_id"; fi
if [ -n "$meta_connection_id" ]; then probe "Meta Ads" "$meta_connection_id"; fi
