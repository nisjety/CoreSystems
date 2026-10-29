#!/usr/bin/env bash
set -uo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT="$TEST_DIR/../build-verevon-services.sh"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

passes=0
failures=0

pass() {
  printf 'ok - %s\n' "$1"
  passes=$((passes + 1))
}

fail() {
  printf 'not ok - %s\n' "$1" >&2
  failures=$((failures + 1))
}

assert_contains() {
  local description="$1"
  local haystack="$2"
  local needle="$3"
  if [[ "$haystack" == *"$needle"* ]]; then
    pass "$description"
  else
    fail "$description (missing: $needle)"
  fi
}

assert_not_contains() {
  local description="$1"
  local haystack="$2"
  local needle="$3"
  if [[ "$haystack" != *"$needle"* ]]; then
    pass "$description"
  else
    fail "$description (unexpected: $needle)"
  fi
}

new_fixture() {
  local name="$1"
  local root="$TMP_DIR/$name"
  local compose_file
  mkdir -p "$root/bin" "$root/apps/Ingestion Plane/config/searxng"
  for compose_file in \
    "apps/Data Plane v2/docker-compose.yml" \
    "apps/Control Plane/docker-compose.yml" \
    "apps/Ingestion Plane/docker-compose.yml" \
    "apps/Model Plane/deploy/docker-compose.yml" \
    "apps/Application Plane/docker-compose.yml" \
    "apps/Frontend Plane/verevonv3/docker-compose.yml" \
    "apps/Control Plane/docker-compose.production.yml" \
    "apps/Ingestion Plane/docker-compose.production.yml" \
    "apps/Model Plane/deploy/docker-compose.production.yml" \
    "apps/Frontend Plane/verevonv3/docker-compose.production.yml"; do
    mkdir -p "$root/$(dirname "$compose_file")"
    printf 'services:\n  fixture:\n    environment:\n      TEST_SECRET: ${TEST_SECRET:?required}\n' > "$root/$compose_file"
  done
  cat > "$root/apps/Control Plane/docker-compose.production.yml" <<'YAML'
services:
  fixture:
    environment:
      TEST_SECRET: ${TEST_SECRET:?required}
      NODE_ENV: production
      BETTER_AUTH_URL: ${BETTER_AUTH_URL:?required in production}
      FRONTEND_URL: ${FRONTEND_URL:?required in production}
      BETTER_AUTH_TRUSTED_ORIGINS: ${BETTER_AUTH_TRUSTED_ORIGINS:?required in production}
YAML
  printf 'INTERNAL_API_KEY=%064d\n' 0 > "$root/apps/Control Plane/.env"
  printf 'search:\n  formats:\n    - html\n    - json\n' > "$root/apps/Ingestion Plane/config/searxng/settings.yml"
  : > "$root/docker-calls.log"

  cat > "$root/bin/docker" <<'DOCKER'
#!/usr/bin/env bash
set -u
printf '%s\n' "$*" >> "${DOCKER_CALL_LOG:?}"

if [[ "${1:-}" == "info" ]]; then
  exit "${DOCKER_INFO_EXIT_CODE:-0}"
fi

if [[ "${1:-}" == "compose" ]]; then
  case " $* " in
    *" config --services "*)
      printf '%s\n' postgres minio-init migrate quarry-edge quarry-control quarry-orchestrator searxng nats gateway frontend
      ;;
    *" config --format json "*)
      printf '%s\n' '{"name":"fixture","services":{"gateway":{"build":{}},"frontend":{"build":{}},"nats":{"image":"nats:latest"}}}'
      ;;
    *" config --quiet "*)
      ;;
    *" config "*)
      printf '%s\n' 'QUARRY_EDGE__SEARXNG_URL: http://searxng:8080'
      ;;
  esac
  exit 0
fi

if [[ "${1:-}" == "image" && "${2:-}" == "inspect" ]]; then
  [[ "${3:-}" == "${MISSING_IMAGE:-}" ]] && exit 1
  exit 0
