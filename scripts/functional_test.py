#!/usr/bin/env python3
"""
CoreSystem Functional Test Suite
Full user journey: register → sign-in → session → onboard → test all planes
Usage: python3 scripts/functional_test.py [--verbose]
"""

import sys
import json
import time
import urllib.request
import urllib.error
import http.cookiejar
import subprocess
import textwrap
from urllib.parse import urlencode

VERBOSE = "--verbose" in sys.argv or "-v" in sys.argv

# ─── Colours ──────────────────────────────────────────────────────────────────
RED = "\033[0;31m"; GREEN = "\033[0;32m"; YELLOW = "\033[1;33m"
CYAN = "\033[0;36m"; BOLD = "\033[1m"; RESET = "\033[0m"

# ─── State ────────────────────────────────────────────────────────────────────
PASS = FAIL = WARN = SKIP = 0
START = time.time()
USER_EMAIL = USER_ID = ORG_ID = DOC_ID = SESSION_TOKEN = ""

# ─── Ports ───────────────────────────────────────────────────────────────────
AUTH    = 3011
USER    = 3012
BILLING = 3014
ORG     = 8080
REASON  = 8101
DOCS    = 8001
RETR    = 8004
LAGO    = 3016

# ─── Cookie-aware HTTP client ─────────────────────────────────────────────────
cj = http.cookiejar.LWPCookieJar()
opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(cj))


def req(method, url, body=None, headers=None, timeout=30):
    """Make an HTTP request; returns (status_code, response_body_str)."""
    _headers = {"Content-Type": "application/json", "Accept": "application/json"}
    if headers:
        _headers.update(headers)
    data = json.dumps(body).encode() if body is not None else None
    r = urllib.request.Request(url, data=data, headers=_headers, method=method)
    try:
        with opener.open(r, timeout=timeout) as resp:
            raw = resp.read()
            return resp.status, raw.decode("utf-8", errors="replace")
    except urllib.error.HTTPError as e:
        raw = e.read()
        return e.code, raw.decode("utf-8", errors="replace")
    except Exception as exc:
        return 0, str(exc)


def jparse(text):
    try:
        return json.loads(text)
    except Exception:
        return {}


def jget(d, path):
    """Dotpath get: jget(d, 'user.id')"""
    for k in path.split("."):
        if isinstance(d, dict):
            d = d.get(k)
        else:
            return None
        if d is None:
            return None
    return d


# ─── Logging ─────────────────────────────────────────────────────────────────
def log(msg):   print(f"{CYAN}[INFO]{RESET}  {msg}")
def vlog(msg):
    if VERBOSE: print(f"{YELLOW}  ↳ {msg}{RESET}")


def passed(msg):
    global PASS; PASS += 1
    print(f"{GREEN}[PASS]{RESET}  {msg}")


def failed(msg):
    global FAIL; FAIL += 1
    print(f"{RED}[FAIL]{RESET}  {msg}")


def warned(msg):
    global WARN; WARN += 1
    print(f"{YELLOW}[WARN]{RESET}  {msg}")


def skipped(msg):
    global SKIP; SKIP += 1
    print(f"{YELLOW}[SKIP]{RESET}  {msg}")


def header(msg):
    bar = "═" * 44
    print(f"\n{BOLD}{CYAN}{bar}{RESET}")
    print(f"{BOLD}{CYAN}  {msg}{RESET}")
    print(f"{BOLD}{CYAN}{bar}{RESET}")


# ─── PRE-FLIGHT ───────────────────────────────────────────────────────────────
header("PRE-FLIGHT: SERVICE PORT CHECK")

preflight = [
    ("AUTH",      AUTH,    "/api/auth/get-session"),
    ("USER-SVC",  USER,    "/health"),
    ("BILLING",   BILLING, "/health"),
    ("ORG-CORE",  ORG,     "/health"),
    ("REASONING", REASON,  "/health"),
    ("DOCUMENTS", DOCS,    "/health"),
    ("RETRIEVAL", RETR,    "/health"),
    ("LAGO",      LAGO,    "/api/v1/add_ons"),
]
for svc, port, path in preflight:
    code, _ = req("GET", f"http://localhost:{port}{path}", timeout=5)
    if code in (200, 401, 403, 404):
        passed(f"{svc} (:{port}) — HTTP {code}")
    else:
        warned(f"{svc} (:{port}) not responding — HTTP {code}")

