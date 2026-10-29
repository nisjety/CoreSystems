#!/usr/bin/env python3
"""Mirror selected NVD JSON 2.0 feeds into Control Plane local storage."""

from __future__ import annotations

import argparse
import gzip
import json
import os
import sys
import time
import urllib.request
from pathlib import Path

DEFAULT_OUTPUT_DIR = "config/security/nvd"
DEFAULT_FEEDS = {
    "cve-recent": "https://nvd.nist.gov/feeds/json/cve/2.0/nvdcve-2.0-recent.json.gz",
    "cve-modified": "https://nvd.nist.gov/feeds/json/cve/2.0/nvdcve-2.0-modified.json.gz",
}


def fetch_bytes(url: str, timeout_s: float) -> bytes:
    request = urllib.request.Request(
        url,
        headers={"User-Agent": "CoreSystem-NVDSync/1.0"},
    )
    with urllib.request.urlopen(request, timeout=timeout_s) as response:
        return response.read()


def decode_payload(payload: bytes, url: str) -> str:
    if url.endswith(".gz"):
        return gzip.decompress(payload).decode("utf-8", errors="replace")
    return payload.decode("utf-8", errors="replace")


def configured_feeds() -> dict[str, str]:
    override = os.getenv("NVD_FEED_URLS", "").strip()
    if not override:
        return DEFAULT_FEEDS
    feeds: dict[str, str] = {}
    for item in override.split(","):
        if not item.strip():
            continue
        name, _, url = item.partition("=")
        feeds[name.strip()] = url.strip() if url else name.strip()
    return feeds


def write_feed(
    output_dir: Path,
    name: str,
    url: str,
    timeout_s: float,
) -> dict[str, int | str]:
    payload = fetch_bytes(url, timeout_s)
    decoded = decode_payload(payload, url)
    parsed = json.loads(decoded)
    output_path = output_dir / f"{name}.json"
    output_path.write_text(
        json.dumps(parsed, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    return {
        "name": name,
        "source_url": url,
        "path": str(output_path),
        "bytes": output_path.stat().st_size,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--output-dir",
        default=os.getenv("NVD_FEED_CACHE_PATH", DEFAULT_OUTPUT_DIR),
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=float(os.getenv("NVD_FEED_TIMEOUT_S", "60")),
    )
    args = parser.parse_args()

    output_dir = Path(args.output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)

    feeds = []
    for name, url in configured_feeds().items():
        feeds.append(write_feed(output_dir, name, url, args.timeout))

    manifest = {
        "schema_version": 1,
        "generated_at_unix": time.time(),
        "feeds": feeds,
    }
    (output_dir / "manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(f"wrote {len(feeds)} NVD feeds to {output_dir}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
