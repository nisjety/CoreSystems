#!/usr/bin/env bash
# Bring up the Model Plane core services in the nested daemon; auto-transfer any locally-built image compose reports missing.
set -u
ROOT="C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567"
SVCS="${*:-session-core model-gateway inference-core execution-core capability-core cost-core}"
for attempt in 1 2 3 4 5 6; do
  out=$(docker exec space-proof-dind bash -c "cd '/src/apps/Model Plane' && export SOURCE_REVISION=w1 BUILD_DATE=2026-10-27T00:00:00Z MODEL_PLANE_DEV=1 && bash scripts/compose.sh --env-file '/src/apps/Control Plane/.env.generated-secrets' --env-file '/src/apps/Model Plane/deploy/.env.generated-secrets' --env-file '/src/apps/Model Plane/deploy/.env' --env-file '/src/apps/Model Plane/deploy/.env.proof-decision-key' up -d --no-build $SVCS 2>&1" | grep -v -i -E "password|secret|token|key|Pull complete|Extracting|Waiting|Download|Pulling")
  echo "$out" | tail -6
  missing=$(echo "$out" | grep -o "No such image: [^ ]*" | head -1 | sed 's/No such image: //')
  if [ -z "$missing" ]; then break; fi
  echo ">> transferring $missing"
  docker save "$missing" | docker exec -i space-proof-dind docker load | tail -1
done