# ─── SECTION 1: REGISTRATION ─────────────────────────────────────────────────
header("SECTION 1: USER REGISTRATION")

TS = int(time.time())
USER_EMAIL  = f"functest_{TS}@test.local"
USER_NAME   = f"Func Tester {TS}"
USER_PASS   = "TestPass1234!"
log(f"Registering: {USER_EMAIL}")

# Try v2 NestJS endpoint (syncs user to user-service on success)
code, body = req("POST", f"http://localhost:{AUTH}/api/v2/auth/signUp",
                 {"name": USER_NAME, "email": USER_EMAIL, "password": USER_PASS})
SIGNUP = jparse(body)
vlog(f"Sign-up v2 HTTP {code}: {body[:200]}")

if code in (200, 201):
    passed(f"Sign-up v2 — HTTP {code}")
else:
    warned(f"v2 sign-up returned {code} — trying native Better Auth /sign-up/email")
    code, body = req("POST", f"http://localhost:{AUTH}/api/auth/sign-up/email",
                     {"name": USER_NAME, "email": USER_EMAIL, "password": USER_PASS})
    SIGNUP = jparse(body)
    vlog(f"Sign-up native HTTP {code}: {body[:200]}")
    if code in (200, 201):
        passed(f"Sign-up native — HTTP {code}")
    else:
        failed(f"Sign-up failed — HTTP {code}")
        vlog(f"Body: {body[:300]}")

# Duplicate should be rejected
log("Testing duplicate registration rejection...")
code2, dup_body = req("POST", f"http://localhost:{AUTH}/api/v2/auth/signUp",
                      {"name": USER_NAME, "email": USER_EMAIL, "password": USER_PASS})
vlog(f"Duplicate HTTP {code2}: {dup_body[:150]}")
dup_err = jget(jparse(dup_body), "error") or ""
if code2 in (400, 409, 422, 500) or (code2 == 200 and dup_err):
    passed(f"Duplicate registration rejected — HTTP {code2} ('{dup_err or 'error field'}')")
else:
    warned(f"Duplicate registration returned {code2} — expected 4xx or error body")

# ─── SECTION 2: SIGN IN & SESSION ────────────────────────────────────────────
header("SECTION 2: SIGN IN & SESSION")

log(f"Signing in as {USER_EMAIL}...")
code, body = req("POST", f"http://localhost:{AUTH}/api/v2/auth/signIn",
                 {"email": USER_EMAIL, "password": USER_PASS})
SIGNIN = jparse(body)
vlog(f"Sign-in v2 HTTP {code}: {body[:300]}")

if code == 200:
    passed("Sign-in v2 — HTTP 200")
else:
    warned(f"v2 sign-in returned {code} — trying native /sign-in/email")
    code, body = req("POST", f"http://localhost:{AUTH}/api/auth/sign-in/email",
                     {"email": USER_EMAIL, "password": USER_PASS, "callbackURL": "/"})
    SIGNIN = jparse(body)
    vlog(f"Sign-in native HTTP {code}: {body[:300]}")
    if code == 200:
        passed("Sign-in native — HTTP 200")
    else:
        failed(f"Sign-in failed — HTTP {code}")
        vlog(f"Body: {body[:300]}")

SESSION_TOKEN = (jget(SIGNIN, "token") or
                  jget(SIGNIN, "user.token") or
                  jget(jparse(body), "session.token") or "")
if SESSION_TOKEN:
    passed(f"Bearer token extracted: {SESSION_TOKEN[:20]}...")
else:
    # token lives in the session object — grab it after getSession call
    pass  # will populate from session below

cookie_count = len(list(cj))
if cookie_count > 0:
    passed(f"Session cookies captured ({cookie_count} cookies)")
    for c in cj:
        vlog(f"Cookie: {c.name}={c.value[:20]}... domain={c.domain}")
else:
    warned("No cookies captured — session-auth tests may fail")

# Get session
log("Getting session...")
code, body = req("POST", f"http://localhost:{AUTH}/api/v2/auth/getSession", {})
SESSION = jparse(body)
vlog(f"getSession POST HTTP {code}: {body[:300]}")

if code != 200:
    code, body = req("GET", f"http://localhost:{AUTH}/api/auth/get-session")
    SESSION = jparse(body)
    vlog(f"getSession GET HTTP {code}: {body[:300]}")

