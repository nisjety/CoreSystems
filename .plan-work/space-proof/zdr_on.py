#!/usr/bin/env python3
# Runs INSIDE the nested daemon. Switch ZDR on for the seeded ZDR-TEST org Z (private stack only). Never prints credentials.
import json, os, re, subprocess, sys
FV = "/src/apps/Frontend Plane/verevonv3"
seed = json.load(open(FV + "/tests/e2e/.auth/space-e2e-seed.json"))
zorg, femail = seed["orgs"]["Z"]["id"], seed["users"]["F"]["email"]
q = "select id from \\\"user\\\" where email = '%s'" % femail
fid = subprocess.run(["docker", "exec", "controlplane-postgres", "sh", "-c", 'psql -U "$POSTGRES_USER" -d auth_service -Atc "%s"' % q], capture_output=True, text=True).stdout.strip()
tok = ""
for line in open("/src/apps/Control Plane/.env.generated-secrets", encoding="utf-8", errors="replace"):
    if line.startswith("GATEWAY_ORG_CORE_SERVICE_TOKEN="):
        tok = line.split("=", 1)[1].strip().strip("\"'")
        break
print("org Z=%s F user id resolved=%s token present=%s" % (zorg, bool(fid), bool(tok)))
env = dict(os.environ, ORG_CORE_SERVICE_TOKEN=tok)
args = ["node", "apps/gateway/scripts/zdr-seed-test-org.mjs", "--base-url", "http://127.0.0.1:18080", "--org-id", zorg, "--user-id", fid, "--expect-name", "ZDR-TEST space e2e"] + sys.argv[1:]
out = subprocess.run(args, cwd=FV, env=env, capture_output=True, text=True)
text = (out.stdout + out.stderr)
if tok:
    text = text.replace(tok, "<redacted>")
print(re.sub(r"(?i)(token|secret|password|signature)([=:] *)\S+", r"\1\2<redacted>", text)[-1500:])
print("exit", out.returncode)
