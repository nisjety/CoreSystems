#!/usr/bin/env python3
"""Put the Dataform project in the GCP repository `Visma`, compile it there and run it.

  push      write the project files that changed into the `Dev` workspace (--prune also removes the
            workspace's project files that are gone here)
  compile   compile the workspace in GCP and print the errors
  run       run the compiled workspace as the repository's service account and wait for it
  commit    commit the project files that differ from the repository's `main` branch (what a release
            config builds), and delete the project files that are gone here
  all       push, compile, run

A listing or a call that fails stops the command with the error. Nothing is treated as empty because
a request failed.

Only `definitions/`, `includes/` and `workflow_settings.yaml` are sent; the tools and the
contract files stay in the git repository. A standalone Dataform repository (no git remote) has no
`push` of its own: a workspace is a draft, and `main` only moves with `commit` (see the memory note
on the Datawarehouse repository). Uses the gcloud login of whoever runs it.

Usage: dataform/tools/deploy.py {push,compile,run,commit,all} [--tags bronze] [--actions a,b] [--prune]
"""

from __future__ import annotations

import argparse
import base64
import json
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PROJECT, LOCATION, REPOSITORY, WORKSPACE = "aquatiq-datavarehus", "europe-north1", "Visma", "Dev"
API = "https://dataform.googleapis.com/v1beta1/"
REPO_NAME = f"projects/{PROJECT}/locations/{LOCATION}/repositories/{REPOSITORY}"
WORKSPACE_RESOURCE = f"{REPO_NAME}/workspaces/{WORKSPACE}"  # what request bodies name
REPO = API + REPO_NAME  # what URLs are built from
WORKSPACE_NAME = API + WORKSPACE_RESOURCE
SENT = ("definitions", "includes")
SENT_FILES = ("workflow_settings.yaml",)
SENT_PREFIXES = tuple(f"{folder}/" for folder in SENT)
SUFFIXES = {".sqlx", ".js", ".json", ".yaml", ".md"}


class ApiError(RuntimeError):
    """A Dataform API call that failed. The status tells a missing path from a broken call."""

    def __init__(self, status: int, method: str, path: str, detail: str) -> None:
        super().__init__(f"{method} {path}: HTTP {status} {detail}")
        self.status = status


def token() -> str:
    return subprocess.run(
        ["gcloud", "auth", "print-access-token"], capture_output=True, text=True, check=True
    ).stdout.strip()


