#!/usr/bin/env python3
"""
Quarry API Performance Benchmark
Measures real latency (avg/min/max) and concurrency for all endpoint types.
"""

import time
import statistics
import concurrent.futures
import subprocess
import json
try:
    import requests
except ModuleNotFoundError:
    # fallback to urllib if requests is not available
    import urllib.request as _urllib_request
    import urllib.error as _urllib_error

    class _SimpleResponse:
        def __init__(self, code, text):
            self.status_code = code
            self.text = text

        def json(self):
            try:
                return json.loads(self.text)
            except Exception:
                return {}

    class _SimpleRequests:
        def __init__(self):
            pass

        def _request(self, method, url, headers=None, data=None, json_body=None, timeout=None):
            hdrs = dict(headers or {})
            body = None
            if json_body is not None:
                body = json.dumps(json_body).encode('utf-8')
                hdrs.setdefault('Content-Type', 'application/json')
            elif data is not None:
                body = data.encode('utf-8') if isinstance(data, str) else data

            req = _urllib_request.Request(url, data=body, headers=hdrs or {}, method=method)
            try:
                with _urllib_request.urlopen(req, timeout=timeout) as resp:
                    txt = resp.read().decode('utf-8')
                    return _SimpleResponse(resp.getcode(), txt)
            except _urllib_error.HTTPError as e:
                try:
                    txt = e.read().decode('utf-8')
                except Exception:
                    txt = ''
                return _SimpleResponse(e.code, txt)

        def post(self, url, headers=None, data=None, json=None, timeout=None):
            return self._request('POST', url, headers=headers, data=data, json_body=json, timeout=timeout)

        def get(self, url, headers=None, timeout=None):
            return self._request('GET', url, headers=headers, timeout=timeout)

    requests = _SimpleRequests()

import os

# Allow overriding the Quarry base URL via env `QUARRY_BASE`; default to port 9090 where services run.
BASE = os.environ.get("QUARRY_BASE", "http://localhost:9090")

# Read API key from .env
try:
    key_raw = subprocess.check_output(
        "grep '^QUARRY_API_KEY' .env | cut -d= -f2", shell=True
    ).decode().strip()
    HDRS = {"X-API-Key": key_raw, "Content-Type": "application/json"}
except Exception:
    HDRS = {"Content-Type": "application/json"}


def bench(label, fn, n=3, timeout=30):
    times = []
    status = None
    for _ in range(n):
        t = time.perf_counter()
        try:
            r = fn()
            status = r.status_code
        except Exception as e:
            status = f"ERR:{e}"
        ms = round((time.perf_counter() - t) * 1000)
        times.append(ms)

    avg = statistics.mean(times)
    print(f"  {label:<45} avg={avg:>6.0f}ms  min={min(times):>5}ms  max={max(times):>5}ms  status={status}")
    return avg


def section(title):
    print(f"\n{'─'*60}")
    print(f"  {title}")
    print(f"{'─'*60}")


results = {}

print("=" * 60)
print("  QUARRY API — PERFORMANCE BENCHMARK")
print("=" * 60)

section("Infrastructure / Health")
results["health"] = bench("/health", lambda: requests.get(f"{BASE}/health", headers=HDRS))
results["ready"] = bench("/ready", lambda: requests.get(f"{BASE}/ready", headers=HDRS))
results["metrics"] = bench("/metrics", lambda: requests.get(f"{BASE}/metrics", headers=HDRS))
results["modules"] = bench("/v1/modules", lambda: requests.get(f"{BASE}/v1/modules", headers=HDRS))

section("URL Discovery")
results["map"] = bench(
    "/v1/map  (limit=10)",
    lambda: requests.post(f"{BASE}/v1/map", headers=HDRS, json={"url": "https://example.com", "limit": 10}),
)
results["search"] = bench(
    "/v1/search  (keyword match)",
    lambda: requests.post(f"{BASE}/v1/search", headers=HDRS, json={"url": "https://example.com", "query": "example", "limit": 5}),
)

section("Scrape — quick module (note: single-request, synchronous live fetch)")
# Scrape benchmarks are run with n=1 since they involve live network fetching (~8-30s)
# Cache warm-up
print("  warming cache for example.com ...")
_warmup = requests.post(f"{BASE}/v1/scrape", headers=HDRS,
    json={"url": "https://example.com", "collection": "quick", "formats": ["markdown"]},
    timeout=60)
