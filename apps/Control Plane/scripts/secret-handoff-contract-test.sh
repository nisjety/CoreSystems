#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
compose="$root/docker-compose.production.yml"
auth_dockerfile="$root/auth-core/Dockerfile"
user_dockerfile="$root/user-core/Dockerfile"
auth_entrypoint="$root/auth-core/docker-entrypoint.sh"
user_entrypoint="$root/user-core/docker-entrypoint.sh"

assert_contains() {
  local file=$1
  local text=$2
  if ! grep -Fq -- "$text" "$file"; then
    printf 'secret handoff contract: %s must contain %s\n' "$file" "$text" >&2
    exit 1
  fi
}

for file in "$auth_dockerfile" "$user_dockerfile"; do
  assert_contains "$file" 'apk add'
  assert_contains "$file" 'su-exec'
done

for file in "$auth_entrypoint" "$user_entrypoint"; do
  assert_contains "$file" 'CONTROL_SECRET_HANDOFF_DONE'
  assert_contains "$file" '/sbin/su-exec appuser'
  assert_contains "$file" 'chmod 0600'
  assert_contains "$file" '/run/control-secrets'
done

auth_block=$(awk '/^  auth-core:/{found=1} found{print} /^  user-core:/{exit}' "$compose")
user_block=$(awk '/^  user-core:/{found=1} found{print} /^  org-core:/{exit}' "$compose")
grep -Fq '    user: root' <<<"$auth_block" || {
  printf 'secret handoff contract: auth-core must start as root for the bounded handoff\n' >&2
  exit 1
}
grep -Fq '    user: root' <<<"$user_block" || {
  printf 'secret handoff contract: user-core must start as root for the bounded handoff\n' >&2
  exit 1
}

printf 'PASS: production secret handoff contract\n'