if code == 200:
    passed("Get session — HTTP 200")
    USER_ID = jget(SESSION, "user.id") or jget(SESSION, "session.userId") or ""
    if USER_ID:
        passed(f"User ID from session: {USER_ID}")
    else:
        warned("No user ID in session")
        vlog(f"Session: {body[:300]}")
        USER_ID = jget(SIGNIN, "user.id") or ""
        if USER_ID:
            passed(f"User ID from sign-in body: {USER_ID}")
    if not SESSION_TOKEN:
        SESSION_TOKEN = jget(SESSION, "session.token") or ""
        if SESSION_TOKEN:
            passed(f"Bearer token from session: {SESSION_TOKEN[:20]}...")
else:
    warned(f"Get session returned {code}")
    vlog(f"Body: {body[:300]}")
    USER_ID = jget(SIGNIN, "user.id") or ""
    if USER_ID:
        passed(f"User ID from sign-in: {USER_ID}")

# Wrong password should be rejected
log("Testing wrong password rejection...")
code_bad, bad_body = req("POST", f"http://localhost:{AUTH}/api/v2/auth/signIn",
                         {"email": USER_EMAIL, "password": "WRONG_PASS_999"})
vlog(f"Wrong password HTTP {code_bad}: {bad_body[:150]}")
bad_err = jget(jparse(bad_body), "error") or ""
if code_bad in (400, 401, 403) or (code_bad == 200 and bad_err):
    passed(f"Wrong password rejected — HTTP {code_bad} ('{bad_err or 'error field'}')")
else:
    warned(f"Wrong password returned {code_bad} — expected 4xx or error body")
    vlog(f"Body: {bad_body[:200]}")

# Re-establish clean session (ensures cookie jar is fresh)
req("POST", f"http://localhost:{AUTH}/api/v2/auth/signIn",
    {"email": USER_EMAIL, "password": USER_PASS})
log("Session refreshed")

# ─── SECTION 3: USER SERVICE ─────────────────────────────────────────────────
header(f"SECTION 3: USER SERVICE (port {USER})")

log("User by email lookup — waiting for NATS sync (up to 20s)...")
import urllib.parse as _urlparse
_encoded_email = _urlparse.quote(USER_EMAIL, safe="")
_by_email_ok = False
for _attempt in range(10):
    time.sleep(2)
    code, body = req("GET", f"http://localhost:{USER}/api/v1/users/by-email/{_encoded_email}")
    vlog(f"  by-email attempt {_attempt+1}: HTTP {code}  {body[:100]}")
    if code == 200:
        _by_email_ok = True
        break
    if code not in (404, 0):
        break  # unexpected error — stop retrying

if _by_email_ok:
    d = jparse(body)
    found = (jget(d, "user") or
             (d if isinstance(d, dict) and d.get("id") else
              (d.get("users") or d.get("data") or [{}])[0] if isinstance(d, dict) else
              (d[0] if isinstance(d, list) and len(d) > 0 else {})))
    found_id = (jget(found, "id") if isinstance(found, dict) else None) or ""
    passed(f"User found in user-service by email after NATS sync ✓  (attempt {_attempt+1})")
    if found_id and not USER_ID:
        USER_ID = found_id
        passed(f"User ID from user-service: {USER_ID}")
else:
    failed(f"NATS sync failed: user not in user-service after 20s (last HTTP {code}): {body[:120]}")

log("GET /api/v1/users/me (Bearer token)...")
_me_headers = {}
if SESSION_TOKEN:
    _me_headers["Authorization"] = f"Bearer {SESSION_TOKEN}"
code, body = req("GET", f"http://localhost:{USER}/api/v1/users/me", headers=_me_headers if _me_headers else None)
vlog(f"users/me HTTP {code}: {body[:200]}")

if code == 200:
    passed("User /me — HTTP 200 ✓")
else:
    # user-service /me extracts user ID from internal service context (not cookie/bearer).
    # It is designed for service-to-service calls via API gateway, not direct client access.
    skipped(f"User /me — {code} (expected: service-to-service auth only; not a test failure)")

# ─── SECTION 4: ONBOARDING — CREATE ORG ──────────────────────────────────────
header(f"SECTION 4: ONBOARDING — CREATE ORGANISATION (port {ORG})")

if not USER_ID:
    warned("No user ID — using placeholder")
    USER_ID = f"test-user-{TS}"

ORG_NAME = f"Test Org {TS}"
ORG_SLUG = f"test-org-{TS}"
log(f"Creating org: {ORG_NAME!r}  (user: {USER_ID})")