fi

if [[ "${1:-}" == "network" && "${2:-}" == "inspect" ]]; then
  exit 1
fi

exit 0
DOCKER
  chmod +x "$root/bin/docker"
  printf '%s' "$root"
}

run_launcher() {
  local root="$1"
  shift
  CORE_ROOT_OVERRIDE="$root" \
    DOCKER_CALL_LOG="$root/docker-calls.log" \
    MIN_DOCKER_FREE_GB=0 \
    VERIFY_INTERNAL_KEY="${VERIFY_INTERNAL_KEY:-1}" \
    TEST_SECRET=fixture-secret \
    PATH="$root/bin:$PATH" \
    "$SCRIPT" "$@" 2>&1
}

test_dry_run_is_daemon_free() {
  local root output calls
  root="$(new_fixture dry-run)"
  output="$(run_launcher "$root" --dry-run)" || {
    fail "dry-run completes with static Compose validation"
    return
  }
  calls="$(<"$root/docker-calls.log")"
  assert_not_contains "dry-run does not inspect Docker networks" "$calls" "network inspect"
  assert_not_contains "dry-run does not call Docker daemon info" "$calls" "info"
  assert_contains "dry-run reports static completion" "$output" "Dry run complete"
}

test_env_file_is_not_executed() {
  local root sentinel
  root="$(new_fixture safe-env)"
  sentinel="$root/env-was-executed"
  printf 'UNTRUSTED=$(touch %s)\n' "$sentinel" > "$root/.env"

  run_launcher "$root" --dry-run >/dev/null || true
  if [[ ! -e "$sentinel" ]]; then
    pass "root .env is parsed as data, never executed as shell"
  else
    fail "root .env is parsed as data, never executed as shell"
  fi
}

test_from_selects_remaining_stacks() {
  local root output
  root="$(new_fixture from)"
  output="$(run_launcher "$root" --dry-run --from ingestion)" || {
    fail "--from accepts a plane alias"
    return
  }
  assert_not_contains "--from skips earlier Data Plane build" "$output" "Building and starting Data Plane v2"
  assert_not_contains "--from skips earlier Control Plane build" "$output" "Building and starting Control Plane"
  assert_contains "--from starts at the selected Ingestion Plane" "$output" "Building and starting Ingestion Plane"
  assert_contains "--from continues through Frontend" "$output" "Building and starting Frontend Plane Verevon v3"
}

test_control_starts_before_data() {
  local root output control_line data_line
  root="$(new_fixture dependency-order)"
  output="$(run_launcher "$root" --dry-run)" || {
    fail "dependency-order dry-run completes"
    return
  }
  control_line="$(grep -n 'Building and starting Control Plane' <<<"$output" | head -1 | cut -d: -f1)"
  data_line="$(grep -n 'Building and starting Data Plane v2' <<<"$output" | head -1 | cut -d: -f1)"
  if [[ -n "$control_line" && -n "$data_line" ]] && (( control_line < data_line )); then
    pass "Control Plane starts before Data services that require Auth Core JWKS"
  else
    fail "Control Plane starts before Data services that require Auth Core JWKS"
  fi
}

test_skip_build_uses_existing_images() {
  local root output
  root="$(new_fixture skip-build)"
  output="$(run_launcher "$root" --dry-run --from frontend --skip-build)" || {
    fail "--skip-build is accepted"
    return
  }
  assert_contains "--skip-build still starts selected services" "$output" "up -d --remove-orphans"
  assert_contains "--skip-build explicitly forbids implicit image builds" "$output" "--no-build"
  assert_not_contains "--skip-build omits Compose build" "$output" " --build"
}

