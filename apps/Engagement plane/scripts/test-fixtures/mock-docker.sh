#!/bin/sh
set -eu

printf '%s\n' "$*" >> "$STUDIO_DOCKER_TEST_LOG"
if [ "${1:-}" = compose ]; then
  test -n "${STUDIO_SOCIAL_INTERNAL_API_KEY:-}"
  test -n "${STUDIO_LEADS_INTERNAL_API_KEY:-}"
  test -n "${STUDIO_MARKETING_INTERNAL_API_KEY:-}"
  test -n "${STUDIO_SOCIAL_NATS_PASSWORD:-}"
  test -n "${STUDIO_LEADS_NATS_PASSWORD:-}"
fi
if [ "${1:-}" = network ] && [ "${2:-}" = inspect ]; then
  test -f "$STUDIO_DOCKER_TEST_NETWORK"
elif [ "${1:-}" = network ] && [ "${2:-}" = create ]; then
  : > "$STUDIO_DOCKER_TEST_NETWORK"
fi
