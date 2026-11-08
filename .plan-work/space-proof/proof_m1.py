#!/usr/bin/env python3
# W1 live proof driver. Runs INSIDE the nested daemon container (private stack only): talks to the SPA origin
# http://localhost:5173 (nginx -> gateway) with the seeded users' session cookies. Never prints cookies or tokens.
import json, subprocess, sys, time, threading, urllib.request, urllib.error, uuid

BASE = "http://localhost:5173"
AUTH = "/src/apps/Frontend Plane/verevonv3/tests/e2e/.auth/"
SEED = json.load(open(AUTH + "space-e2e-seed.json"))
ORG_X = SEED["orgs"]["X"]["id"]
results = []

def cookies(letter):
    st = json.load(open(AUTH + "space-e2e-state-%s.json" % letter))
    return "; ".join("%s=%s" % (c["name"], c["value"]) for c in st.get("cookies", []))

COOK = {l: cookies(l) for l in "ABCDEF"}

def call(letter, method, path, body=None, extra=None, timeout=30):
    h = {"Cookie": COOK[letter], "Origin": BASE, "Referer": BASE + "/", "Accept": "application/json"}
    data = None
    if body is not None:
        data = json.dumps(body).encode()
        h["Content-Type"] = "application/json"
    if extra:
        h.update(extra)
    req = urllib.request.Request(BASE + path, data=data, headers=h, method=method)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            raw = r.read().decode("utf-8", "replace")
            return r.status, raw, dict(r.headers)
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode("utf-8", "replace"), dict(e.headers)
    except Exception as e:
        return 0, "ERR %s" % e, {}

def js(raw):
    try:
        return json.loads(raw)
    except Exception:
        return {}

def rec(sid, name, ok, detail):
    results.append((sid, name, "PASS" if ok else "FAIL", detail))
    print("%s | %s | %s | %s" % (sid, "PASS" if ok else "FAIL", name, detail), flush=True)

def sql(db, q, container="controlplane-postgres"):
    out = subprocess.run(["docker", "exec", container, "sh", "-c", 'psql -U "$POSTGRES_USER" -d %s -Atc "%s"' % (db, q)], capture_output=True, text=True)
    return out.stdout.strip()

# user ids
uid = {}
for l in "ABCDEF":
    em = SEED["users"][l]["email"]
    uid[l] = sql("auth_service", 'select id from \\"user\\" where email = \'%s\'' % em)
print("user ids resolved:", {k: bool(v) for k, v in uid.items()})

# ---------- M1-S7 client resilience (static) ----------
s, raw, h = call("A", "GET", "/version.json")
rec("M1-S7", "version.json served", s == 200 and "build_id" in raw, "status %s %s cache-control=%s" % (s, raw[:80], h.get("Cache-Control") or h.get("cache-control")))
s, raw, h = call("A", "GET", "/")
rec("M1-S7", "index.html no-cache", s == 200 and "no-cache" in (h.get("Cache-Control") or h.get("cache-control") or "").lower() or "no-store" in (h.get("Cache-Control") or "").lower(), "status %s cache-control=%s" % (s, h.get("Cache-Control") or h.get("cache-control")))

# ---------- M1-S5 governance (read side) ----------
s, raw, _ = call("A", "GET", "/api/v1/orgs/%s/space-policy" % ORG_X)
rec("M1-S5", "owner reads space policy", s == 200, "status %s %s" % (s, raw[:140]))
s, raw, _ = call("B", "GET", "/api/v1/orgs/%s/space-policy" % ORG_X)
rec("M1-S5", "member refused typed", s in (401, 403) and ("code" in raw), "status %s %s" % (s, raw[:140]))
s, raw, _ = call("A", "GET", "/api/v1/orgs/%s/space-policy/events?limit=5" % ORG_X)
rec("M1-S5", "policy events list", s == 200, "status %s %s" % (s, raw[:120]))

