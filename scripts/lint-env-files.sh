#!/usr/bin/env bash
# G51 (verevon-gap.md §8.33) — lint `.env` vs `.env.docker` for drift.
#
# Background: three production-impacting incidents have been traced to the
# same pattern (§8.23 AUTH_CORE_INTERNAL_API_KEY, §8.29 billing-core
# INTERNAL_API_KEY, §8.33 MICROSOFT_CLIENT_SECRET): a developer edits
# `.env` thinking that's what compose reads, but compose actually uses
# `.env.docker`. The two files drift until something breaks at runtime.
#
# This script walks every `apps/*/.../` directory that contains both files
# and:
#   1. Flags KEYS that exist in one file but not the other ("forgot to
#      mirror") — always a violation.
#   2. Flags KEYS whose value differs between the two files, EXCEPT for
#      host- and URL-shaped keys where divergence is expected and correct
#      (`localhost:5432` vs `controlplane-postgres:5432`, etc.).
#
# Exit codes:
#   0 — clean.
#   1 — violations found (prints details).
#   2 — script invocation error (bad arguments / not in a checkout).
#
# Override:
#   SKIP_ENV_LINT=1  → script does nothing and exits 0 (CI escape hatch
#                       for intentional drift during a multi-step roll-out).
#   ENV_LINT_DEBUG=1 → verbose per-pair output for troubleshooting.

set -uo pipefail

if [[ "${SKIP_ENV_LINT:-0}" == "1" ]]; then
  echo "lint-env-files: SKIP_ENV_LINT=1 set; skipping."
  exit 0
fi

# Locate monorepo root. Script lives at <root>/scripts/lint-env-files.sh,
# so going one level up is sufficient regardless of where the caller cd'd.
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APPS_DIR="$ROOT_DIR/apps"

if [[ ! -d "$APPS_DIR" ]]; then
  echo "lint-env-files: cannot find apps/ under $ROOT_DIR" >&2
  exit 2
fi

# Keys whose value divergence between .env and .env.docker is EXPECTED.
# Anything matching one of these patterns is allowed to differ; we still
# flag missing-in-one-side so the contract stays in sync.
#
# Rationale per pattern:
#   *_URL / *_HOST / *_PORT — container-vs-host hostnames; the whole point
#     of having two files is to swap these.
#   NEXT_PUBLIC_* — browser-side URL; almost always host-shaped.
#   PASSKEY_RP_ID / PASSKEY_ORIGIN — same reason: localhost vs prod domain.
#   CONVEX_* URLs — different scheme/host between Convex Cloud + self-hosted.
#   BETTER_AUTH_URL / FRONTEND_URL — outer browser-facing origin.
#
# To add a new exemption: append to the array with a one-line comment
# explaining WHY a divergence is legitimate. Do not blanket-exempt
# secret-shaped keys (those are the whole point of this lint).
EXEMPT_PATTERNS=(
  # URLs and host-shaped keys
  '.*_URL$'
  '.*_HOST$'
  '.*_PORT$'
  '.*_GRPC_URL$'
  '.*_BACKEND_URL$'
  '.*_HTTP_URL$'
  '.*_SERVICE_URL$'
  '.*_ADDR$'
  '.*_ADDRESS$'
  '.*_ENDPOINT$'
  '.*_SEEDS$'
  '^DATABASE_URL$'
  '^REDIS_URL$'
  '^NATS_URL$'
  '^BETTER_AUTH_URL$'
  '^FRONTEND_URL$'
  '^PASSKEY_RP_ID$'
  '^PASSKEY_ORIGIN$'
  '^CONVEX_SELF_HOSTED_URL$'
  '^CONVEX_AUTH_ISSUER$'
  '^CONVEX_AUTH_JWKS_URL$'
  '^CONVEX_BACKEND_URL$'
  '^NEXT_PUBLIC_.*$'
  # Intentional dev convenience: local dev usually skips email verification
  # so testers can sign up with throwaway addresses. Docker compose runs
  # the integration-test surface where email verification is required.
  '^REQUIRE_EMAIL_VERIFICATION$'
)

is_exempt() {
  local key="$1"
  for pat in "${EXEMPT_PATTERNS[@]}"; do
    if [[ "$key" =~ $pat ]]; then
      return 0
    fi
  done
  return 1
}

