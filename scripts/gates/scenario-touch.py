#!/usr/bin/env python3
"""Scenario-touch gate: a prompt or tool-schema change must also touch a scenario (EP-14 S8).

The prompt and tool manifest, apps/Model Plane/rust/crates/mp-contracts/tests/fixtures/
prompt-tool-manifest.json with its pin, records a digest of every platform prompt text and
offered tool definition. When the manifest or its pin changes against the base, the same change
must also add or update a scenario: an eval case under apps/Model Plane/python/eval-lab-py/cases/
or a replay cassette under apps/Model Plane/rust/crates/mp-inference-replay/cassettes/.

A reviewed exception is the other way through. It names one manifest digest in
scripts/gates/baselines/scenario-touch-exceptions.json, with a reason and the reviewer who
accepted it. The file is reviewed under CODEOWNERS. An exception covers the digest it names
only, so a later manifest change needs its own scenario or its own exception.

The change is the set of files that differ between the merge base with the base ref and the
working tree, plus untracked files. In CI the working tree is the pull request's head, so this
is the pull request's own change and not the base branch's later commits.

Usage, from the repository root:
    python3 scripts/gates/scenario-touch.py --base origin/<branch>
Exit codes: 0 the manifest is unchanged, a scenario changed with it, or a reviewed exception
covers its digest; 1 the manifest changed with neither; 2 the gate cannot run (unknown base,
unreadable or malformed exceptions file).
"""
import argparse
import hashlib
import json
import subprocess
import sys
from pathlib import Path

MANIFEST = "apps/Model Plane/rust/crates/mp-contracts/tests/fixtures/prompt-tool-manifest.json"
PIN = MANIFEST + ".sha256"
TRIGGERS = (MANIFEST, PIN)
CASES = "apps/Model Plane/python/eval-lab-py/cases/"
CASSETTES = "apps/Model Plane/rust/crates/mp-inference-replay/cassettes/"
SCENARIO_ROOTS = (CASES, CASSETTES)
EXCEPTIONS = "scripts/gates/baselines/scenario-touch-exceptions.json"
EXCEPTIONS_SCHEMA = "scenario-touch-exceptions-v1"

OK = 0
FAIL = 1
CANNOT_RUN = 2


class GateError(Exception):
    """The gate cannot reach a decision; the exit code is 2."""


def git(repo, *args):
    result = subprocess.run(
        ["git", "-C", str(repo), *args],
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
    )
    if result.returncode != 0:
        raise GateError(f"git {' '.join(args)} failed: {result.stderr.strip()}")
    return result.stdout


def changed_paths(repo, base):
    try:
        merge_base = git(repo, "merge-base", base, "HEAD").strip()
    except GateError as error:
        raise GateError(
            f"no merge base for {base} and HEAD; in CI fetch the base with fetch-depth 0 ({error})"
        ) from error
    tracked = git(repo, "diff", "-z", "--name-only", "--no-renames", merge_base)
    untracked = git(repo, "ls-files", "-z", "--others", "--exclude-standard")
    paths = {path.replace("\\", "/") for path in (tracked + untracked).split("\0") if path}
    return sorted(paths)


def manifest_digest(repo):
    path = Path(repo) / MANIFEST
    if not path.is_file():
        return None
    return hashlib.sha256(path.read_bytes().replace(b"\r\n", b"\n")).hexdigest()


def _is_digest(value):
    return isinstance(value, str) and len(value) == 64 and all(c in "0123456789abcdef" for c in value)


def load_exceptions(repo):
    path = Path(repo) / EXCEPTIONS
    if not path.is_file():
        return []
    try:
        document = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        raise GateError(f"{EXCEPTIONS} is not readable JSON: {error}") from error
    if not isinstance(document, dict) or document.get("schema") != EXCEPTIONS_SCHEMA:
        raise GateError(f"{EXCEPTIONS} must carry schema {EXCEPTIONS_SCHEMA}")
    exceptions = document.get("exceptions")
    if not isinstance(exceptions, list):
        raise GateError(f"{EXCEPTIONS} must hold an exceptions list")
    for entry in exceptions:
        if not isinstance(entry, dict) or not _is_digest(entry.get("manifest_sha256")):
            raise GateError(f"an exception has no valid manifest_sha256: {entry!r}")
        if not str(entry.get("reason", "")).strip():
            raise GateError(f"the exception for {entry['manifest_sha256']} has no reason")
        if not str(entry.get("reviewed_by", "")).strip():
            raise GateError(f"the exception for {entry['manifest_sha256']} has no reviewed_by")
    return exceptions


def decide(changed, digest, exceptions):
    """Return (exit code, message) for a change, given the manifest digest and the exceptions."""
    triggered = [path for path in changed if path in TRIGGERS]
    if not triggered:
        return OK, "scenario-touch: the prompt and tool manifest is unchanged"
    touched = sorted(path for path in changed if path.startswith(SCENARIO_ROOTS))
    if touched:
        return OK, f"scenario-touch: the manifest changed and {len(touched)} scenario file(s) changed with it, for example {touched[0]}"
    for entry in exceptions:
        if digest is not None and entry["manifest_sha256"] == digest:
            return OK, f"scenario-touch: the manifest changed; reviewed exception for {digest} applies: {entry['reason']}"
    return FAIL, (
        "scenario-touch: the prompt and tool manifest changed without a scenario change\n"
        f"  changed: {', '.join(triggered)}\n"
        f"  required: a case under {CASES} or a cassette under {CASSETTES} in this change,\n"
        f"            or a reviewed exception for manifest digest {digest} in {EXCEPTIONS}"
    )


def run(repo, base):
    changed = changed_paths(repo, base)
    return decide(changed, manifest_digest(repo), load_exceptions(repo))


def main(argv=None):
    parser = argparse.ArgumentParser(description="Require a scenario change with a prompt or tool-schema change.")
    parser.add_argument("--base", required=True, help="the base ref the change is compared with, for example origin/main")
    parser.add_argument("--repo", default=".", help="the repository root (default: the current directory)")
    args = parser.parse_args(argv)
    try:
        code, message = run(args.repo, args.base)
    except GateError as error:
        print(f"scenario-touch: cannot run: {error}", file=sys.stderr)
        return CANNOT_RUN
    print(message)
    return code


if __name__ == "__main__":
    sys.exit(main())