# ---------- audit role gate (WS05-S1) ----------
s, raw, _ = call("B", "GET", "/api/v1/audit?limit=5")
sa, rawa, _ = call("A", "GET", "/api/v1/audit?limit=5")
s2, raw2, _ = call("B", "GET", "/api/v1/audit?limit=5&user_id=%s" % uid["A"])
rec("M1-S5", "audit read member own-rows / other user refused", s in (200, 403) and s2 == 403, "member own=%s other-user=%s owner=%s %s" % (s, s2, sa, raw2[:100]))

# ---------- M1-S6/S1 room, members, posts ----------
s, raw, _ = call("A", "POST", "/api/v1/spaces", {"kind": "room", "name": "W1 proof room"})
_d = js(raw).get("data", js(raw))
space = (_d.get("space") or {}).get("space_ref") or ""
rec("M1-S6", "create room (accepted, registration pending)", s in (200, 201, 202) and bool(space), "status %s space=%s lifecycle=%s" % (s, space, (_d.get("space") or {}).get("lifecycle")))
if not space:
    print("cannot continue without a room")
    sys.exit(2)
sp = urllib.request.quote(space, safe="")
life = ""
for _i in range(40):
    s_, raw_, _ = call("A", "GET", "/api/v1/spaces/%s/context" % sp)
    _c = js(raw_).get("data", js(raw_))
    life = str(((_c.get("space") or {}).get("lifecycle")) or _c.get("lifecycle") or "")
    if life in ("active", "registered") or s_ not in (200, 202, 404):
        break
    time.sleep(3)
rec("M1-S6", "room registration reaches active at Control", life in ("active", "registered"), "lifecycle=%s after polling (context status %s)" % (life, s_))
s, raw, _ = call("A", "POST", "/api/v1/spaces/%s/members" % sp, {"member_id": uid["B"], "role": "editor"})
rec("M1-S6", "add B as editor", s in (200, 201), "status %s %s" % (s, raw[:140]))
s, raw, _ = call("A", "POST", "/api/v1/spaces/%s/members" % sp, {"member_id": uid["C"], "role": "viewer"})
rec("M1-S6", "add C as viewer", s in (200, 201), "status %s %s" % (s, raw[:140]))
time.sleep(4)

sid = "proof-" + uuid.uuid4().hex[:12]
runs_before = sql("session_core", "select count(*) from runs", "model-plane-postgres-1")
s, raw, _ = call("A", "POST", "/api/v1/spaces/%s/threads" % sp, {"content": "W1 proof: first human post", "client_send_id": sid})
j = js(raw).get("data", js(raw))
thread = j.get("thread_id") or ""
msg1 = j.get("message_id") or ""
rec("M1-S1", "A posts a new thread (no model)", s in (200, 201) and bool(thread), "status %s %s" % (s, raw[:200]))
s, raw, _ = call("A", "POST", "/api/v1/spaces/%s/threads" % sp, {"content": "W1 proof: first human post", "client_send_id": sid})
j2 = js(raw).get("data", js(raw))
rec("M1-S1", "replay same client_send_id is deduplicated", s in (200, 201) and (j2.get("deduplicated") is True or j2.get("message_id") == msg1), "status %s %s" % (s, raw[:160]))
s, raw, _ = call("A", "POST", "/api/v1/spaces/%s/threads" % sp, {"content": "different content", "client_send_id": sid})
rec("M1-S1", "same key, changed content refused", s in (409, 400, 422), "status %s %s" % (s, raw[:160]))
if thread:
    tp = urllib.request.quote(thread, safe="")
    s, raw, _ = call("B", "POST", "/api/v1/spaces/%s/threads/%s/messages" % (sp, tp), {"content": "W1 proof: B replies in A's thread", "client_send_id": "proof-" + uuid.uuid4().hex[:12]})
    rec("M1-S1", "member B (editor) replies in A's thread", s in (200, 201), "status %s %s" % (s, raw[:200]))
    s, raw, _ = call("C", "POST", "/api/v1/spaces/%s/threads/%s/messages" % (sp, tp), {"content": "viewer should not post", "client_send_id": "proof-" + uuid.uuid4().hex[:12]})
    rec("M1-S1", "viewer C refused (typed)", s == 403 and "code" in raw, "status %s %s" % (s, raw[:160]))
    s, raw, _ = call("E", "POST", "/api/v1/spaces/%s/threads/%s/messages" % (sp, tp), {"content": "outsider", "client_send_id": "proof-" + uuid.uuid4().hex[:12]})
    rec("M1-S1", "outsider E refused", s in (403, 404), "status %s %s" % (s, raw[:160]))
    s, raw, _ = call("B", "GET", "/api/v1/spaces/%s/threads/%s/transcript" % (sp, tp))
    rec("M1-S1", "B reads the transcript with both posts", s == 200 and "first human post" in raw and "B replies" in raw, "status %s len=%d" % (s, len(raw)))
