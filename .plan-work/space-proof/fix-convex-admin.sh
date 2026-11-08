#!/usr/bin/env bash
# Regenerate the Convex admin key from the PRIVATE backend instance and write it into the STAGING copy of the env file(s) only.
set -eu
AP="/c/dev/space-proof/src/apps/Application Plane"
KEY=$(docker exec space-proof-dind docker exec convex-backend ./generate_admin_key.sh 2>/dev/null | tr -d '\r' | grep -E '\|' | tail -n 1 | tr -d '[:space:]')
[ -n "$KEY" ] || { echo "no key generated"; exit 1; }
for f in "$AP/convex-core/.env.local" "$AP/convex-core/.env" "$AP/.env" "$AP/.env.generated-secrets"; do
  if [ -f "$f" ] && grep -q "^CONVEX_ADMIN_KEY=" "$f"; then
    python - "$f" "$KEY" <<'EOF'
import sys,re
p,k=sys.argv[1],sys.argv[2]
b=open(p,'rb').read().decode('utf-8')
nl='\r\n' if '\r\n' in b else '\n'
out=[]
for l in b.split(nl):
    out.append('CONVEX_ADMIN_KEY='+k if l.startswith('CONVEX_ADMIN_KEY=') else l)
open(p,'wb').write(nl.join(out).encode('utf-8'))
EOF
    echo "updated $(basename "$f")"
  fi
done
docker exec space-proof-dind bash -c "cd '/src/apps/Application Plane' && export SOURCE_REVISION=w1 BUILD_DATE=2026-10-27T00:00:00Z && bash scripts/run-application-plane.sh up -d --no-build --force-recreate convex-gateway 2>&1" | grep -v -i -E "password|secret|token|key" | tail -4
