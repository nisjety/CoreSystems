#!/usr/bin/env python3
"""Compile and execute the isolated Xero Dataform workspace in Google Cloud."""

import argparse
import json
import time
from pathlib import Path
from urllib.error import HTTPError
from urllib.request import Request, urlopen

from sync_to_gcp import gcloud_value


PROJECT = "aquatiq-datavarehus"
LOCATION = "europe-north1"
REPOSITORY = "Xero"
WORKSPACE = f"projects/{PROJECT}/locations/{LOCATION}/repositories/{REPOSITORY}/workspaces/Dev"
REPOSITORY_RESOURCE = WORKSPACE.rsplit("/workspaces/", 1)[0]
API_URL = "https://dataform.googleapis.com/v1/"
TERMINAL_STATES = {"SUCCEEDED", "FAILED", "CANCELLED"}


def request(token: str, method: str, resource: str, payload: dict | None = None) -> dict:
    headers = {"Authorization": f"Bearer {token}"}
    body = None
    if payload is not None:
        headers["Content-Type"] = "application/json"
        body = json.dumps(payload).encode()
    request = Request(API_URL + resource, data=body, headers=headers, method=method)
    with urlopen(request) as response:
        raw = response.read()
    return json.loads(raw) if raw else {}


def parse_target(value: str) -> dict[str, str]:
    parts = value.split(".")
    if len(parts) != 3 or any(not part for part in parts):
        raise ValueError("Dataform targets must use project.dataset.name")
    database, schema, name = parts
    if database != PROJECT:
        raise ValueError(f"Target must be in {PROJECT}")
    return {"database": database, "schema": schema, "name": name}


def create_compilation_result(token: str) -> str:
    result = request(
        token,
        "POST",
        f"{REPOSITORY_RESOURCE}/compilationResults",
        {"workspace": WORKSPACE},
    )
    return result["name"]


def create_workflow_invocation(
    token: str,
    compilation_result: str,
    targets: list[dict[str, str]],
    include_dependencies: bool,
    full_refresh: bool,
) -> str:
    invocation_config: dict = {
        "fullyRefreshIncrementalTablesEnabled": full_refresh,
    }
    if targets:
        invocation_config["includedTargets"] = targets
        invocation_config["transitiveDependenciesIncluded"] = include_dependencies
    result = request(
        token,
        "POST",
        f"{REPOSITORY_RESOURCE}/workflowInvocations",
        {
            "compilationResult": compilation_result,
            "invocationConfig": invocation_config,
        },
    )
    return result["name"]


def wait_for_workflow(token: str, workflow: str, poll_seconds: int, timeout_seconds: int) -> str:
    deadline = time.monotonic() + timeout_seconds
    while True:
        status = request(token, "GET", workflow)
        state = status.get("state", "UNKNOWN")
        if state in TERMINAL_STATES:
            return state
        if time.monotonic() >= deadline:
            raise TimeoutError(f"Workflow did not finish within {timeout_seconds} seconds: {workflow}")
        time.sleep(poll_seconds)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", action="append", default=[], help="Target: project.dataset.name")
    parser.add_argument("--include-dependencies", action="store_true")
    parser.add_argument("--full", action="store_true", help="Perform a full incremental refresh")
    parser.add_argument("--wait", action="store_true")
    parser.add_argument("--poll-seconds", type=int, default=10)
    parser.add_argument("--timeout-seconds", type=int, default=900)
    args = parser.parse_args()

    if args.poll_seconds < 1 or args.timeout_seconds < 1:
        parser.error("poll and timeout values must be positive")
    try:
        targets = [parse_target(target) for target in args.target]
    except ValueError as error:
        parser.error(str(error))

    token = gcloud_value("auth", "print-access-token")
    compilation_result = create_compilation_result(token)
    workflow = create_workflow_invocation(
        token,
        compilation_result,
        targets,
        args.include_dependencies,
        args.full,
    )
    print(workflow)
    if not args.wait:
        return 0
    try:
        state = wait_for_workflow(token, workflow, args.poll_seconds, args.timeout_seconds)
    except (HTTPError, TimeoutError) as error:
        print(str(error))
        return 1
    print(state)
    return 0 if state == "SUCCEEDED" else 1


if __name__ == "__main__":
    raise SystemExit(main())