test_daemon_preflight_fails_before_start() {
  local root output status calls
  root="$(new_fixture daemon)"
  set +e
  output="$(DOCKER_INFO_EXIT_CODE=1 run_launcher "$root" --from frontend --skip-build)"
  status=$?
  set -e
  calls="$(<"$root/docker-calls.log")"

  if (( status != 0 )); then
    pass "unavailable Docker daemon fails the launcher"
  else
    fail "unavailable Docker daemon fails the launcher"
  fi
  assert_contains "daemon failure has an actionable error" "$output" "Docker daemon is unavailable"
  assert_not_contains "daemon preflight runs before Compose up" "$calls" " up "
}

test_skip_build_preflights_all_images_before_start() {
  local root output status calls
  root="$(new_fixture missing-image)"
  set +e
  output="$(MISSING_IMAGE=fixture-frontend run_launcher "$root" --from frontend --skip-build)"
  status=$?
  set -e
  calls="$(<"$root/docker-calls.log")"
  if (( status != 0 )); then
    pass "--skip-build fails when a selected build image is absent"
  else
    fail "--skip-build fails when a selected build image is absent"
  fi
  assert_contains "missing-image error names the absent image" "$output" "fixture-frontend"
  assert_not_contains "missing-image preflight runs before Compose up" "$calls" " up "
}

test_compose_loads_root_and_plane_env_files() {
  local root calls
  root="$(new_fixture env-files)"
  printf 'ROOT_FALLBACK=value\n' > "$root/.env"
  printf 'PLANE_VALUE=value\n' > "$root/apps/Ingestion Plane/.env"

  run_launcher "$root" --dry-run --from ingestion >/dev/null || {
    fail "Compose env-file precedence fixture completes"
    return
  }
  calls="$(<"$root/docker-calls.log")"
  assert_contains "Compose receives the root fallback env file" "$calls" "--env-file $root/.env"
  assert_contains "Compose receives the selected plane env file last" "$calls" "--env-file $root/apps/Ingestion Plane/.env"
}

test_production_overrides_are_rendered() {
  local root calls
  root="$(new_fixture production)"
  BETTER_AUTH_URL=https://auth.example.test \
    FRONTEND_URL=https://app.example.test \
    BETTER_AUTH_TRUSTED_ORIGINS=https://app.example.test \
    run_launcher "$root" --dry-run --production >/dev/null || {
    fail "--production is accepted"
    return
  }
  calls="$(<"$root/docker-calls.log")"
  assert_contains "production validation layers the Control override" "$calls" \
    "-f apps/Control Plane/docker-compose.yml -f apps/Control Plane/docker-compose.production.yml"
  assert_contains "production validation layers the Ingestion override" "$calls" \
    "-f apps/Ingestion Plane/docker-compose.yml -f apps/Ingestion Plane/docker-compose.production.yml"
  assert_contains "production validation layers the Model override" "$calls" \
    "-f apps/Model Plane/deploy/docker-compose.yml -f apps/Model Plane/deploy/docker-compose.production.yml"
  assert_contains "production validation layers the Frontend override" "$calls" \
    "-f apps/Frontend Plane/verevonv3/docker-compose.yml -f apps/Frontend Plane/verevonv3/docker-compose.production.yml"
}

test_production_fails_when_overlay_is_missing() {
  local root output status
  root="$(new_fixture missing-production)"
  rm "$root/apps/Ingestion Plane/docker-compose.production.yml"
  set +e
  output="$(run_launcher "$root" --dry-run --production --from ingestion)"
  status=$?
  set -e
  if (( status != 0 )); then
    pass "production mode fails closed when a required overlay is missing"
  else
    fail "production mode fails closed when a required overlay is missing"
  fi
  assert_contains "missing production overlay error is actionable" "$output" "Required production Compose override is missing"
}

test_production_fails_when_control_overlay_is_missing() {
  local root output status
  root="$(new_fixture missing-control-production)"
  rm "$root/apps/Control Plane/docker-compose.production.yml"
  set +e
  output="$(BETTER_AUTH_URL=https://auth.example.test \
    FRONTEND_URL=https://app.example.test \
    BETTER_AUTH_TRUSTED_ORIGINS=https://app.example.test \
    run_launcher "$root" --dry-run --production)"
  status=$?
  set -e
  if (( status != 0 )); then
    pass "production mode fails closed when the Control overlay is missing"
  else
    fail "production mode fails closed when the Control overlay is missing"
  fi
  assert_contains "missing Control overlay error is actionable" "$output" \
    "apps/Control Plane/docker-compose.production.yml"
}