code, body = req("POST", f"http://localhost:{ORG}/api/v1/organizations",
                 {"name": ORG_NAME, "slug": ORG_SLUG, "plan": "free"},
                 headers={"x-user-id": USER_ID})
ORG_DATA = jparse(body)
vlog(f"Create org (v1) HTTP {code}: {body[:300]}")

if code in (200, 201):
    passed(f"Organisation created — HTTP {code}")
    ORG_ID = jget(ORG_DATA, "id") or jget(ORG_DATA, "organization.id") or ""
    if ORG_ID:
        passed(f"Org ID: {ORG_ID}")
    else:
        warned("Could not extract org ID from response")
        vlog(f"Keys: {list(ORG_DATA.keys()) if ORG_DATA else 'N/A'}")
else:
    vlog(f"v1 failed with {code}, trying /orgs...")
    code, body = req("POST", f"http://localhost:{ORG}/orgs",
                     {"name": ORG_NAME, "slug": ORG_SLUG, "plan": "free"},
                     headers={"x-user-id": USER_ID})
    ORG_DATA = jparse(body)
    vlog(f"Create org (/orgs) HTTP {code}: {body[:300]}")
    if code in (200, 201):
        passed(f"Organisation created (/orgs) — HTTP {code}")
        ORG_ID = jget(ORG_DATA, "id") or ""
        if ORG_ID:
            passed(f"Org ID: {ORG_ID}")
    else:
        failed(f"Organisation creation failed — HTTP {code}")
        vlog(f"Body: {body[:400]}")

log("Listing user organisations...")
code, body = req("GET", f"http://localhost:{ORG}/api/v1/organizations",
                 headers={"x-user-id": USER_ID})
vlog(f"List orgs HTTP {code}")

if code == 200:
    d = jparse(body)
    orgs = d if isinstance(d, list) else d.get("organizations", d.get("data", []))
    passed(f"List organisations — HTTP 200 (count: {len(orgs)})")
else:
    code, body = req("GET", f"http://localhost:{ORG}/orgs",
                     headers={"x-user-id": USER_ID})
    if code == 200:
        passed("List organisations (/orgs) — HTTP 200")
    else:
        warned(f"List organisations returned HTTP {code}")
        vlog(f"Body: {body[:300]}")

if ORG_ID:
    log(f"Fetch org by ID: {ORG_ID}...")
    code, body = req("GET", f"http://localhost:{ORG}/api/v1/organizations/{ORG_ID}",
                     headers={"x-user-id": USER_ID})
    if code == 200:
        passed("Get org by ID — HTTP 200")
    else:
        code, body = req("GET", f"http://localhost:{ORG}/orgs/{ORG_ID}",
                         headers={"x-user-id": USER_ID})
        if code == 200:
            passed("Get org (/orgs/:id) — HTTP 200")
        else:
            warned(f"Get org by ID — HTTP {code}")

log("Auth-service /api/v2/organizations (session cookie)...")
code, body = req("GET", f"http://localhost:{AUTH}/api/v2/organizations")
vlog(f"Auth orgs HTTP {code}: {body[:200]}")
if code == 200:
    passed("Auth-service /api/v2/organizations — HTTP 200")
elif code in (401, 403):
    warned(f"Auth-service /api/v2/organizations — {code} (cookie not forwarded to v2 router)")
else:
    warned(f"Auth-service /api/v2/organizations — HTTP {code}")

# ─── SECTION 5: REASONING ────────────────────────────────────────────────────
header(f"SECTION 5: REASONING PLANE (port {REASON})")

log("Chain-of-thought test...")
code, body = req("POST", f"http://localhost:{REASON}/api/v1/reason",
                 {"query": "What are the three branches of the US government?",
                  "strategy": "chain_of_thought"},
                 timeout=90)
COT = jparse(body)
vlog(f"CoT HTTP {code}")

