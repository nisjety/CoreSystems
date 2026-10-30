#!/usr/bin/env python3
"""Mirror the authoritative Google-hosted Xero Dataform workspace locally."""

import argparse
import base64
from pathlib import Path
from urllib.parse import urlencode
from urllib.request import Request, urlopen

from sync_to_gcp import API_URL, gcloud_value


ROOT = Path(__file__).resolve().parent


def request(token: str, suffix: str) -> dict:
    request = Request(
        API_URL + suffix,
        headers={"Authorization": f"Bearer {token}"},
        method="GET",
    )
    with urlopen(request) as response:
        import json

        return json.loads(response.read())


def directory_entries(token: str, path: str) -> list[dict]:
    query = urlencode(
        {
            "path": path,
            "pageSize": 1000,
            "view": "DIRECTORY_CONTENTS_VIEW_BASIC",
        }
    )
    return request(token, f":queryDirectoryContents?{query}").get("directoryEntries", [])


def remote_files(token: str, path: str = "") -> list[str]:
    files: list[str] = []
    for entry in directory_entries(token, path):
        if "file" in entry:
            files.append(entry["file"])
        elif "directory" in entry:
            files.extend(remote_files(token, entry["directory"]))
    return sorted(files)


def read_file(token: str, path: str) -> bytes:
    query = urlencode({"path": path})
    result = request(token, f":readFile?{query}")
    return base64.b64decode(result["fileContents"])


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apply", action="store_true", help="Write remote workspace files locally")
    parser.add_argument(
        "--overwrite",
        action="store_true",
        help="Replace local files that differ from the remote workspace",
    )
    args = parser.parse_args()

    token = gcloud_value("auth", "print-access-token")
    changed = 0
    for relative_path in remote_files(token):
        target = ROOT / relative_path
        remote_contents = read_file(token, relative_path)
        if target.exists() and target.read_bytes() == remote_contents:
            continue
        if target.exists() and not args.overwrite:
            print(f"Conflict {relative_path}; rerun with --overwrite after reviewing it")
            changed += 1
            continue
        print(f"{'Downloading' if args.apply else 'Would download'} {relative_path}")
        changed += 1
        if args.apply:
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(remote_contents)
    print(f"{changed} changed file(s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
