#!/usr/bin/env python3
"""Publish only the isolated Xero Dataform project to its Google-hosted workspace."""

import argparse
import base64
import json
import subprocess
from pathlib import Path
from urllib.error import HTTPError
from urllib.parse import quote
from urllib.request import Request, urlopen


ROOT = Path(__file__).resolve().parent
WORKSPACE = (
    "projects/aquatiq-datavarehus/locations/europe-north1"
    "/repositories/Xero/workspaces/Dev"
)
API_URL = f"https://dataform.googleapis.com/v1/{WORKSPACE}"


def source_files() -> list[Path]:
    return [
        ROOT / "workflow_settings.yaml",
        ROOT / "README.md",
        *sorted((ROOT / "definitions").rglob("*.js")),
        *sorted((ROOT / "definitions").rglob("*.sqlx")),
    ]


def gcloud_value(*args: str) -> str:
    result = subprocess.run(
        ["gcloud", *args], capture_output=True, text=True, check=True
    )
    return result.stdout.strip()


def request(token: str, method: str, suffix: str, payload: dict | None = None) -> dict:
    headers = {"Authorization": f"Bearer {token}"}
    body = None
    if payload is not None:
        headers["Content-Type"] = "application/json"
        body = json.dumps(payload).encode()
    req = Request(API_URL + suffix, data=body, headers=headers, method=method)
    with urlopen(req) as response:
        raw = response.read()
    return json.loads(raw) if raw else {}


def remote_contents(token: str, path: str) -> bytes | None:
    try:
        result = request(token, "GET", f":readFile?path={quote(path)}")
    except HTTPError as error:
        if error.code == 404:
            return None
        raise
    return base64.b64decode(result["fileContents"])


def ensure_directory(token: str, path: str) -> None:
    parts = Path(path).parent.parts
    for depth in range(1, len(parts) + 1):
        try:
            request(token, "POST", ":makeDirectory", {"path": "/".join(parts[:depth])})
        except HTTPError as error:
            if error.code != 409:
                raise


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apply", action="store_true", help="Upload and commit changes")
    parser.add_argument("--message", default="Sync Xero Dataform models")
    args = parser.parse_args()

    token = gcloud_value("auth", "print-access-token")
    changed = []
    for file in source_files():
        path = file.relative_to(ROOT).as_posix()
        if remote_contents(token, path) != file.read_bytes():
            changed.append((path, file.read_bytes()))
    for path, contents in changed:
        print(f"{'Uploading' if args.apply else 'Would upload'} {path}")
        if args.apply:
            ensure_directory(token, path)
            request(token, "POST", ":writeFile", {
                "path": path,
                "contents": base64.b64encode(contents).decode("ascii"),
            })
    if changed and args.apply:
        email = gcloud_value("config", "get-value", "account")
        request(token, "POST", ":commit", {
            "author": {"name": "Xero Dataform sync", "emailAddress": email},
            "commitMessage": args.message,
        })
    print(f"{len(changed)} changed file(s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