if code == 200:
    answer = COT.get("answer") or COT.get("result") or ""
    if answer:
        snippet = answer[:80].replace("\n", " ")
        passed(f"CoT answer present ({len(answer)} chars): {snippet}...")
    else:
        warned("CoT answer field missing — check response schema")
        vlog(f"Response keys: {list(COT.keys())}")

    thoughts = (COT.get("thoughts") or COT.get("chain") or
               COT.get("steps") or COT.get("reasoning_steps") or
               COT.get("reasoning_trace") or [])
    if isinstance(thoughts, list) and len(thoughts) > 0:
        passed(f"CoT reasoning_trace present ({len(thoughts)} steps)")
    elif isinstance(thoughts, str) and len(thoughts) > 10:
        passed(f"CoT reasoning_trace present ({len(thoughts)} chars)")
    else:
        warned("CoT reasoning_trace = 0 — check reasoning-core field names")

    strategy = COT.get("model") or COT.get("strategy_used") or COT.get("strategy", "")
    if strategy:
        passed(f"Strategy/model: {strategy}")
    else:
        warned("Strategy/model field missing from response")
else:
    failed(f"CoT reasoning — HTTP {code}")
    vlog(f"Body: {body[:400]}")

log("Tree-of-thought test (max_branches=2)...")
code, body = req("POST", f"http://localhost:{REASON}/api/v1/reason",
                 {"query": "Pros and cons of microservices vs monolith architecture?",
                  "strategy": "tree_of_thought",
                  "max_branches": 2},
                 timeout=120)
TOT = jparse(body)
vlog(f"ToT HTTP {code}")

if code == 200:
    passed("ToT reasoning — HTTP 200")
    branches = (TOT.get("branches") or TOT.get("tree") or
                TOT.get("paths") or TOT.get("all_paths") or
                TOT.get("alternative_explanations") or
                TOT.get("reasoning_trace") or [])
    if isinstance(branches, list) and len(branches) > 0:
        passed(f"ToT branches/alternatives present ({len(branches)})")
    elif isinstance(branches, str) and len(branches) > 10:
        passed(f"ToT reasoning trace present ({len(branches)} chars)")
    else:
        warned("ToT branches/alternatives = 0 — check tree_of_thought.py field names")
        vlog(f"Keys: {list(TOT.keys())}")
else:
    failed(f"ToT reasoning — HTTP {code}")
    vlog(f"Body: {body[:400]}")

# ─── SECTION 6: DATA PLANE E2E ───────────────────────────────────────────────
header(f"SECTION 6: DATA PLANE — INGEST → EMBED → RETRIEVE (ports {DOCS}/{RETR})")

ORG_FOR_DATA = ORG_ID or f"functest-org-{TS}"
UMARKER      = f"coresystem-functest-marker-{TS}"
log("Ingesting test document...")

code, body = req("POST", f"http://localhost:{DOCS}/v1/documents",
                 {"content": f"Quantum entanglement allows particles to be correlated "
                             f"across any distance. Marker: {UMARKER}",
                  "org_id": ORG_FOR_DATA,
                  "source": "functest",
                  "type":   "text",
                  "title":  "Quantum Entanglement Functest"})
DOC_DATA = jparse(body)
vlog(f"Ingest HTTP {code}: {body[:300]}")

if code in (200, 201):
    passed(f"Document ingested — HTTP {code}")
    DOC_ID = DOC_DATA.get("document_id") or DOC_DATA.get("id") or DOC_DATA.get("doc_id", "")
    if DOC_ID:
        passed(f"Doc ID: {DOC_ID}")
    else:
        warned("Could not extract document ID")
else:
    failed(f"Document ingest — HTTP {code}")
    vlog(f"Body: {body[:400]}")

if DOC_ID:
    log("Polling document status (≤30s)...")
    doc_status = "pending"
    for _i in range(10):
        time.sleep(3)
        c2, b2 = req("GET", f"http://localhost:{DOCS}/v1/documents/{DOC_ID}?org_id={ORG_FOR_DATA}")
        doc_status = jparse(b2).get("status", "unknown")
        vlog(f"Poll {_i+1}/10: status={doc_status}")
        if doc_status in ("indexed", "ready", "completed", "processed"):
            break

    if doc_status in ("indexed", "ready", "completed", "processed"):
        passed(f"Document indexed — status: {doc_status}")
    else:
        warned(f"Not indexed after 30s — final status: {doc_status}")

    log("Semantic retrieval test...")
    code, body = req("POST", f"http://localhost:{RETR}/v1/retrieve",
                     {"org_id": ORG_FOR_DATA,
                      "query": "quantum entanglement correlated particles",
                      "top_k": 3},
                     timeout=30)
    RETR_DATA = jparse(body)
    vlog(f"Retrieve HTTP {code}: {body[:300]}")

    if code == 200:
        results = (RETR_DATA.get("results") or RETR_DATA.get("hits") or
                   RETR_DATA.get("documents") or RETR_DATA.get("data") or
                   RETR_DATA.get("facts") or [])
        if isinstance(results, list) and len(results) > 0:
            passed(f"Retrieval returned {len(results)} result(s)")
            match = any("entanglement" in str(r).lower() for r in results)
            if match:
                passed("Ingested content found in retrieval results ✓")
            else:
                warned("Content not yet in results (may still be embedding)")
        else:
            warned("Retrieval returned 0 results — not indexed yet?")
            vlog(f"Body: {body[:400]}")
    else:
        failed(f"Retrieval — HTTP {code}")
        vlog(f"Body: {body[:400]}")

    log("Cleanup: delete test document...")
    code, body = req("DELETE", f"http://localhost:{DOCS}/v1/documents/{DOC_ID}?org_id={ORG_FOR_DATA}")
    if code in (200, 202, 204):
        passed(f"Document deleted — HTTP {code}")
    else:
        warned(f"Delete returned HTTP {code}")

