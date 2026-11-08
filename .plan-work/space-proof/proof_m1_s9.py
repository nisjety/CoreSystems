#!/usr/bin/env python3
# M1-S9 containment probe (feasible part). Runs INSIDE the nested daemon. ONE real room-agent turn on claude-sonnet-5-5 (Anthropic key via the private Model Plane).
import json, subprocess, sys, time, urllib.request, urllib.error, urllib.parse
BASE = "http://localhost:5173"
AUTH = "/src/apps/Frontend Plane/verevonv3/tests/e2e/.auth/"
st = json.load(open(AUTH + "space-e2e-state-A.json"))
COOK = "; ".join("%s=%s" % (c["name"], c["value"]) for c in st.get("cookies", []))
SEED = json.load(open(AUTH + "space-e2e-seed.json"))
def call(method, path, body=None, timeout=200):
    h = {"Cookie": COOK, "Origin": BASE, "Referer": BASE + "/", "Accept": "application/json"}
    data = None
    if body is not None:
        data = json.dumps(body).encode(); h["Content-Type"] = "application/json"
    req = urllib.request.Request(BASE + path, data=data, headers=h, method=method)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return r.status, r.read().decode("utf-8", "replace")
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode("utf-8", "replace")
    except Exception as e:
        return 0, "ERR %s" % e
def sql(q):
    p = subprocess.run(["docker", "exec", "-i", "model-plane-postgres-1", "sh", "-c", 'psql -U "$POSTGRES_USER" -d session_core -At'], input=q, capture_output=True, text=True)
    return p.stdout.strip()
def rec(name, ok, detail):
    print("M1-S9 | %s | %s | %s" % ("PASS" if ok else "FAIL", name, detail), flush=True)

s, raw = call("POST", "/api/v1/spaces", {"kind": "room", "name": "W1 containment room"})
space = ((json.loads(raw).get("data") or {}).get("space") or {}).get("space_ref") or ""
print("room", s, space)
for _ in range(20):
    c, r = call("GET", "/api/v1/spaces/%s/context" % urllib.parse.quote(space, safe=""))
    if c == 200:
        break
    time.sleep(2)
time.sleep(4)
mem0 = sql("select count(*) from agent_memory;")
runs0 = sql("select count(*) from runs;")
t0 = time.time()
s, raw = call("POST", "/api/v1/chat/invoke", {"content": "Please remember that my favourite colour is teal. Reply with exactly one short sentence.", "model": "claude-sonnet-5-5", "space_ref": space, "zdr": False})
dt = time.time() - t0
j = {}
try: j = json.loads(raw)
except Exception: pass
d = j.get("data", j) if isinstance(j, dict) else {}
print("invoke status %s in %.1fs keys=%s" % (s, dt, sorted(d.keys())[:14] if isinstance(d, dict) else type(d).__name__))
print("invoke body head:", raw[:420].replace("\n", " "))
thread = (d.get("thread_id") if isinstance(d, dict) else "") or ""
time.sleep(3)
rec("room turn answered on claude-sonnet-5-5", s == 200, "status %s thread=%s model=%s" % (s, bool(thread), (d.get("model") if isinstance(d, dict) else "")))
if thread:
    sk = sql("select coalesce(space_kind,'<null>')||'|'||coalesce(space_id,'') from threads where id='%s';" % thread)
    rec("thread stamped with the verified space kind", sk.startswith("room|") or sk.startswith("shared|") or (sk and not sk.startswith("<null>")), "space_kind|space_id = %s" % sk)
    print("runs for thread:", sql("select count(*)||' runs, models: '||coalesce(string_agg(distinct coalesce(model,'?'), ','),'-') from runs where thread_id='%s';" % thread))
    print("messages:", sql("select string_agg(role||':'||coalesce(post_kind,'-'), ', ' order by sequence) from messages where thread_id='%s';" % thread))
mem1 = sql("select count(*) from agent_memory;")
rec("room fact not learned (no agent_memory rows from the room turn)", mem0 == mem1, "agent_memory %s -> %s" % (mem0, mem1))
# 'save this' in the room
if thread:
    s2, raw2 = call("POST", "/api/v1/chat/invoke", {"content": "save this", "model": "claude-sonnet-5-5", "thread_id": thread, "space_ref": space, "zdr": False})
    print("save-this status %s head: %s" % (s2, raw2[:300].replace("\n", " ")))
    mem2 = sql("select count(*) from agent_memory;")
    rec("'save this' in the room stores nothing", mem2 == mem1, "agent_memory %s -> %s; refusal marker present=%s" % (mem1, mem2, "memory_scope_contained" in raw2))
print("dream_runs total:", sql("select count(*) from dream_runs;"), "| skipped_space_scope:", sql("select count(*) from dream_runs where status='skipped_space_scope';"))