# Parse one env file into a sorted "KEY=VALUE" stream. Ignores blank lines
# and lines starting with `#`. Inline `# trailing comments` are stripped.
# Leading/trailing whitespace around the value is preserved (env-loaders
# do not strip), but surrounding quotes ARE stripped so we don't false-
# alarm on `FOO="bar"` vs `FOO=bar`.
parse_env() {
  local file="$1"
  awk '
    /^[[:space:]]*#/ { next }
    /^[[:space:]]*$/  { next }
    /=/ {
      # split on first =
      eq = index($0, "=")
      key = substr($0, 1, eq - 1)
      val = substr($0, eq + 1)
      # trim leading whitespace from key
      sub(/^[[:space:]]+/, "", key)
      sub(/[[:space:]]+$/, "", key)
      # strip trailing inline comment (only if preceded by whitespace +
      # `#`; avoids breaking values that legitimately contain `#`)
      sub(/[[:space:]]+#.*$/, "", val)
      # strip matching surrounding quotes
      if (val ~ /^".*"$/) { val = substr(val, 2, length(val) - 2) }
      else if (val ~ /^\047.*\047$/) { val = substr(val, 2, length(val) - 2) }
      print key "=" val
    }
  ' "$file" | LC_ALL=C sort -u
}

# Friendly path relative to ROOT_DIR for display.
rel() {
  local p="$1"
  printf '%s' "${p#$ROOT_DIR/}"
}

violations=0
checked_pairs=0

# Collect every directory in apps/ that has BOTH .env AND .env.docker.
# Using `-print0` + `read` keeps spaces-in-paths working ("Control Plane").
while IFS= read -r -d '' env_file; do
  dir="$(dirname "$env_file")"
  docker_file="$dir/.env.docker"

  if [[ ! -f "$docker_file" ]]; then
    continue
  fi

  checked_pairs=$((checked_pairs + 1))

  env_parsed="$(parse_env "$env_file")"
  docker_parsed="$(parse_env "$docker_file")"

  env_keys="$(printf '%s\n' "$env_parsed" | awk -F= '{print $1}' | LC_ALL=C sort -u)"
  docker_keys="$(printf '%s\n' "$docker_parsed" | awk -F= '{print $1}' | LC_ALL=C sort -u)"

  only_env="$(comm -23 <(echo "$env_keys") <(echo "$docker_keys"))"
  only_docker="$(comm -13 <(echo "$env_keys") <(echo "$docker_keys"))"
  common_keys="$(comm -12 <(echo "$env_keys") <(echo "$docker_keys"))"

  pair_violations=0

  pair_report() {
    if (( pair_violations == 0 )); then
      printf '\n%s\n' "── $(rel "$dir") ──"
    fi
    pair_violations=$((pair_violations + 1))
    printf '  %s\n' "$*"
  }

  if [[ -n "$only_env" ]]; then
    while IFS= read -r k; do
      [[ -z "$k" ]] && continue
      pair_report "❌ MISSING in .env.docker: $k (present in .env)"
    done <<< "$only_env"
  fi

  if [[ -n "$only_docker" ]]; then
    while IFS= read -r k; do
      [[ -z "$k" ]] && continue
      pair_report "❌ MISSING in .env:        $k (present in .env.docker)"
    done <<< "$only_docker"
  fi

  if [[ -n "$common_keys" ]]; then
    while IFS= read -r k; do
      [[ -z "$k" ]] && continue
      env_val="$(printf '%s\n' "$env_parsed" | awk -F= -v key="$k" '$1 == key { sub("^[^=]*=", ""); print; exit }')"
      docker_val="$(printf '%s\n' "$docker_parsed" | awk -F= -v key="$k" '$1 == key { sub("^[^=]*=", ""); print; exit }')"
      if [[ "$env_val" != "$docker_val" ]]; then
        if is_exempt "$k"; then
          [[ "${ENV_LINT_DEBUG:-0}" == "1" ]] && pair_report "ℹ️  EXEMPT divergent: $k (host/URL-shaped key — divergence is expected)"
          continue
        fi
        # Don't leak full secret values; show only a fingerprint.
        env_fp="$(printf '%s' "$env_val" | head -c 6)…"
        docker_fp="$(printf '%s' "$docker_val" | head -c 6)…"
        pair_report "⚠️  VALUE DIFFERS:      $k  (env=\"$env_fp\"  docker=\"$docker_fp\")"
      fi
    done <<< "$common_keys"
  fi

  if (( pair_violations > 0 )); then
    violations=$((violations + pair_violations))
  fi
done < <(find "$APPS_DIR" -type f -name ".env" -not -path "*/node_modules/*" -print0 2>/dev/null)

echo
if (( violations == 0 )); then
  printf "✅ lint-env-files: checked %d pairs, no drift detected.\n" "$checked_pairs"
  exit 0
fi

printf "\n❌ lint-env-files: %d violations across %d pairs.\n" "$violations" "$checked_pairs"
printf "   Run with ENV_LINT_DEBUG=1 to see exempt divergences too.\n"
printf "   To intentionally bypass during a roll-out: SKIP_ENV_LINT=1 ./scripts/lint-env-files.sh\n"
printf "   See verevon-gap.md §8.33 G51 for context on the trap this prevents.\n"
exit 1
