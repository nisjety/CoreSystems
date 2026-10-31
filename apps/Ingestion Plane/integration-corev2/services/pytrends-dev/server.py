"""Local-only, bounded pytrends worker. No account cookies, proxies or retries."""
import datetime as dt
import json
import os
import subprocess
import sys
import threading
import time
from collections import OrderedDict
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


def stamp(seconds=None):
    return dt.datetime.fromtimestamp(time.time() if seconds is None else seconds, dt.timezone.utc).isoformat(timespec="seconds").replace("+00:00", "Z")


def validate(value):
    if not isinstance(value, dict) or set(value) != {"terms", "geo", "timeframe"}:
        raise ValueError("Invalid request")
    terms = value["terms"]
    if not isinstance(terms, list) or not 1 <= len(terms) <= 5 or any(
        not isinstance(term, str) or not 1 <= len(term.encode("utf-8")) <= 80
        or term != term.strip() or not term.isprintable() for term in terms
    ) or len(set(terms)) != len(terms):
        raise ValueError("Invalid terms")
    if value["timeframe"] not in ("today 3-m", "today 12-m", "today 5-y"):
        raise ValueError("Invalid period")
    geo = value["geo"]
    if not isinstance(geo, str) or geo and (len(geo) != 2 or not geo.isascii() or not geo.isalpha() or geo != geo.upper()):
        raise ValueError("Invalid country")
    return value


def result(query, status, rows=None):
    return dict(query, source="pytrends_dev", scale="relative_0_100_per_request", status=status,
                observedAt=stamp(), rows=rows or [])


def collect(query):
    from pytrends.request import TrendReq
    from pytrends.exceptions import TooManyRequestsError, ResponseError
    from requests.exceptions import Timeout
    try:
        client = TrendReq(hl="en-US", tz=0, timeout=(4, 7), retries=0, backoff_factor=0)
        client.build_payload(query["terms"], timeframe=query["timeframe"], geo=query["geo"])
        frame = client.interest_over_time()
        if frame.empty:
            return result(query, "no_data")
        if len(frame) > 400:
            return result(query, "upstream_error")
        rows = []
        for date, record in frame.iterrows():
            values = [int(record[term]) for term in query["terms"]]
            if any(v < 0 or v > 100 for v in values):
                return result(query, "upstream_error")
            rows.append({"date": date.strftime("%Y-%m-%d"), "values": values, "partial": bool(record.get("isPartial", False))})
        return result(query, "available", rows)
    except TooManyRequestsError:
        return result(query, "rate_limited")
    except Timeout:
        return result(query, "timeout")
    except ResponseError as error:
        return result(query, "blocked" if error.response.status_code in (401, 403) else "upstream_error")
    except Exception:
        # Never return upstream HTML, cookies, URLs or exception text.
        return result(query, "upstream_error")


class Collector:
    def __init__(self, run=None, now=time.time):
        self.run = run or self.child
        self.now = now
        self.cache = OrderedDict()
        self.lock = threading.Lock()
        self.next_request = 0

    @staticmethod
    def child(query):
        try:
            child = subprocess.run([sys.executable, __file__, "collect"], input=json.dumps(query),
                                   text=True, capture_output=True, timeout=25, check=True)
            if len(child.stdout) > 131072:
                return result(query, "upstream_error")
            return json.loads(child.stdout)
        except subprocess.TimeoutExpired:
            return result(query, "timeout")
        except Exception:
            return result(query, "upstream_error")

    def get(self, query):
        key = json.dumps(validate(query), sort_keys=True)
        # No wait queue: one upstream request at a time across all callers.
        if not self.lock.acquire(blocking=False):
            return dict(result(query, "cooldown"), retryAt=stamp(self.now() + 60))
        try:
            now = self.now()
            cached = self.cache.get(key)
            if cached and cached[0] > now:
                return dict(cached[1], cached=True)
            if now < self.next_request:
                return dict(result(query, "cooldown"), retryAt=stamp(self.next_request))
            value = self.run(query)
            success = value["status"] in ("available", "no_data")
            delay = 60 if success else 3600 if value["status"] in ("rate_limited", "blocked") else 900
            self.next_request = self.now() + delay
            if not success:
                value["retryAt"] = stamp(self.next_request)
            value["cached"] = False
            ttl = 86400 if success else delay
            self.cache[key] = (self.now() + ttl, value)
            self.cache.move_to_end(key)
            while len(self.cache) > 64:
                self.cache.popitem(last=False)
            return value
        finally:
            self.lock.release()


collector = Collector()


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass  # Search terms and requests are deliberately not logged.

    def send(self, status, value):
        data = json.dumps(value, allow_nan=False).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        self.send(200 if self.path == "/health" else 404, {"status": "ok"} if self.path == "/health" else {})

    def do_POST(self):
        if self.path != "/report" or os.environ.get("PYTRENDS_DEV_ENABLED") != "1":
            return self.send(404, {})
        self.connection.settimeout(5)
        try:
            length = int(self.headers.get("Content-Length", "0"))
            if not 1 <= length <= 2048 or self.headers.get("Content-Type") != "application/json":
                raise ValueError("Invalid body")
            query = validate(json.loads(self.rfile.read(length)))
        except (ValueError, TimeoutError):
            return self.send(400, {"error": "invalid_request"})
        self.send(200, collector.get(query))


if __name__ == "__main__":
    if sys.argv[-1] == "collect":
        print(json.dumps(collect(validate(json.load(sys.stdin))), allow_nan=False))
    else:
        ThreadingHTTPServer(("0.0.0.0", 8000), Handler).serve_forever()
