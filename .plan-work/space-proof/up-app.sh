#!/usr/bin/env bash
# Bring up Application Plane services in the nested daemon; auto-transfer missing locally-built images from the host.
set -u
SVCS="${*:-convex-backend convex-gateway convex-subscriber notification-core}"
for attempt in 1 2 3 4 5 6 7 8; do
  out=$(docker exec space-proof-dind bash -c "cd '/src/apps/Application Plane' && export SOURCE_REVISION=w1 BUILD_DATE=2026-10-27T00:00:00Z && bash scripts/run-application-plane.sh up -d --no-build $SVCS 2>&1" | grep -v -i -E "password|secret|token|key|Pull complete|Extracting|Waiting|Download|Pulling")
  echo "$out" | tail -8
  missing=$(echo "$out" | grep -o "No such image: [^ ]*" | head -1 | sed 's/No such image: //')
  pullfail=$(echo "$out" | grep -o "pull access denied for [^,]*" | head -1 | sed 's/pull access denied for //')
  if [ -n "$missing" ]; then img="$missing"; elif [ -n "$pullfail" ]; then img="$pullfail"; else break; fi
  echo ">> transferring $img"
  docker save "$img" | docker exec -i space-proof-dind docker load | tail -1
done
