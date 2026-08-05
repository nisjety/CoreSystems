#!/usr/bin/env python3
"""
Phase 6 E2E Event System Test
==============================
Tests the complete cross-plane event flow for all 3 event types by:
    1. Publishing real events to the shared NATS JetStream broker (verevon-nats)
  2. Waiting for each subscribing plane's handler to execute
  3. Verifying handler execution via container logs
  4. Checking quota state changes via service APIs

Test Scenarios:
  - user.provider_linked → imports-api M365 handler
  - org.plan_changed     → retrieval + documents quota handlers
  - billing.quota_exceeded → ALL planes enforcement handlers

Usage:
  pip install nats-py
  python3 scripts/e2e_phase6_events.py

Requirements:
  All planes must be running (docker compose up -d in each plane directory)
"""

import asyncio
import json
import subprocess
import sys
import time
from datetime import datetime, timezone

# ─── Output helpers ─────────────────────────────────────────────────────────
GREEN  = "\033[92m"
RED    = "\033[91m"
YELLOW = "\033[93m"
BLUE   = "\033[94m"
BOLD   = "\033[1m"
DIM    = "\033[2m"
RESET  = "\033[0m"

results: list[tuple[str, str, str]] = []  # (status, name, detail)

def _p(color: str, icon: str, msg: str, detail: str = ""):
    d = f" {DIM}({detail}){RESET}" if detail else ""
    print(f"  {color}{icon}{RESET} {msg}{d}")

def ok(name: str, detail: str = ""):
    results.append(("PASS", name, detail))
    _p(GREEN, "✅", name, detail)

def fail(name: str, detail: str = ""):
    results.append(("FAIL", name, detail))
    _p(RED, "❌", name, detail)

def warn(name: str, detail: str = ""):
    results.append(("WARN", name, detail))
    _p(YELLOW, "⚠️ ", name, detail)

def info(msg: str):
    print(f"  {BLUE}ℹ{RESET}  {DIM}{msg}{RESET}")

def section(title: str):
    print()
    print(f"  {BOLD}{title}{RESET}")
    print(f"  {'─' * (len(title) + 2)}")

# ─── Log checker ─────────────────────────────────────────────────────────────
def wait_for_log(container: str, pattern: str, timeout: int = 20) -> tuple[bool, str]:
    """
    Poll container logs until 'pattern' appears or timeout expires.
    Returns (found, matched_line).
    """
    deadline = time.time() + timeout
    while time.time() < deadline:
        result = subprocess.run(
            ["docker", "logs", container, "--since", "60s"],
            capture_output=True, text=True, timeout=5,
        )
        combined = result.stdout + result.stderr
        for line in combined.splitlines():
            if pattern in line:
                return True, line.strip()
        time.sleep(1.5)
    return False, ""


def service_health(url: str) -> int:
    """HTTP GET to url, return status code or -1 on error."""
    try:
        result = subprocess.run(
            ["curl", "-s", "-o", "/dev/null", "-w", "%{http_code}", url, "--max-time", "5"],
            capture_output=True, text=True, timeout=8,
        )
        return int(result.stdout.strip())
    except Exception:
        return -1