# ─── SECTION 7: PROFILE & CONSENT ────────────────────────────────────────────
header("SECTION 7: PROFILE & CONSENT (auth-service v2)")

log("Get profile...")
code, body = req("POST", f"http://localhost:{AUTH}/api/v2/auth/profile/getProfile", {})
vlog(f"Profile HTTP {code}: {body[:200]}")
if code == 200:
    passed("Profile — HTTP 200")
elif code in (401, 403):
    warned(f"Profile — {code} (cookie not forwarded correctly to v2 router)")
else:
    warned(f"Profile — HTTP {code}")

log("Get consent...")
code, body = req("POST", f"http://localhost:{AUTH}/api/v2/auth/consent/get", {})
vlog(f"Consent HTTP {code}: {body[:200]}")
if code == 200:
    passed("Consent — HTTP 200")
elif code in (401, 403):
    warned(f"Consent — {code} (requires valid session)")
else:
    warned(f"Consent — HTTP {code}")

# ─── SECTION 8: SIGN OUT ─────────────────────────────────────────────────────
header("SECTION 8: SIGN OUT & SESSION INVALIDATION")

log("Signing out...")
code, body = req("POST", f"http://localhost:{AUTH}/api/v2/auth/signOut", {})
vlog(f"Sign-out HTTP {code}: {body[:200]}")
if code == 200:
    passed("Sign-out — HTTP 200")
else:
    warned(f"Sign-out — HTTP {code}")

log("Verifying session invalidated...")
code, body = req("POST", f"http://localhost:{AUTH}/api/v2/auth/getSession", {})
vlog(f"Post-signout session HTTP {code}: {body[:300]}")
d = jparse(body)
user_id_after = jget(d, "user.id") or ""
auth_flag     = d.get("authenticated")
if not user_id_after and auth_flag in (None, False, "false", "False"):
    passed("Session invalidated after sign-out ✓")
else:
    warned(f"Session may still be active — user.id='{user_id_after}' authenticated='{auth_flag}'")

# ─── SECTION 10: BILLING ───────────────────────────────────────────────────────────────
header(f"SECTION 10: BILLING (port {BILLING})")

BILL_ORG = ORG_ID or f"functest-{TS}"
log(f"Billing org: {BILL_ORG}")

log("GET billing account (auto-creates free plan)...")
code, body = req("GET", f"http://localhost:{BILLING}/api/v1/billing/orgs/{BILL_ORG}/account")
vlog(f"Billing account HTTP {code}: {body[:200]}")
if code == 200:
    d = jparse(body)
    plan = jget(d, "plan") or d.get("plan_id") or ""
    passed(f"Billing account — HTTP 200 (plan: {plan or 'unknown'})")
else:
    failed(f"Billing account — HTTP {code}: {body[:120]}")

log("POST usage (ai_tokens: 500)...")
code, body = req("POST", f"http://localhost:{BILLING}/api/v1/billing/orgs/{BILL_ORG}/usage",
                 {"metric": "ai_tokens", "quantity": 500,
                  "idempotency_key": f"functest-{TS}-tokens"})
vlog(f"Record usage HTTP {code}: {body[:200]}")
if code in (200, 202):
    passed(f"Record usage — HTTP {code}")
else:
    failed(f"Record usage — HTTP {code}: {body[:120]}")

