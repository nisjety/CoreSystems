#!/usr/bin/env python3
"""Sync disposable-email-domains into the Control Plane local blocklist."""

from __future__ import annotations

import argparse
import os
import sys
import urllib.request
from pathlib import Path

DEFAULT_SOURCE_URL = (
    "https://raw.githubusercontent.com/disposable-email-domains/"
    "disposable-email-domains/master/disposable_email_blocklist.conf"
)
DEFAULT_OUTPUT = "config/security/disposable-email-domains.txt"


def fetch_text(url: str, timeout_s: float) -> str:
    request = urllib.request.Request(
        url,
        headers={"User-Agent": "CoreSystem-DisposableEmailSync/1.0"},
    )
    with urllib.request.urlopen(request, timeout=timeout_s) as response:
        encoding = response.headers.get_content_charset() or "utf-8"
        return response.read().decode(encoding, errors="replace")


def normalize_domains(content: str) -> list[str]:
    domains = {
        line.strip().lower().rstrip(".")
        for line in content.splitlines()
        if line.strip() and not line.strip().startswith("#")
    }
    return sorted(domains)


def write_domains(path: Path, domains: list[str], source_url: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(
        "# Generated from disposable-email-domains.\n"
        f"# Source: {source_url}\n"
        + "\n".join(domains)
        + "\n",
        encoding="utf-8",
    )
    tmp.replace(path)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--source-url",
        default=os.getenv("DISPOSABLE_EMAIL_SOURCE_URL", DEFAULT_SOURCE_URL),
    )
    parser.add_argument(
        "--output",
        default=os.getenv("DISPOSABLE_EMAIL_DOMAINS_FILE", DEFAULT_OUTPUT),
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=float(os.getenv("DISPOSABLE_EMAIL_SYNC_TIMEOUT_S", "30")),
    )
    args = parser.parse_args()

    domains = normalize_domains(fetch_text(args.source_url, args.timeout))
    write_domains(Path(args.output), domains, args.source_url)
    print(f"wrote {len(domains)} disposable domains to {args.output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