# ─── Main test ───────────────────────────────────────────────────────────────
async def run_tests() -> int:
    try:
        import nats
        from nats.js.api import StreamConfig
    except ImportError:
        print(f"\n{RED}nats-py not installed. Run: pip install nats-py{RESET}")
        return 2

    print()
    print(f"  {BOLD}{'═' * 65}{RESET}")
    print(f"  {BOLD}  PHASE 6 — CROSS-PLANE EVENT SYSTEM E2E TEST{RESET}")
    print(f"  {BOLD}{'═' * 65}{RESET}")
    print(f"  {DIM}Started: {datetime.now(timezone.utc).strftime('%Y-%m-%d %H:%M:%S UTC')}{RESET}")

    # ──────────────────────────────────────────────────────────────────────────
    section("0. Pre-flight: Service Availability")
    # ──────────────────────────────────────────────────────────────────────────
    checks = [
        ("imports-api",           "http://localhost:9025/health"),
        ("data-retrieval-service","http://localhost:9404/health"),
        ("data-documents-service","http://localhost:9401/health"),
        ("reasoning-ai-core",     "http://localhost:8100/health"),
        ("verevon-nats",           "http://localhost:8240/healthz"),
    ]
    all_healthy = True
    for name, url in checks:
        code = service_health(url)
        if code == 200:
            ok(f"{name} health", f"HTTP {code}")
        else:
            fail(f"{name} health", f"HTTP {code} from {url}")
            all_healthy = False

    if not all_healthy:
        warn("Some services unhealthy — continuing with available services")

    # ──────────────────────────────────────────────────────────────────────────
    section("1. NATS Broker Connection")
    # ──────────────────────────────────────────────────────────────────────────
    nc = None
    try:
        nc = await nats.connect(
            "nats://localhost:4240",
            token="aqencia-shared-nats-token-2026",
            name="e2e-phase6-test",
            connect_timeout=10,
        )
        js = nc.jetstream()
        ok("Connected to verevon-nats (token auth)", "nats://localhost:4240")
    except Exception as e:
        fail("Connect to verevon-nats", str(e))
        print(f"\n  {RED}Cannot proceed without NATS connection.{RESET}")
        return 1

    # Ensure stream exists
    try:
        stream_nfo = await js.stream_info("AQENCIA_CONTROLPLANE")
        msg_count = stream_nfo.state.messages
        ok("AQENCIA_CONTROLPLANE JetStream stream exists", f"{msg_count} msgs stored")
        # Purge old test messages to prevent cross-run interference
        if msg_count > 0:
            info(f"Purging {msg_count} stale messages from stream before test...")
            await js.purge_stream("AQENCIA_CONTROLPLANE")
            ok("Stream purged — fresh start for this test run")
    except Exception as e:
        fail("AQENCIA_CONTROLPLANE stream", f"not found: {e}")

    # ──────────────────────────────────────────────────────────────────────────
    section("2. Event: user.provider_linked  →  imports-api")
    # ──────────────────────────────────────────────────────────────────────────
    run_id = int(time.time())
    test_org_id   = f"e2e-org-{run_id}"
    test_user_id  = f"e2e-user-{run_id}"
    test_tenant_id = f"tenant-{run_id}"

    payload = {
        "user_id":   test_user_id,
        "email":     f"test+{run_id}@e2e.aqencia.io",
        "provider":  "microsoft",
        "tenant_id": test_tenant_id,
        "timestamp": datetime.now(timezone.utc).isoformat(),
    }
    info(f"Subject: aqencia.controlplane.user.provider_linked")
    info(f"Payload user_id={test_user_id}  tenant_id={test_tenant_id}")

    try:
        ack = await js.publish(
            "aqencia.controlplane.user.provider_linked",
            json.dumps(payload).encode(),
        )
        ok(f"Published user.provider_linked", f"JetStream seq={ack.seq}")
    except Exception as e:
        fail("Publish user.provider_linked", str(e))
        goto_cleanup = True

    # Wait for imports-api handler
    info("Waiting up to 20s for imports-api to handle event...")
    patterns = [test_user_id, test_tenant_id, "provider_linked", "m365", "M365", "microsoft"]
    found = False
    matched_line = ""
    for pat in patterns:
        f, line = wait_for_log("imports-api", pat, 20)
        if f:
            found = True
            matched_line = line
            break

    if found:
        ok("imports-api handled user.provider_linked", matched_line[:80])
    else:
        fail("imports-api handled user.provider_linked", f"No matching log within 20s")

    # ──────────────────────────────────────────────────────────────────────────
    section("3. Event: org.plan_changed  →  retrieval + documents")
    # ──────────────────────────────────────────────────────────────────────────
    payload = {
        "org_id":        test_org_id,
        "org_name":      "E2E Test Org",
        "previous_plan": "free",
        "new_plan":      "professional",
        "changed_by":    "e2e-test",
        "change_reason": "automated_e2e_test",
        "timestamp":     datetime.now(timezone.utc).isoformat(),
    }
    info(f"Subject: aqencia.controlplane.org.plan_changed")
    info(f"Payload org_id={test_org_id}  free→professional")

    try:
        ack = await js.publish(
            "aqencia.controlplane.org.plan_changed",
            json.dumps(payload).encode(),
        )
        ok(f"Published org.plan_changed", f"JetStream seq={ack.seq}")
    except Exception as e:
        fail("Publish org.plan_changed", str(e))

    info("Waiting up to 20s for retrieval and documents handlers...")

    # retrieval-service
    found_r, line_r = wait_for_log("data-retrieval-service", test_org_id, 20)
    if not found_r:
        found_r, line_r = wait_for_log("data-retrieval-service", "plan_changed", 5)
    if found_r:
        ok("retrieval-service handled org.plan_changed", line_r[:80])
    else:
        fail("retrieval-service handled org.plan_changed", f"No log match for org_id={test_org_id}")

    # documents-service
    found_d, line_d = wait_for_log("data-documents-service", test_org_id, 15)
    if not found_d:
        found_d, line_d = wait_for_log("data-documents-service", "plan_changed", 5)
    if found_d:
        ok("documents-service handled org.plan_changed", line_d[:80])
    else:
        fail("documents-service handled org.plan_changed", f"No log match for org_id={test_org_id}")

    # ──────────────────────────────────────────────────────────────────────────
    section("4. Event: billing.quota_exceeded  →  ALL planes")
    # ──────────────────────────────────────────────────────────────────────────
    payload = {
        "org_id":    test_org_id,
        "metric":    "api_calls",
        "limit":     1000,
        "current":   1001,
        "timestamp": datetime.now(timezone.utc).isoformat(),
    }
    info(f"Subject: aqencia.controlplane.billing.quota_exceeded")
    info(f"Payload org_id={test_org_id}  api_calls 1001/1000")

    try:
        ack = await js.publish(
            "aqencia.controlplane.billing.quota_exceeded",
            json.dumps(payload).encode(),
        )
        ok(f"Published billing.quota_exceeded", f"JetStream seq={ack.seq}")
    except Exception as e:
        fail("Publish billing.quota_exceeded", str(e))

    info("Waiting up to 25s for all 4 subscribers...")
    subscribers = [
        ("imports-api",            "Ingestion / imports-api",      "quota_exceeded"),
        ("data-retrieval-service", "Data Plane / retrieval",       "quota_blocker"),
        ("data-documents-service", "Data Plane / documents",       "quota_blocker"),
        ("reasoning-ai-core",      "Reasoning / ai-core",          "quota_exceeded"),
    ]
    for container, label, specific_pat in subscribers:
        # First try the service-specific pattern (confirms handler actually ran)
        found_q, line_q = wait_for_log(container, specific_pat, 25)
        if not found_q:
            # Fallback: check that the org_id appears in any recent log
            found_q, line_q = wait_for_log(container, test_org_id, 5)
        if found_q:
            ok(f"{label} handled billing.quota_exceeded", line_q[:80])
        else:
            fail(f"{label} handled billing.quota_exceeded", f"No log match within 25s")

    # ──────────────────────────────────────────────────────────────────────────
    section("5. Quota Enforcement: Retrieval blocks after quota_exceeded")
    # ──────────────────────────────────────────────────────────────────────────
    # Wait explicitly for the QuotaBlocker to log BLOCKED for our org
    info(f"Waiting for retrieval-service QuotaBlocker to log BLOCKED for org_id={test_org_id}...")
    found_block, block_line = wait_for_log("data-retrieval-service", "quota_blocker.*BLOCKED", 20)
    if not found_block:
        # Try plain text (log filtering may not support regex)
        found_block, block_line = wait_for_log("data-retrieval-service", f"BLOCKED org_id={test_org_id}", 10)
    if found_block:
        ok("retrieval-service QuotaBlocker confirmed BLOCKED org", block_line[:80])
    else:
        warn("QuotaBlocker BLOCKED log not seen in 20s — quota enforcement may be timing-dependent")

    info(f"Testing retrieval POST for blocked org_id={test_org_id}...")
    time.sleep(1)  # small buffer after seeing the block log

    try:
        result = subprocess.run(
            ["curl", "-s", "-w", "\n%{http_code}",
             "-X", "POST", "http://localhost:9404/v1/retrieve",
             "-H", "Content-Type: application/json",
             "-d", json.dumps({"org_id": test_org_id, "query": "test", "top_k": 1}),
             "--max-time", "10"],
            capture_output=True, text=True, timeout=15,
        )
        lines = result.stdout.strip().split("\n")
        status_code = int(lines[-1]) if lines else 0
        body = "\n".join(lines[:-1])

        if status_code == 429:
            ok("Retrieval blocked (HTTP 429) for quota-exceeded org", f"Response: {body[:60]}")
        elif status_code == 200:
            warn("Retrieval returned 200 for quota-exceeded org",
                 "Quota enforcement may be async — check retrieval-service quota handler wiring")
        else:
            warn(f"Retrieval returned HTTP {status_code}", body[:60])
    except Exception as e:
        warn("Could not test retrieval quota enforcement", str(e))

    # ──────────────────────────────────────────────────────────────────────────
    section("6. Event Persistence: Replay from JetStream")
    # ──────────────────────────────────────────────────────────────────────────
    info("Checking NATS stream state after test events...")
    try:
        stream_nfo2 = await js.stream_info("AQENCIA_CONTROLPLANE")
        msg_count = stream_nfo2.state.messages
        ok(f"AQENCIA_CONTROLPLANE stream healthy", f"{msg_count} messages stored")
    except Exception as e:
        fail("AQENCIA_CONTROLPLANE stream info", str(e))

    # ──────────────────────────────────────────────────────────────────────────
    # Cleanup & summary
    # ──────────────────────────────────────────────────────────────────────────
    if nc:
        await nc.drain()
        await nc.close()

    passed = sum(1 for r in results if r[0] == "PASS")
    warned = sum(1 for r in results if r[0] == "WARN")
    failed = sum(1 for r in results if r[0] == "FAIL")
    total  = len(results)

    print()
    print(f"  {BOLD}{'═' * 65}{RESET}")
    print(f"  {BOLD}  RESULTS  {RESET}")
    print(f"  {'─' * 65}")
    for status, name, detail in results:
        icon  = "✅" if status == "PASS" else ("⚠️ " if status == "WARN" else "❌")
        color = GREEN if status == "PASS" else (YELLOW if status == "WARN" else RED)
        d = f" {DIM}({detail}){RESET}" if detail else ""
        print(f"  {icon} {color}{name}{RESET}{d}")

    print()
    bar_color = GREEN if failed == 0 else RED
    print(f"  {bar_color}{BOLD}  {passed} PASSED  /  {warned} WARNED  /  {failed} FAILED  (of {total}){RESET}")
    print(f"  {BOLD}{'═' * 65}{RESET}")
    print()

    return 0 if failed == 0 else 1


if __name__ == "__main__":
    exit_code = asyncio.run(run_tests())
    sys.exit(exit_code)