results["scrape_warm"] = bench(
    "/v1/scrape  (cache warm, markdown)",
    lambda: requests.post(f"{BASE}/v1/scrape", headers=HDRS,
        json={"url": "https://example.com", "collection": "quick", "formats": ["markdown"]},
        timeout=60),
    n=1,
)
results["scrape_cold"] = bench(
    "/v1/scrape  (cold, markdown only)",
    lambda: requests.post(f"{BASE}/v1/scrape", headers=HDRS,
        json={"url": "https://httpbin.org/html", "collection": "quick", "formats": ["markdown"]},
        timeout=60),
    n=1,
)
results["scrape_multi_format"] = bench(
    "/v1/scrape  (markdown + json formats)",
    lambda: requests.post(f"{BASE}/v1/scrape", headers=HDRS,
        json={"url": "https://example.com", "collection": "quick", "formats": ["markdown", "json"]},
        timeout=60),
    n=1,
)

section("Change Tracking")
results["change_dryrun"] = bench(
    "/v1/change/check  dryRun=true",
    lambda: requests.post(f"{BASE}/v1/change/check", headers=HDRS, json={"url": "https://example.com", "dryRun": True}),
)
results["change_persist"] = bench(
    "/v1/change/check  persist",
    lambda: requests.post(f"{BASE}/v1/change/check", headers=HDRS, json={"url": "https://example.com"}),
)
results["change_latest"] = bench(
    "/v1/change/latest",
    lambda: requests.get(f"{BASE}/v1/change/latest", headers=HDRS),
)

section("Async Job Dispatch")
_crawl_r = requests.post(f"{BASE}/v1/crawl", headers=HDRS, json={"url": "https://example.com", "limit": 2, "collection": "quick"})
jid = (_crawl_r.json().get("job") or {}).get("id") or _crawl_r.json().get("id", "missing")
results["crawl_dispatch"] = bench(
    "/v1/crawl  dispatch (async)",
    lambda: requests.post(f"{BASE}/v1/crawl", headers=HDRS, json={"url": "https://example.com", "limit": 2, "collection": "quick"}),
    n=2,
)
results["job_status"] = bench(
    f"/v1/jobs/:id  status",
    lambda: requests.get(f"{BASE}/v1/jobs/{jid}", headers=HDRS),
)

_batch_r = requests.post(f"{BASE}/v1/batch", headers=HDRS, json={"urls": ["https://example.com"], "collection": "quick"})
bid = _batch_r.json().get("id", "missing")
results["batch_dispatch"] = bench(
    "/v1/batch  dispatch",
    lambda: requests.post(f"{BASE}/v1/batch", headers=HDRS, json={"urls": ["https://example.com"], "collection": "quick"}),
    n=2,
)
results["batch_status"] = bench(
    f"/v1/batch/:id  status",
    lambda: requests.get(f"{BASE}/v1/batch/{bid}", headers=HDRS),
)

section("Agent Mode")
results["agent"] = bench(
    "/v1/agent",
    lambda: requests.post(f"{BASE}/v1/agent", headers=HDRS, json={"url": "https://example.com", "goal": "extract main content"}),
    n=1,
)

section("Concurrency — 20× parallel /health")
with concurrent.futures.ThreadPoolExecutor(max_workers=20) as ex:
    t = time.perf_counter()
    futs = [ex.submit(lambda: requests.get(f"{BASE}/health", headers=HDRS)) for _ in range(20)]
    statuses = [f.result().status_code for f in futs]
    elapsed_ms = (time.perf_counter() - t) * 1000
rps = 20_000 / elapsed_ms
print(f"  20 requests completed in {elapsed_ms:.0f}ms →  {rps:.1f} req/s")
print(f"  All 200s: {all(s == 200 for s in statuses)}")

section("Concurrency — 5× parallel /v1/map")
with concurrent.futures.ThreadPoolExecutor(max_workers=5) as ex:
    t = time.perf_counter()
    futs = [
        ex.submit(
            lambda: requests.post(f"{BASE}/v1/map", headers=HDRS, json={"url": "https://example.com", "limit": 10})
        )
        for _ in range(5)
    ]
    map_statuses = [f.result().status_code for f in futs]
    elapsed_map = (time.perf_counter() - t) * 1000
print(f"  5 /v1/map requests: {elapsed_map:.0f}ms total  statuses={map_statuses}")

# ── Summary table ─────────────────────────────────────────────
print("\n" + "=" * 60)
print("  SUMMARY TABLE (ms)")
print("=" * 60)
for k, v in results.items():
    print(f"  {k:<35}  {v:>6.0f}ms")
