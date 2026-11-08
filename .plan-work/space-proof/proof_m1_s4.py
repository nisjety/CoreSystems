#!/usr/bin/env python3
# M1-S4 ZDR org tour. Runs INSIDE the nested daemon; never prints cookies or tokens.
import json, subprocess, sys, time, urllib.request, urllib.error, uuid
BASE = "http://localhost:5173"
AUTH = "/src/apps/Frontend Plane/verevonv3/tests/e2e/.auth/"
SEED = json.load(open(AUTH + "space-e2e-seed.json"))
ORG_Z = SEED["orgs"]["Z"]["id"]
st = json.load(open(AUTH + "space-e2e-state-F.json"))
COOK = "; ".join("%s=%s" % (c["name"], c["value"]) for c in st.get("cookies", []))
results = []

def call(method, path, body=None, timeout=30):
    h = {"Cookie": COOK, "Origin": BASE, "Referer": BASE + "/", "Accept": "application/json"}
    data = None
    if body is not None:
        data = json.dumps(body).encode(); h["Content-Type"] = "application/json"
    req = urllib.request.Request(BASE + path, data=data, headers=h, method=method)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return r.status, r.read().decode("utf-8", "replace"), dict(r.headers)
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode("utf-8", "replace"), dict(e.headers)
    except Exception as e:
        return 0, "ERR %s" % e, {}

def rec(name, ok, detail):
    results.append(ok); print("M1-S4 | %s | %s | %s" % ("PASS" if ok else "FAIL", name, detail), flush=True)

def sql(db, q, container):
    out = subprocess.run(["docker", "exec", container, "sh", "-c", 'psql -U "$POSTGRES_USER" -d %s -Atc "%s"' % (db, q)], capture_output=True, text=True)
    return out.stdout.strip()

def usercore_log_lines():
    out = subprocess.run(["docker", "logs", "--since", "2m", "user-service"], capture_output=True, text=True)
    return [l for l in (out.stdout + out.stderr).splitlines() if "/internal/spaces/" in l and ("thread-decision" in l or "thread-read-decision" in l)]

# is org Z actually ZDR in org-core?
z = sql("org_service", "select zero_data_retention from organizations where id='%s'" % ORG_Z, "controlplane-postgres") or sql("org_service", "select coalesce(settings::text,'') from organizations where id='%s'" % ORG_Z, "controlplane-postgres")
print("org Z zdr flag row:", z[:80])

runs_before = sql("session_core", "select count(*) from runs", "model-plane-postgres-1")
msgs_before = sql("session_core", "select count(*) from messages", "model-plane-postgres-1")
before_ctl = len(usercore_log_lines())
s0, raw0, _ = call("POST", "/api/v1/spaces", {"kind": "room", "name": "ZDR proof room"})
_d = (json.loads(raw0).get("data") or {}) if raw0.startswith("{") else {}
space = (_d.get("space") or {}).get("space_ref") or ("zdr-probe-" + uuid.uuid4().hex[:8])
print("org Z room create:", s0, raw0[:200])
time.sleep(8)
s, raw, _ = call("POST", "/api/v1/spaces/%s/threads" % space, {"content": "ZDR probe post", "client_send_id": "zdr-probe-" + uuid.uuid4().hex[:16]})
rec("ZDR org F post refused typed 412", s == 412, "status %s %s" % (s, raw[:240]))
after_ctl = len(usercore_log_lines())
rec("no Control decision call for the refused post", after_ctl == before_ctl, "thread-decision log lines before=%d after=%d" % (before_ctl, after_ctl))
runs_after = sql("session_core", "select count(*) from runs", "model-plane-postgres-1")
msgs_after = sql("session_core", "select count(*) from messages", "model-plane-postgres-1")
rec("zero rows written (runs, messages)", runs_before == runs_after and msgs_before == msgs_after, "runs %s->%s messages %s->%s" % (runs_before, runs_after, msgs_before, msgs_after))
# subscribe: stream endpoint for ZDR org
s, raw, h = call("GET", "/api/v1/spaces/%s/events" % space, timeout=15)
rec("ZDR stream refused or poll_only (never a live stream)", s in (412, 200) and ("poll_only" in raw or s == 412), "status %s %s" % (s, raw[:240]))
# artifacts tab surface
for path in ("/api/v1/spaces/%s/artifacts" % space, "/api/v1/spaces/%s/context" % space):
    s, raw, _ = call("GET", path)
    rec("ZDR org %s typed refusal, no content" % path.split("/")[-1], s in (403, 404, 412), "status %s %s" % (s, raw[:200]))
print("SUMMARY", sum(results), "pass,", len(results) - sum(results), "fail of", len(results))
