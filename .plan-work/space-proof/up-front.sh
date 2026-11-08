#!/usr/bin/env bash
# Bring up the Frontend plane (verevon gateway-rs + SPA nginx + verevon-nats) in the nested daemon; transfer missing images from the host.
set -u
SVCS="${*:-nats gateway frontend}"
for attempt in 1 2 3 4 5 6; do
  out=$(docker exec space-proof-dind bash -c "cd '/src/apps/Frontend Plane/verevonv3' && export SOURCE_REVISION=w1 BUILD_DATE=2026-10-27T00:00:00Z VEREVON_BIND_ADDRESS=0.0.0.0 && docker compose --env-file .env --env-file '/src/apps/Application Plane/.env' --env-file '/src/apps/Application Plane/convex-core/.env' --env-file '/src/apps/Application Plane/convex-core/.env.local' --env-file '/src/apps/Application Plane/.env.generated-secrets' --env-file '/src/apps/Model Plane/deploy/.env.generated-secrets' --env-file '/src/apps/Control Plane/.env.generated-secrets' -f docker-compose.yml up -d --no-build $SVCS 2>&1" | grep -v -i -E "password|secret|token|key|Pull complete|Extracting|Waiting|Download|Pulling")
  echo "$out" | tail -10
  missing=$(echo "$out" | grep -o "No such image: [^ ]*" | head -1 | sed 's/No such image: //')
  pullfail=$(echo "$out" | grep -o "pull access denied for [^,]*" | head -1 | sed 's/pull access denied for //')
  if [ -n "$missing" ]; then img="$missing"; elif [ -n "$pullfail" ]; then img="$pullfail"; else break; fi
  echo ">> transferring $img"
  docker save "$img" | docker exec -i space-proof-dind docker load | tail -1
done