test_production_requires_auth_public_urls_and_origins() {
  local root output status
  root="$(new_fixture missing-production-auth-urls)"
  set +e
  output="$(run_launcher "$root" --dry-run --production)"
  status=$?
  set -e
  if (( status != 0 )); then
    pass "production mode fails closed without Auth Core public URLs and origins"
  else
    fail "production mode fails closed without Auth Core public URLs and origins"
  fi
  assert_contains "missing production auth configuration names the required key" "$output" \
    "Control Plane:BETTER_AUTH_URL"
}

test_compose_bootstrap_dry_run_is_daemon_free() {
  local root output calls
  root="$(new_fixture bootstrap-dry-run)"
  output="$(run_launcher "$root" --dry-run --compose-bootstrap)" || {
    fail "compose-bootstrap dry-run completes statically"
    return
  }
  calls="$(<"$root/docker-calls.log")"
  assert_not_contains "compose-bootstrap dry-run does not query Compose runtime state" "$calls" " ps "
  assert_not_contains "compose-bootstrap dry-run does not inspect containers" "$calls" "inspect"
}

test_production_resume_is_rejected() {
  local root output status
  root="$(new_fixture production-resume)"
  set +e
  output="$(run_launcher "$root" --production --resume)"
  status=$?
  set -e
  if (( status == 2 )); then
    pass "production and resume cannot preserve stale development containers"
  else
    fail "production and resume cannot preserve stale development containers"
  fi
  assert_contains "production/resume conflict explains the safe alternative" "$output" "cannot be combined"
}

test_production_skip_build_is_rejected() {
  local root output status
  root="$(new_fixture production-skip-build)"
  set +e
  output="$(run_launcher "$root" --production --skip-build)"
  status=$?
  set -e
  if (( status == 2 )); then
    pass "production cannot reuse an unverified development image"
  else
    fail "production cannot reuse an unverified development image"
  fi
  assert_contains "production/skip-build conflict requires a production rebuild" "$output" "must be rebuilt"
}

test_production_forces_internal_key_verification() {
  local root output status
  root="$(new_fixture production-key-check)"
  set +e
  output="$(VERIFY_INTERNAL_KEY=0 run_launcher "$root" --dry-run --production --from frontend)"
  status=$?
  set -e
  if (( status != 0 )); then
    pass "production cannot disable internal-key verification"
  else
    fail "production cannot disable internal-key verification"
  fi
  assert_contains "production key verification failure is actionable" "$output" "cannot disable internal-key verification"
}

test_resume_aliases_follow_stack_order() {
  local source
  source="$(<"$SCRIPT")"
  assert_contains "resume aliases preserve Control-before-Data order" "$source" \
    'local resume_aliases=(control data ingestion model application frontend)'
}

test_dry_run_is_daemon_free
test_env_file_is_not_executed
test_from_selects_remaining_stacks
test_control_starts_before_data
test_skip_build_uses_existing_images
test_daemon_preflight_fails_before_start
test_skip_build_preflights_all_images_before_start
test_compose_loads_root_and_plane_env_files
test_production_overrides_are_rendered
test_production_fails_when_overlay_is_missing
test_production_fails_when_control_overlay_is_missing
test_production_requires_auth_public_urls_and_origins
test_compose_bootstrap_dry_run_is_daemon_free
test_production_resume_is_rejected
test_production_skip_build_is_rejected
test_production_forces_internal_key_verification
test_resume_aliases_follow_stack_order

printf '\n%d passed, %d failed\n' "$passes" "$failures"
(( failures == 0 ))
