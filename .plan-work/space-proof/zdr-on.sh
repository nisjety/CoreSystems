#!/usr/bin/env bash
# Switch ZDR on for the seeded ZDR-TEST org Z INSIDE the nested daemon only (private stack). Prints no credential.
docker exec space-proof-dind bash -c '
set -u
cd "/src/apps/Frontend Plane/verevonv3"
sec() { grep -m1 "^$1=" "/src/apps/Control Plane/.env.generated-secrets" | cut -d= -f2- | tr -d "\r\"\047"; }
ZORG=$(python3 -c "import json;print(json.load(open(\"tests/e2e/.auth/space-e2e-seed.json\"))[\"orgs\"][\"Z\"][\"id\"])")
FEMAIL=$(python3 -c "import json;print(json.load(open(\"tests/e2e/.auth/space-e2e-seed.json\"))[\"users\"][\"F\"][\"email\"])")
FID=$(docker exec controlplane-postgres sh -c "psql -U \"\$POSTGRES_USER\" -d auth_service -Atc \"select id from \\\"user\\\" where email = \x27$FEMAIL\x27\"")
echo "org Z=$ZORG F user id resolved: $([ -n "$FID" ] && echo yes || echo no)"
export ORG_CORE_SERVICE_TOKEN="$(sec GATEWAY_ORG_CORE_SERVICE_TOKEN)"
node apps/gateway/scripts/zdr-seed-test-org.mjs --base-url http://127.0.0.1:18080 --org-id "$ZORG" --user-id "$FID" --expect-name "ZDR-TEST space e2e" '"$*"' 2>&1 | sed -E "s/(token|secret|password|key|signature)[=:] *[^ ]+/\1=<redacted>/Ig" | tail -20
'