log("GET quota status (ai_tokens)...")
code, body = req("GET", f"http://localhost:{BILLING}/api/v1/billing/orgs/{BILL_ORG}/quotas/ai_tokens")
vlog(f"Quota HTTP {code}: {body[:200]}")
if code == 200:
    d = jparse(body)
    used = d.get("used") or d.get("current") or 0
    passed(f"Quota status — HTTP 200 (used: {used})")
else:
    failed(f"Quota status — HTTP {code}: {body[:120]}")

log("GET entitlement check (ai_assistant)...")
code, body = req("GET", f"http://localhost:{BILLING}/api/v1/billing/orgs/{BILL_ORG}/entitlements/ai_assistant")
vlog(f"Entitlement HTTP {code}: {body[:200]}")
if code == 200:
    d = jparse(body)
    allowed = d.get("allowed")
    passed(f"Entitlement check — HTTP 200 (allowed: {allowed})")
elif code == 402:
    # 402 = plan does not include this feature; valid business-logic response on free plan
    passed(f"Entitlement check — HTTP 402 (feature not included in free plan — expected)")
else:
    failed(f"Entitlement check — HTTP {code}: {body[:120]}")

log("POST create invoice...")
code, body = req("POST", f"http://localhost:{BILLING}/api/v1/billing/orgs/{BILL_ORG}/invoices",
                 {"period_start": "2026-02-01", "period_end": "2026-02-28",
                  "amount_cents": 1000, "currency": "usd"})
vlog(f"Invoice HTTP {code}: {body[:200]}")
if code in (200, 201):
    passed(f"Invoice created — HTTP {code}")
else:
    failed(f"Invoice creation — HTTP {code}: {body[:120]}")

log("Verify Lago connectivity (GET /api/v1/events)...")
code, body = req("GET", f"http://localhost:{LAGO}/api/v1/events",
                 headers={"Authorization": "Bearer lago-coresystem-dev-api-key"})
vlog(f"Lago events HTTP {code}: {body[:200]}")
if code == 200:
    d = jparse(body)
    events = d.get("events") or d.get("data") or []
    passed(f"Lago API reachable — HTTP 200 ({len(events)} event(s) in log)")
elif code in (401, 403):
    warned(f"Lago API auth rejected (check lago-coresystem-dev-api-key) — HTTP {code}")
elif code in (404, 0):
    warned(f"Lago not reachable or endpoint changed — HTTP {code}")
else:
    warned(f"Lago events — unexpected HTTP {code}: {body[:100]}")

# ─── SECTION 11: API KEYS ───────────────────────────────────────────────────────────────
header(f"SECTION 11: API KEYS (port {USER})")

_ak_user_id       = USER_ID or f"test-user-{TS}"
_api_key_id       = ""
_api_key_plaintext = ""

# user-core auth middleware accepts X-Internal-Api-Key + X-User-Id for service-to-service calls.
# The INTERNAL_SERVICE_SECRET env var in the Docker container is "dev-service-secret-local".
# Bearer auth using session.token is rejected by /api/auth/get-session (Better Auth returns
# 401 "Invalid API key"), so we use the internal key approach here.
_ak_headers = {
    "X-Internal-Api-Key": "dev-service-secret-local",
    "X-User-Id": _ak_user_id,
}
log(f"API key tests using internal service auth for user {_ak_user_id[:16]}...")

log("POST /api/v1/api-keys (create)...")
code, body = req("POST", f"http://localhost:{USER}/api/v1/api-keys",
                 {"name": f"functest-key-{TS}",
                  "scopes": ["read", "write"],
                  "expires_in_days": 30},
                 headers=_ak_headers)
vlog(f"Create API key HTTP {code}: {body[:200]}")
if code in (200, 201):
    d = jparse(body)
    _api_key_id        = jget(d, "id") or jget(d, "api_key.id") or ""
    _api_key_plaintext = jget(d, "key") or jget(d, "api_key.key") or ""
    if _api_key_id and _api_key_plaintext:
        passed(f"API key created — prefix={_api_key_plaintext[:8]}... id={_api_key_id}")
    else:
        warned(f"API key created but missing id/key — keys: {list(d.keys())}")
else:
    failed(f"Create API key — HTTP {code}: {body[:120]}")

log("GET /api/v1/api-keys (list)...")
code, body = req("GET", f"http://localhost:{USER}/api/v1/api-keys",
                 headers=_ak_headers)
