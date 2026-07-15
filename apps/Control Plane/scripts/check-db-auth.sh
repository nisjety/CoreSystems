#!/usr/bin/env bash
#
# check-db-auth.sh — surface Postgres authentication failures: the stale
# DB-password drift that silently breaks writes. A core created before a
# password change limps on its old pooled connections for reads (so it looks
# "up") but FATALs on every new connection — i.e. every write. This is exactly
# what broke create-org on 2026-06-18.
#
# Two independent signals:
#   1. DEEP /health on each DB-backed Control Plane core — can it authenticate
#      to Postgres RIGHT NOW? (/health round-trips the DB; 503 = it cannot.)
#   2. `FATAL: password authentication failed` in controlplane-postgres logs
#      over a recent window — catches ANY client, including ones without an
#      HTTP /health (migrators, one-shots, external tools).
#
# Exits non-zero when either signal is bad, so it can gate CI or drive an alert.
# Pair with a cron for continuous detection of post-startup drift, e.g.:
#   */5 * * * * "apps/Control Plane/scripts/check-db-auth.sh" 5m || <notify>
#
# Usage:  check-db-auth.sh [SINCE]        SINCE default 5m (e.g. 30m, 1h)
# Env:    PG_CONTAINER     (default controlplane-postgres)
#         DB_AUTH_CHECKS   (default "org-core:18080 user-core:3012 billing-core:3014")
#                          space-separated service:host-port pairs to probe.

set -uo pipefail

SINCE="${1:-${DB_AUTH_SINCE:-5m}}"
PG="${PG_CONTAINER:-controlplane-postgres}"
read -r -a CHECKS <<<"${DB_AUTH_CHECKS:-org-core:18080 user-core:3012 billing-core:3014}"

problems=0
say()  { printf '[db-auth] %s\n' "$*"; }
warn() { printf '[db-auth] %s\n' "$*" >&2; }

# ── Signal 1: deep /health per core (can it authenticate right now?) ──────────
for entry in "${CHECKS[@]}"; do
  svc="${entry%%:*}"
  port="${entry##*:}"
  code="$(curl -s -o /dev/null -w '%{http_code}' "http://localhost:${port}/health" 2>/dev/null)"
  [[ -z "$code" ]] && code="000"
  if [[ "$code" == "200" ]]; then
    say "$(printf '%-12s /health 200 — DB reachable + authenticated' "$svc")"
  else
    warn "$(printf '%-12s /health %s — DB unreachable/unauthenticated' "$svc" "$code")"
    problems=$((problems + 1))
  fi
done

# ── Signal 2: Postgres auth-failure log scan (catches all clients) ────────────
if docker ps --format '{{.Names}}' | grep -qx "$PG"; then
  fails="$(docker logs "$PG" --since "$SINCE" 2>&1 | grep -c 'password authentication failed' || true)"
  if [[ "${fails:-0}" -gt 0 ]]; then
    warn "ALERT: ${fails} Postgres auth failure(s) for the DB role in the last ${SINCE}."
    # Best-effort source → container mapping (only when log_connections is on:
    # the FATAL lines themselves carry no IP, the preceding 'connection received'
    # lines do). Harmless no-op otherwise.
    ips="$(docker logs "$PG" --since "$SINCE" 2>&1 | grep -oE 'host=[0-9.]+' | cut -d= -f2 | sort -u || true)"
    for ip in $ips; do
      name=""
      for c in $(docker ps --format '{{.Names}}'); do
        if docker inspect "$c" --format '{{range .NetworkSettings.Networks}}{{.IPAddress}} {{end}}' 2>/dev/null | grep -qw "$ip"; then
          name="$c"
          break
        fi
      done
      warn "  source ${ip} → ${name:-unknown container}"
    done
    problems=$((problems + 1))
  else
    say "No Postgres auth failures in the last ${SINCE}."
  fi
else
  warn "WARN: ${PG} not running; skipped Postgres log scan."
fi

# ── Verdict ───────────────────────────────────────────────────────────────────
if [[ "$problems" -gt 0 ]]; then
  warn ""
  warn "DB-auth problems detected — most likely a stale DB password in a running container."
  warn "Remediation: cd 'apps/Control Plane' && ./scripts/run-control-plane.sh up -d --force-recreate --no-deps <service>"
  warn "(the recreated service picks up the service-local database credential through the runner)"
  docker ps --filter health=unhealthy --format '  unhealthy: {{.Names}} ({{.Status}})' 2>/dev/null \
    | sed 's/^/[db-auth] /' >&2 || true
  exit 1
fi

say "All clear — every probed core authenticates and no recent auth failures."
exit 0