def api(method: str, url: str, body: dict | None = None) -> dict:
    request = urllib.request.Request(
        url,
        data=json.dumps(body).encode() if body is not None else None,
        method=method,
        headers={"Authorization": f"Bearer {token()}", "Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(request, timeout=300) as response:
            return json.loads(response.read() or b"{}")
    except urllib.error.HTTPError as error:
        raise ApiError(
            error.code, method, url.split("/v1beta1/")[-1], error.read().decode()[:600]
        ) from None


def is_project_file(path: str) -> bool:
    """The files the project owns: the sources in the sent folders, and the sent files at the root."""
    if path in SENT_FILES:
        return True
    name = path.rsplit("/", 1)[-1]
    return (
        path.startswith(SENT_PREFIXES)
        and Path(name).suffix in SUFFIXES
        and not name.startswith(".")
    )


def local_files() -> dict[str, bytes]:
    files: dict[str, bytes] = {}
    for folder in SENT:
        for path in sorted((ROOT / folder).rglob("*")):
            relative = str(path.relative_to(ROOT))
            if path.is_file() and is_project_file(relative):
                files[relative] = path.read_bytes()
    for name in SENT_FILES:
        files[name] = (ROOT / name).read_bytes()
    return files


def directory_entries(listing: str, folder: str) -> list[dict]:
    """Every entry of one folder, all pages. A failed call raises: it is never an empty folder."""
    entries: list[dict] = []
    page_token = ""
    while True:
        query = {
            "path": folder,
            "pageSize": 1000,
            **({"pageToken": page_token} if page_token else {}),
        }
        data = api("GET", f"{listing}:queryDirectoryContents?{urllib.parse.urlencode(query)}")
        entries.extend(data.get("directoryEntries", []))
        page_token = data.get("nextPageToken", "")
        if not page_token:
            return entries


def entry_path(folder: str, name: str) -> str:
    """The full path of an entry of `folder`.

    The workspace listing names an entry by its full path (`definitions/gold`), the repository's main
    by its name in the folder (`gold`). A name never contains a slash, so a name with one is already a
    full path, and anything else is joined onto its folder. Joining a full path would ask for a path
    that does not exist (`definitions/definitions/gold`), and the 404 that follows used to be hidden.
    """
    if not folder or "/" in name:
        return name
    return f"{folder}/{name}"


def list_files(listing: str) -> set[str]:
    """The path of every file under the root of `listing` (a workspace, or the repository's main)."""
    found: set[str] = set()
    folders = [""]
    while folders:
        folder = folders.pop()
        for entry in directory_entries(listing, folder):
            if "directory" in entry:
                folders.append(entry_path(folder, entry["directory"]))
            else:
                found.add(entry_path(folder, entry["file"]))
    return found


def main_files() -> dict[str, bytes]:
    """The content of every project file on the repository's main branch (its default branch).

    A repository with no main yet answers 404 for its root. That is printed and treated as empty;
    any other failure stops the commit.
    """
    try:
        api(
            "GET",
            f"{REPO}:queryDirectoryContents?{urllib.parse.urlencode({'path': '', 'pageSize': 1})}",
        )
    except ApiError as error:
        if error.status != 404:
            raise
        print(f"main: {error}; treating the repository as empty")
        return {}
    files: dict[str, bytes] = {}
    for path in sorted(p for p in list_files(REPO) if is_project_file(p)):
        body = api("GET", f"{REPO}:readFile?{urllib.parse.urlencode({'path': path})}")
        files[path] = base64.b64decode(body.get("contents", ""))
    return files


def plan_commit(local: dict[str, bytes], main: dict[str, bytes]) -> dict[str, dict]:
    """The file operations that make main match the project.

    A project file is written when its content differs from main. A project file that main has and
    the project does not is deleted. Files outside the project (tools, contract, docs) are never touched.
    """
    operations: dict[str, dict] = {}
    for path, content in local.items():
        if main.get(path) != content:
            operations[path] = {"writeFile": {"contents": base64.b64encode(content).decode()}}
    for path in sorted(set(main) - set(local)):
        if is_project_file(path):
            operations[path] = {"deleteFile": {}}
    return operations


def push(prune: bool) -> None:
    local = local_files()
    remote = list_files(WORKSPACE_NAME)
    sent = unchanged = 0
    for path, content in local.items():
        if path in remote:
            current = api(
                "GET", f"{WORKSPACE_NAME}:readFile?{urllib.parse.urlencode({'path': path})}"
            )
            if (
                base64.b64decode(current.get("fileContents", current.get("contents", "")))
                == content
            ):
                unchanged += 1
                continue
        api(
            "POST",
            f"{WORKSPACE_NAME}:writeFile",
            {"path": path, "contents": base64.b64encode(content).decode()},
        )
        sent += 1
    removed = 0
    if prune:
        for path in sorted(remote - set(local)):
            if is_project_file(path):
                api("POST", f"{WORKSPACE_NAME}:removeFile", {"path": path})
                removed += 1
    print(f"workspace {WORKSPACE}: {sent} written, {unchanged} unchanged, {removed} removed")


def compile_workspace() -> str:
    result = api("POST", f"{REPO}/compilationResults", {"workspace": WORKSPACE_RESOURCE})
    errors = result.get("compilationErrors", [])
    print(f"compiled {result['name'].rsplit('/', 1)[-1]}: {len(errors)} errors")
    for error in errors[:20]:
        print(f"  {error.get('path')}: {error.get('message', '')[:200]}")
    if errors:
        raise SystemExit(1)
    return result["name"]


def included_target(action: str) -> dict:
    """`schema.name` names the target's dataset too; a plain name keeps the old form (no schema).

    The API rejects a target without its schema when the action lives in a named dataset
    (FAILED_PRECONDITION), so the schema-qualified form is the one to use for assertions.
    """
    schema, dot, name = action.partition(".")
    if not dot:
        return {"database": PROJECT, "name": action}
    if not schema or not name or "." in name:
        raise SystemExit(f"--actions takes name or schema.name, got {action!r}")
    return {"database": PROJECT, "schema": schema, "name": name}


def run(
    compilation: str, tags: list[str], actions: list[str], with_dependents: bool = False
) -> None:
    config: dict = {}
    if tags:
        config["includedTags"] = tags
    if actions:
        config["includedTargets"] = [included_target(a) for a in actions]
    if with_dependents:
        config["transitiveDependentsIncluded"] = True  # also rebuild everything downstream
    invocation = api(
        "POST",
        f"{REPO}/workflowInvocations",
        {"compilationResult": compilation, "invocationConfig": config},
    )
    name = invocation["name"]
    print(f"invocation {name.rsplit('/', 1)[-1]} started")
    while True:
        time.sleep(10)
        state = api("GET", API + name)
        if state.get("state") not in ("RUNNING", "CANCELING"):
            break
    listing = api("GET", f"{API}{name}:query?pageSize=1000")
    counts: dict[str, int] = {}
    failed = []
    for action in listing.get("workflowInvocationActions", []):
        counts[action.get("state", "?")] = counts.get(action.get("state", "?"), 0) + 1
        if action.get("state") not in ("SUCCEEDED", "SKIPPED", "DISABLED"):
            target = action.get("target", {})
            failed.append(
                (target.get("schema"), target.get("name"), action.get("failureReason", "")[:240])
            )
    print(f"{state.get('state')}: {counts}")
    for schema, name_, reason in failed[:25]:
        print(f"  {schema}.{name_}: {reason}")
    if state.get("state") != "SUCCEEDED":
        raise SystemExit(1)


def commit(message: str) -> None:
    operations = plan_commit(local_files(), main_files())
    if not operations:
        print("main is already the same as the project")
        return
    deleted = sorted(path for path, operation in operations.items() if "deleteFile" in operation)
    print(f"main: {len(operations) - len(deleted)} files to write, {len(deleted)} to delete")
    for path in deleted:
        print(f"  delete {path}")
    who = subprocess.run(
        ["gcloud", "config", "get-value", "account"], capture_output=True, text=True
    ).stdout.strip()
    result = api(
        "POST",
        f"{REPO}:commit",
        {
            "commitMetadata": {
                "author": {"name": who.split("@")[0], "emailAddress": who},
                "commitMessage": message,
            },
            "fileOperations": operations,
        },
    )
    print(f"main: {len(operations)} files in commit {result.get('commitSha', '?')[:10]}")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("command", choices=["push", "compile", "run", "commit", "all"])
    ap.add_argument("--tags", default="", help="run only these tags, comma-separated")
    ap.add_argument(
        "--actions",
        default="",
        help="run only these actions, comma-separated: name or schema.name (use schema.name for assertions)",
    )
    ap.add_argument(
        "--prune", action="store_true", help="push: remove workspace files that are gone here"
    )
    ap.add_argument(
        "--with-dependents",
        action="store_true",
        help="run: also run every action downstream of the selected ones",
    )
    ap.add_argument("--message", default="Update the Visma Dataform project")
    args = ap.parse_args()
    tags = [t for t in args.tags.split(",") if t]
    actions = [a for a in args.actions.split(",") if a]
    try:
        if args.command in ("push", "all"):
            push(args.prune)
        if args.command in ("compile", "run", "all"):
            compilation = compile_workspace()
            if args.command in ("run", "all"):
                run(compilation, tags, actions, args.with_dependents)
        if args.command == "commit":
            commit(args.message)
    except ApiError as error:
        print(f"error: {error}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