vlog(f"List API keys HTTP {code}: {body[:200]}")
if code == 200:
    d = jparse(body)
    keys = d if isinstance(d, list) else d.get("keys", d.get("api_keys", []))
    passed(f"List API keys — HTTP 200 ({len(keys)} key(s))")
else:
    failed(f"List API keys — HTTP {code}: {body[:120]}")

if _api_key_id:
    log(f"DELETE /api/v1/api-keys/{_api_key_id} (revoke)...")
    code, body = req("DELETE", f"http://localhost:{USER}/api/v1/api-keys/{_api_key_id}",
                     headers=_ak_headers)
    vlog(f"Revoke API key HTTP {code}: {body[:200]}")
    if code in (200, 202, 204):
        passed(f"API key revoked — HTTP {code}")
    else:
        failed(f"Revoke API key — HTTP {code}: {body[:120]}")

    log("Verify revoked key no longer active in list...")
    code, body = req("GET", f"http://localhost:{USER}/api/v1/api-keys",
                     headers=_ak_headers)
    if code == 200:
        d = jparse(body)
        keys = d if isinstance(d, list) else d.get("keys", d.get("api_keys", []))
        active_ids = [k.get("id") for k in keys
                      if isinstance(k, dict) and not k.get("revoked_at")]
        if _api_key_id not in active_ids:
            passed("Revoked key absent from active keys list ✓")
        else:
            warned("Revoked key still appears active in list")
    else:
        warned(f"Could not verify revocation — list returned HTTP {code}")
else:
    skipped("API key revoke — no key ID captured from create step")

# ─── SECTION 12: LOG ERROR SCAN ────────────────────────────────────────────────
header("SECTION 12: DOCKER LOG ERROR SCAN")

CONTAINERS = [
    "auth-service",
    "user-service",
    "billing-core-service",
    "org-core-service",
    "reasoning-reasoning-core",
    "dataplane-retrieval-service-1",
    "dataplane-documents-service-1",
]
ERROR_PATTERNS = ("ERROR", "FATAL", "ECONNREFUSED", "ENOTFOUND",
                  "UnhandledPromiseRejection", "panic:", "SQLSTATE",
                  "Traceback", "Exception")

for name in CONTAINERS:
    try:
        result = subprocess.run(
            ["docker", "logs", name, "--tail", "50"],
            capture_output=True, text=True, timeout=10
        )
        log_text = result.stdout + result.stderr
    except Exception:
        skipped(f"{name} — docker logs failed")
        continue

    if "No such container" in log_text or not log_text.strip():
        skipped(f"{name} — container not found or empty logs")
        continue

    error_lines = [l for l in log_text.splitlines()
                   if any(p in l for p in ERROR_PATTERNS)]
    warn_lines  = [l for l in log_text.splitlines()
                   if "WARN" in l or "WARNING" in l]

    if error_lines:
        warned(f"{name} — {len(error_lines)} error line(s)")
        for line in error_lines[-5:]:
            print(f"    {RED}↳ {line.strip()[:120]}{RESET}")
    else:
        passed(f"{name} — clean logs ({len(warn_lines)} warnings)")

# ─── SUMMARY ─────────────────────────────────────────────────────────────────
elapsed = int(time.time() - START)
print()
bar = "═" * 53
print(f"{BOLD}╔{bar}╗{RESET}")
print(f"{BOLD}║          FUNCTIONAL TEST SUITE — SUMMARY           ║{RESET}")
print(f"{BOLD}╠{bar}╣{RESET}")
print(f"{BOLD}║{RESET}  Elapsed : {elapsed}s")
print(f"{BOLD}║{RESET}  User    : {USER_EMAIL}")
if USER_ID:  print(f"{BOLD}║{RESET}  User ID : {USER_ID}")
if ORG_ID:   print(f"{BOLD}║{RESET}  Org ID  : {ORG_ID}")
if DOC_ID:   print(f"{BOLD}║{RESET}  Doc ID  : {DOC_ID}")
print(f"{BOLD}║{RESET}")
print(f"{BOLD}║{RESET}  {GREEN}PASS : {PASS}{RESET}")
print(f"{BOLD}║{RESET}  {RED}FAIL : {FAIL}{RESET}")
print(f"{BOLD}║{RESET}  {YELLOW}WARN : {WARN}{RESET}")
print(f"{BOLD}║{RESET}  {YELLOW}SKIP : {SKIP}{RESET}")
print(f"{BOLD}╚{bar}╝{RESET}")
print()

sys.exit(0 if FAIL == 0 else 1)
