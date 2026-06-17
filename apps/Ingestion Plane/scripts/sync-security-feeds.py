#!/usr/bin/env python3
"""Sync URL phishing/malware feeds into Quarry's local security snapshot.

The script writes quarry-security's native JSON shape:
    {"blocklist": ["bad.example"], "allowlist": []}

Feed downloads are explicit operator actions. A metadata sidecar enforces a
10-minute minimum interval by default, matching URLhaus guidance.
"""

from __future__ import annotations

import argparse
import csv
import io
import json
import os
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

DEFAULT_URLHAUS_CSV_URL = "https://urlhaus.abuse.ch/downloads/csv_recent/"
DEFAULT_OUTPUT = "config/security-feeds/quarry-security.json"
DEFAULT_META_OUTPUT = "config/security-feeds/quarry-security.meta.json"
MIN_SYNC_INTERVAL_S = 600


def env_bool(name: str, default: bool = False) -> bool:
    value = os.getenv(name)
    if value is None:
        return default
    return value.strip().lower() in {"1", "true", "yes", "on"}


def fetch_text(url: str, timeout_s: float) -> str:
    request = urllib.request.Request(
        url,
        headers={"User-Agent": "CoreSystem-SecurityFeedSync/1.0"},
    )
    with urllib.request.urlopen(request, timeout=timeout_s) as response:
        encoding = response.headers.get_content_charset() or "utf-8"
        return response.read().decode(encoding, errors="replace")


def normalize_host(url: str) -> str | None:
    parsed = urllib.parse.urlparse(url.strip())
    host = parsed.hostname
    if not host:
        return None
    try:
        host = host.encode("idna").decode("ascii")
    except UnicodeError:
        return None
    return host.lower().rstrip(".")


def iter_csv_urls(payload: str) -> set[str]:
    cleaned = "\n".join(
        line
        for line in payload.splitlines()
        if line.strip() and not line.startswith("#")
    )
    if not cleaned:
        return set()

    rows = csv.DictReader(io.StringIO(cleaned))
    urls: set[str] = set()
    if rows.fieldnames and "url" in {field.lower() for field in rows.fieldnames}:
        url_field = next(field for field in rows.fieldnames if field.lower() == "url")
        for row in rows:
            value = row.get(url_field)
            if value:
                urls.add(value)
        return urls

    for row in csv.reader(io.StringIO(cleaned)):
        for value in row:
            if value.startswith(("http://", "https://")):
                urls.add(value)
                break
    return urls


def should_skip(meta_path: Path, force: bool) -> bool:
    if force or not meta_path.exists():
        return False
    try:
        meta = json.loads(meta_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return False
    last_synced_at = float(meta.get("synced_at_unix", 0))
    return time.time() - last_synced_at < MIN_SYNC_INTERVAL_S


def write_json(path: Path, payload: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(
        json.dumps(payload, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    tmp.replace(path)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--output",
        default=os.getenv("QUARRY_SECURITY_SNAPSHOT_PATH", DEFAULT_OUTPUT),
    )
    parser.add_argument(
        "--meta-output",
        default=os.getenv("QUARRY_SECURITY_META_PATH", DEFAULT_META_OUTPUT),
    )
    parser.add_argument(
        "--force",
        action="store_true",
        default=env_bool("SECURITY_FEED_FORCE"),
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=float(os.getenv("SECURITY_FEED_TIMEOUT_S", "30")),
    )
    args = parser.parse_args()

    output = Path(args.output)
    meta_output = Path(args.meta_output)
    if should_skip(meta_output, args.force):
        print("security feed sync skipped: last sync is newer than 10 minutes")
        return 0

    feeds = {
        "urlhaus": os.getenv("URLHAUS_CSV_URL", DEFAULT_URLHAUS_CSV_URL),
        "phishtank": os.getenv("PHISHTANK_CSV_URL", "").strip(),
    }
    max_hosts = int(os.getenv("SECURITY_FEED_MAX_HOSTS", "250000"))

    hosts: set[str] = set()
    feed_stats: dict[str, dict[str, int | str]] = {}
    for name, url in feeds.items():
        if not url:
            feed_stats[name] = {"status": "skipped", "hosts": 0}
            continue
        try:
            payload = fetch_text(url, args.timeout)
            feed_hosts = {
                host
                for candidate in iter_csv_urls(payload)
                if (host := normalize_host(candidate))
            }
            hosts.update(feed_hosts)
            feed_stats[name] = {"status": "ok", "hosts": len(feed_hosts)}
        except (urllib.error.URLError, TimeoutError, OSError) as exc:
            feed_stats[name] = {"status": exc.__class__.__name__, "hosts": 0}

    sorted_hosts = sorted(hosts)[:max_hosts]
    write_json(output, {"allowlist": [], "blocklist": sorted_hosts})
    write_json(
        meta_output,
        {
            "feeds": feed_stats,
            "host_count": len(sorted_hosts),
            "synced_at_unix": time.time(),
        },
    )
    print(f"wrote {len(sorted_hosts)} blocked hosts to {output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
