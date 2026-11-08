#!/usr/bin/env python3
# M1-S9 part 2: 'save this' in the room vs in a personal chat. INSIDE the nested daemon. Two tiny claude-sonnet-5-5 turns.
import json, subprocess, sys, urllib.request, urllib.error
BASE = "http://localhost:5173"
AUTH = "/src/apps/Frontend Plane/verevonv3/tests/e2e/.auth/"
st = json.load(open(AUTH + "space-e2e-state-A.json"))
COOK = "; ".join("%s=%s" % (c["name"], c["value"]) for c in st.get("cookies", []))
ROOM, THREAD = sys.argv[1], sys.argv[2]
def call(path, body, timeout=200):
    h = {"Cookie": COOK, "Origin": BASE, "Referer": BASE + "/", "Accept": "application/json", "Content-Type": "application/json"}
    req = urllib.request.Request(BASE + path, data=json.dumps(body).encode(), headers=h, method="POST")
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
def rec(name, ok, detail): print("M1-S9 | %s | %s | %s" % ("PASS" if ok else "FAIL", name, detail), flush=True)
m0 = sql("select count(*) from agent_memory;")
s, raw = call("/api/v1/chat/invoke", {"content": "save this", "model": "claude-sonnet-5-5", "thread_id": THREAD, "space_ref": ROOM, "zdr": False})
print("room 'save this' ->", s, raw[:360].replace("\n", " "))
m1 = sql("select count(*) from agent_memory;")
rec("'save this' in the room stores no memory", m0 == m1, "agent_memory %s -> %s; typed marker memory_scope_contained in body=%s" % (m0, m1, "memory_scope_contained" in raw))
s, raw = call("/api/v1/chat/invoke", {"content": "Remember that my favourite colour is teal. save this", "model": "claude-sonnet-5-5", "zdr": False})
print("personal turn ->", s, raw[:300].replace("\n", " "))
m2 = sql("select count(*) from agent_memory;")
print("agent_memory after personal turn: %s -> %s" % (m1, m2))
print("space-less thread kinds (last 3 non-space threads):", sql("select string_agg(coalesce(space_kind,'<null>'), ',') from (select space_kind from threads where coalesce(space_id,'')='' order by created_at desc limit 3) t;"))
