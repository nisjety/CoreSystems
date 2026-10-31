#!/usr/bin/env bash
# Shared guard for gateway-cache-forensics.sh and gateway-cache-purge.sh (WS4.B).
#
# Both scripts read (forensics) or delete (purge) keys in a Redis that, on a real
# deployment, is shared with rate-limit and session-validation keys. They therefore
# refuse a target that does not look throwaway unless the operator passes
# --allow-non-throwaway. The operator action on real gateways (runbook steps 1 and 4)
# is exactly that: an explicit, deliberate flag.
#
# A target counts as throwaway when REDIS_CLI is
#   * `docker ... exec ... <container> redis-cli ...` and <container> starts with one of
#     THROWAWAY_CONTAINER_PREFIXES (default: "scratch- scratch_ throwaway- sbs- test-"), or
#   * a direct redis-cli whose host (-h, --host, or the host of -u/--uri; default
#     127.0.0.1) is loopback or listed in THROWAWAY_HOSTS (space separated, for a
#     side-by-side stack's private network name).
# Loopback is accepted because the scratch Redis recipes publish a loopback port; it is
# still on the operator not to point REDIS_CLI at the live stack's published port.
#
# This file only defines functions; source it.

gateway_cache_throwaway_reason() {
  local cli=$1
  local -a tok
  read -r -a tok <<<"$cli"
  local prefixes=${THROWAWAY_CONTAINER_PREFIXES:-"scratch- scratch_ throwaway- sbs- test-"}
  local i n=${#tok[@]} base is_docker=0
  for ((i = 0; i < n; i++)); do
    base=${tok[$i]##*/}
    base=${base%.exe}
    [ "$base" = "docker" ] && is_docker=1
  done

  if [ "$is_docker" -eq 1 ]; then
    local container=""
    for ((i = 1; i < n; i++)); do
      base=${tok[$i]##*/}
      base=${base%.exe}
      if [ "$base" = "redis-cli" ]; then container=${tok[$((i - 1))]}; break; fi
    done
    if [ -z "$container" ]; then
      echo "docker command without a recognisable '<container> redis-cli'"
      return 1
    fi
    local p
    for p in $prefixes; do
      case "$container" in "$p"*) echo "container '$container' has throwaway prefix '$p'"; return 0 ;; esac
    done
    echo "container '$container' does not start with any of: $prefixes"
    return 1
  fi

  local host="" j
  for ((i = 0; i < n; i++)); do
    case "${tok[$i]}" in
      -h | --host) j=$((i + 1)); host=${tok[$j]:-} ;;
      -u | --uri)
        j=$((i + 1))
        host=${tok[$j]:-}
        host=${host#*://}   # scheme
        host=${host##*@}    # userinfo
        host=${host%%/*}    # path
        if [[ $host == \[* ]]; then host=${host%%]*}]; else host=${host%%:*}; fi
        ;;
    esac
  done
  [ -z "$host" ] && host=127.0.0.1
  case "$host" in
    localhost | 127.* | ::1 | "[::1]") echo "loopback host '$host'"; return 0 ;;
  esac
  local allowed
  for allowed in ${THROWAWAY_HOSTS:-}; do
    [ "$host" = "$allowed" ] && { echo "host '$host' is listed in THROWAWAY_HOSTS"; return 0; }
  done
  echo "host '$host' is not loopback and not in THROWAWAY_HOSTS"
  return 1
}

# gateway_cache_require_throwaway <redis-cli command> <allow: 0|1>
# Returns 0 to proceed, 2 to refuse. Messages go to stderr and never include the
# command line (it can carry a password).
gateway_cache_require_throwaway() {
  local reason
  if reason=$(gateway_cache_throwaway_reason "$1"); then
    echo "target accepted as throwaway: ${reason}" >&2
    return 0
  fi
  if [ "${2:-0}" = "1" ]; then
    echo "WARNING: ${reason}; continuing only because --allow-non-throwaway was given." >&2
    echo "         This Redis may be shared (rate-limit and session keys live next to the cache keys)." >&2
    return 0
  fi
  echo "refusing: ${reason}." >&2
  echo "          This script is meant for a scratch or side-by-side Redis. To run it against a real" >&2
  echo "          Redis (runbook steps 1 and 4, operator only) pass --allow-non-throwaway explicitly." >&2
  return 2
}