runs_after = sql("session_core", "select count(*) from runs", "model-plane-postgres-1")
rec("M1-S1", "no model run created by posts", runs_before == runs_after, "runs before=%s after=%s" % (runs_before, runs_after))
kinds = sql("session_core", "select post_kind||':'||count(*) from messages group by post_kind", "model-plane-postgres-1")
rec("M1-S1", "messages stored with post_kind", "post" in kinds, "post_kind counts: %s" % kinds.replace("\n", ","))

# ---------- M1-S2 realtime + M1-S3 revocation ----------
frames = []
stop = threading.Event()
def stream():
    h = {"Cookie": COOK["B"], "Origin": BASE, "Accept": "text/event-stream"}
    req = urllib.request.Request(BASE + "/api/v1/spaces/%s/events" % sp, headers=h)
    try:
        with urllib.request.urlopen(req, timeout=120) as r:
            frames.append((time.time(), "OPEN %s" % r.status))
            for line in r:
                frames.append((time.time(), line.decode("utf-8", "replace").strip()[:100]))
                if stop.is_set():
                    break
            if not stop.is_set():
                frames.append((time.time(), "CLOSED eof (server ended the stream)"))
    except Exception as e:
        frames.append((time.time(), "CLOSED %s" % str(e)[:80]))
t = threading.Thread(target=stream, daemon=True)
t.start()
time.sleep(4)
t0 = time.time()
call("A", "POST", "/api/v1/spaces/%s/threads" % sp, {"content": "W1 proof: live push probe", "client_send_id": "proof-" + uuid.uuid4().hex[:12]})
time.sleep(5)
opened = any(f[1].startswith("OPEN 200") for f in frames)
pushed = [f for f in frames if f[0] >= t0 and f[1].startswith("data:")]
lat = (pushed[0][0] - t0) if pushed else None
rec("M1-S2", "B's stream opens and receives a pushed frame after A's post", opened and bool(pushed), "opened=%s first frame latency=%s s frames=%d first=%s" % (opened, ("%.2f" % lat) if lat is not None else None, len(frames), pushed[0][1] if pushed else ""))
t1 = time.time()
s, raw, _ = call("A", "DELETE", "/api/v1/spaces/%s/members/%s" % (sp, urllib.request.quote(uid["B"], safe="")))
rec("M1-S3", "A removes B from the room", s in (200, 204), "status %s %s" % (s, raw[:100]))
closed_at = None
for _ in range(40):
    time.sleep(1)
    c = [f for f in frames if f[0] >= t1 and f[1].startswith("CLOSED")]
    if c:
        closed_at = c[0][0] - t1
        break
rec("M1-S3", "B's stream closes after revocation (target <= 15 s p95)", closed_at is not None and closed_at <= 15, "closed after %s s" % (("%.1f" % closed_at) if closed_at is not None else "not within 40 s"))
s, raw, _ = call("B", "GET", "/api/v1/spaces/%s/threads" % sp)
rec("M1-S3", "removed B gets no content", s in (403, 404) and "W1 proof" not in raw, "status %s %s" % (s, raw[:100]))
stop.set()

print("\nSUMMARY", sum(1 for r in results if r[2] == "PASS"), "pass,", sum(1 for r in results if r[2] == "FAIL"), "fail of", len(results))
json.dump(results, open("/tmp/proof_m1_results.json", "w"))
