#!/usr/bin/env python3
# Existence-oracle probe: real room vs missing room, as a same-org non-member (E) and a different-org user (D). INSIDE the nested daemon.
import json, sys, urllib.request, urllib.error, urllib.parse
BASE = "http://localhost:5173"; AUTH = "/src/apps/Frontend Plane/verevonv3/tests/e2e/.auth/"
def cookie(l):
    st = json.load(open(AUTH + "space-e2e-state-%s.json" % l)); return "; ".join("%s=%s" % (c["name"], c["value"]) for c in st.get("cookies", []))
def call(l, path):
    h = {"Cookie": cookie(l), "Origin": BASE, "Referer": BASE + "/", "Accept": "application/json"}
    try:
        with urllib.request.urlopen(urllib.request.Request(BASE + path, headers=h), timeout=30) as r: return r.status, r.read().decode()
    except urllib.error.HTTPError as e: return e.code, e.read().decode()
real = urllib.parse.quote(sys.argv[1], safe=""); missing = "pn7doesnotexist0000000000000000000"
for who in ("E", "D"):
    for ep in ("context", "threads", "roster", "events", "artifacts"):
        s1, b1 = call(who, "/api/v1/spaces/%s/%s" % (real, ep)); s2, b2 = call(who, "/api/v1/spaces/%s/%s" % (missing, ep))
        print("%s %-9s real=%s missing=%s byte_equal=%s" % (who, ep, s1, s2, (s1 == s2 and b1 == b2)))
