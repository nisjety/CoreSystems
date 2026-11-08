#!/usr/bin/env python3
# M1-S6 room administration probe. INSIDE the nested daemon. Never prints cookies or tokens.
import json, subprocess, time, urllib.request, urllib.error, urllib.parse
BASE = "http://localhost:5173"
AUTH = "/src/apps/Frontend Plane/verevonv3/tests/e2e/.auth/"
SEED = json.load(open(AUTH + "space-e2e-seed.json"))
def cookies(l):
    st = json.load(open(AUTH + "space-e2e-state-%s.json" % l)); return "; ".join("%s=%s" % (c["name"], c["value"]) for c in st.get("cookies", []))
COOK = {l: cookies(l) for l in "ABCE"}
def call(l, method, path, body=None):
    h = {"Cookie": COOK[l], "Origin": BASE, "Referer": BASE + "/", "Accept": "application/json"}
    data = None
    if body is not None: data = json.dumps(body).encode(); h["Content-Type"] = "application/json"
    req = urllib.request.Request(BASE + path, data=data, headers=h, method=method)
    try:
        with urllib.request.urlopen(req, timeout=30) as r: return r.status, r.read().decode("utf-8", "replace")
    except urllib.error.HTTPError as e: return e.code, e.read().decode("utf-8", "replace")
    except Exception as e: return 0, "ERR %s" % e
def sql(db, q, c="controlplane-postgres"):
    return subprocess.run(["docker", "exec", c, "sh", "-c", 'psql -U "$POSTGRES_USER" -d %s -Atc "%s"' % (db, q)], capture_output=True, text=True).stdout.strip()
uid = {l: sql("auth_service", 'select id from \\"user\\" where email = \'%s\'' % SEED["users"][l]["email"]) for l in "ABCE"}
n = [0, 0]
def rec(name, ok, detail):
    n[0 if ok else 1] += 1; print("M1-S6 | %s | %s | %s" % ("PASS" if ok else "FAIL", name, detail[:230]), flush=True)
s, raw = call("A", "POST", "/api/v1/spaces", {"kind": "room", "name": "W1 admin room"})
sp = ((json.loads(raw).get("data") or {}).get("space") or {}).get("space_ref") or ""
q = urllib.parse.quote(sp, safe="")
for _ in range(20):
    c, r = call("A", "GET", "/api/v1/spaces/%s/context" % q)
    if c == 200: break
    time.sleep(2)
for l, role in (("B", "editor"), ("C", "viewer")):
    s, raw = call("A", "POST", "/api/v1/spaces/%s/members" % q, {"member_id": uid[l], "role": role}); rec("add %s as %s" % (l, role), s == 200, "status %s" % s)
time.sleep(3)
s, raw = call("A", "PATCH", "/api/v1/spaces/%s" % q, {"name": "Renamed admin room", "topic": "W1 topic"})
rec("owner renames the room and sets a topic", s == 200, "status %s %s" % (s, raw[:160]))
c, r = call("A", "GET", "/api/v1/spaces/%s/context" % q)
rec("context reflects the new name and topic", c == 200 and "Renamed admin room" in r and "W1 topic" in r, "status %s" % c)
s, raw = call("C", "PATCH", "/api/v1/spaces/%s" % q, {"name": "viewer rename"})
rec("viewer cannot rename (typed)", s == 403 and "code" in raw, "status %s %s" % (s, raw[:160]))
s, raw = call("E", "PATCH", "/api/v1/spaces/%s" % q, {"name": "outsider rename"})
rec("outsider cannot rename", s in (403, 404), "status %s %s" % (s, raw[:120]))
# 404 bodies byte-equal for outsider: real room vs nonexistent
s1, b1 = call("E", "GET", "/api/v1/spaces/%s/context" % q)
s2, b2 = call("E", "GET", "/api/v1/spaces/%s/context" % "pn7doesnotexist0000000000000000000")
rec("outsider sees byte-equal 404 for a real and a missing room", s1 == s2 == 404 and b1 == b2, "real=%s missing=%s equal=%s" % (s1, s2, b1 == b2))
s, raw = call("A", "PATCH", "/api/v1/spaces/%s/members/%s" % (q, urllib.parse.quote(uid["B"], safe="")), {"role": "manager"})
rec("owner promotes B to manager", s == 200, "status %s %s" % (s, raw[:140]))
time.sleep(2)
s, raw = call("B", "PATCH", "/api/v1/spaces/%s/members/%s" % (q, urllib.parse.quote(uid["C"], safe="")), {"role": "manager"})
rec("manager cannot grant manager", s in (403, 409) and "code" in raw, "status %s %s" % (s, raw[:200]))
s, raw = call("B", "PATCH", "/api/v1/spaces/%s/members/%s" % (q, urllib.parse.quote(uid["C"], safe="")), {"role": "owner"})
rec("manager cannot grant owner", s in (403, 409) and "code" in raw, "status %s %s" % (s, raw[:200]))
s, raw = call("B", "PATCH", "/api/v1/spaces/%s/members/%s" % (q, urllib.parse.quote(uid["C"], safe="")), {"role": "editor"})
rec("manager can grant editor", s == 200, "status %s %s" % (s, raw[:140]))
s, raw = call("A", "PATCH", "/api/v1/spaces/%s/members/%s" % (q, urllib.parse.quote(uid["A"], safe="")), {"role": "editor"})
rec("sole owner cannot demote self (409)", s == 409, "status %s %s" % (s, raw[:200]))
s, raw = call("A", "DELETE", "/api/v1/spaces/%s/members/%s" % (q, urllib.parse.quote(uid["A"], safe="")))
rec("sole owner cannot leave (409)", s == 409, "status %s %s" % (s, raw[:200]))
s, raw = call("A", "POST", "/api/v1/spaces/%s/archive" % q, {})
print("archive route probe (WS04-S7 gateway leg is documented as not built in W1): status %s %s" % (s, raw[:120]))
print("SUMMARY %d pass, %d fail" % (n[0], n[1]))
