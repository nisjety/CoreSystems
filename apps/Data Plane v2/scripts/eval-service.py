#!/usr/bin/env python3
"""The retrieval-quality harness's client of data-quality-go (CR-T14).

The gate (scripts/eval-gate.py) judges evidence that only the running stack can give it. This script fetches
that evidence, writes it where the gate looks, and sends the gate's records back to be stored:

    eval-service.py coverage <work-dir>        GET /v1/quality/context-coverage of both evaluation organisations
                                               -> <work-dir>/coverage-<baseline|contextual>.json
    eval-service.py gates    <work-dir>        GET /v1/quality/gates of both (a 412 with a report is evidence, not an error)
                                               -> <work-dir>/gates-<baseline|contextual>.json
    eval-service.py record   <records-dir>     POST every record-*.json (written by `eval-gate.py --records-out`) to
                                               POST /v1/evals/results of the organisation it belongs to
    eval-service.py state    <work-dir> <variant> [--prompt V] [--model M] [--rerank-input-context 0|1]
                                               -> <work-dir>/state-<variant>.json: what the contextual organisation
                                               IS (policy, the provenance of its units, the retrieval engine's rerank-input
                                               flag) next to what the variant DECLARES it to be; the promotion gate judges
                                               whether the two agree

A token is minted per organisation for the corpus-seeder principal, for the one scope each call needs
(`data:quality:admin`), with DATA_PLANE_CORPUS_SEEDER_API_KEY from the environment (eval-nightly.sh exports it;
it is never logged and never part of an error message). HTTP goes through `docker exec` into the data-quality
container, the way the other eval scripts reach it; the units' provenance is read with psql (SELECT only).

Evidence that could not be read is NEVER written, and a file left by an earlier run is removed first: a missing
file fails the gate, a stale one would pass it.

Exit status: 0 ok, 1 a call or a file was refused, 2 the command line or the environment is wrong.
"""
from __future__ import annotations

import argparse
import importlib.util
import json
import os
import re
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
QUALITY = "http://127.0.0.1:8013"
RETRIEVAL_CONTAINER = "data-plane-v2-retrieval-engine-1"
VARIANT_RE = re.compile(r"^[a-z0-9][a-z0-9._-]{0,48}$")
SCOPE = ["data:quality:admin"]
KNOWN_NOT_BUILT = {"breadcrumb": "the deterministic title-and-heading breadcrumb is deferred (CR-T18, owner decision OD-BREADCRUMB): there is nothing to measure yet"}


def _sibling(filename: str):
    path = HERE / filename
    spec = importlib.util.spec_from_file_location("dpv2_" + filename.replace("-", "_").removesuffix(".py"), path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class UsageError(Exception):
    """The command line or the environment is wrong (exit 2)."""


class ServiceError(Exception):
    """A call or a file was refused (exit 1)."""


class LiveState:
    """What only the running stack knows: the provenance of the units, and the rerank-input flag of the retrieval engine."""

    def __init__(self, ingest, run=subprocess.run):
        self._ingest, self._run = ingest, run
        self._psql = ingest.PsqlState(run=run)

    def provenance(self, org: str):
        org = self._psql._org(org)
        rows = self._psql._psql(
            "SELECT row_to_json(t) FROM (SELECT context_prompt_version AS prompt_version, context_model AS model, "
            "coalesce(context_status, 'none') AS context_status, count(*) AS units FROM knowledge_units "
            f"WHERE org_id = '{org}' GROUP BY 1, 2, 3 ORDER BY 1, 2, 3) t")
        return [json.loads(line) for line in rows]

    def retrieval_flag(self):
        try:
            done = self._run(self._ingest.docker_argv("exec", RETRIEVAL_CONTAINER, "sh", "-c", 'printf %s "${RERANK_INPUT_INCLUDE_CONTEXT:-0}"'),
                             capture_output=True, text=True, timeout=30)
        except (OSError, subprocess.SubprocessError):
            return None
        value = (done.stdout or "").strip()
        return value if done.returncode == 0 and value in ("0", "1") else None


def _credential(environ) -> str:
    credential = (environ.get("DATA_PLANE_CORPUS_SEEDER_API_KEY") or "").strip()
    if not credential:
        raise UsageError("DATA_PLANE_CORPUS_SEEDER_API_KEY is not set: export it (Control Plane publishes it in .env.generated-secrets; "
                         "eval-nightly.sh exports it)")
    return credential


def _remove(path: Path) -> None:
    try:
        path.unlink()
    except FileNotFoundError:
        pass


def _write_json(path: Path, body) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(body, indent=2, sort_keys=True) + "\n", encoding="utf-8", newline="\n")


class Client:
    def __init__(self, ingest, transport, mint, out):
        self.orgs, self._transport, self._mint, self._out = ingest.ORGS, transport, mint, out
        self._tokens: dict[str, str] = {}

    def token(self, org: str) -> str:
        if org not in self._tokens:
            self._tokens[org] = self._mint(org, SCOPE, "retrieval-quality harness (CR-T14)")
        return self._tokens[org]

    def get(self, org: str, path: str):
        return self._transport.request("GET", QUALITY + path, self.token(org))


def cmd_evidence(client, work: Path, what: str) -> int:
    path, prefix, accept = {
        "coverage": ("/v1/quality/context-coverage", "coverage", (200,)),
        "gates": ("/v1/quality/gates", "gates", (200, 412)),
    }[what]
    failed = 0
    for label, org in client.orgs.items():
        target = work / f"{prefix}-{label}.json"
        _remove(target)
        status, body = client.get(org, path)
        usable = status in accept and isinstance(body, dict) and (what != "gates" or isinstance(body.get("gates"), list))
        if not usable:
            failed += 1
            print(f"  {org}: GET {path} answered {status}; nothing was written", file=client._out)
            continue
        _write_json(target, body)
        print(f"  {org}: {what} written to {target.name}" + (f" (HTTP {status}: at least one gate did not pass)" if status == 412 else ""), file=client._out)
    return 1 if failed else 0


def cmd_record(client, gate, directory: Path) -> int:
    files = sorted(directory.glob("record-*.json")) if directory.is_dir() else []
    if not files:
        print(f"no records to send in {directory}: run eval-gate.py with --records-out first", file=client._out)
        return 1
    recorded = replays = failed = 0
    for file in files:
        try:
            record = json.loads(file.read_text(encoding="utf-8"))
            org, key, body = record["org_id"], record["idempotency_key"], record["body"]
        except (OSError, ValueError, KeyError, TypeError):
            failed += 1
            print(f"  {file.name}: not a record file", file=client._out)
            continue
        if org not in client.orgs.values():
            failed += 1
            print(f"  {file.name}: refused, {org} is not an evaluation organisation of this harness", file=client._out)
            continue
        problems = gate.validate_record(body)
        if problems:
            failed += 1
            print(f"  {file.name}: refused before sending, the service would refuse it: {'; '.join(problems)}", file=client._out)
            continue
        status, answer = client._transport.request("POST", f"{QUALITY}/v1/evals/results", client.token(org), body,
                                                   {"Idempotency-Key": key})
        code = answer.get("code", "") if isinstance(answer, dict) else ""
        if status == 201:
            recorded += 1
            print(f"  {file.name}: recorded ({key})", file=client._out)
        elif status == 200:
            replays += 1
            print(f"  {file.name}: already recorded ({key})", file=client._out)
        elif status == 409:
            failed += 1
            print(f"  {file.name}: conflict: the key {key} is bound to a different record, and history is immutable", file=client._out)
        else:
            failed += 1
            detail = "; ".join(answer.get("problems", [])) if isinstance(answer, dict) and answer.get("problems") else code
            print(f"  {file.name}: HTTP {status} {detail}", file=client._out)
    print(f"recorded {recorded}, already recorded {replays}, failed {failed}", file=client._out)
    return 1 if failed else 0


def cmd_state(client, state, work: Path, args) -> int:
    contextual = client.orgs["contextual"]
    status, body = client.get(contextual, f"/v1/admin/orgs/{contextual}/processing-policy")
    if status != 200 or not isinstance(body, dict) or "policy" not in body:
        raise ServiceError(f"{contextual}: the processing policy could not be read (HTTP {status})")
    policy = body["policy"]
    flag = state.retrieval_flag()
    declared_flag = None if args.rerank_input_context is None else args.rerank_input_context == "1"
    document = {
        "schema": "eval-state-v1",
        "variant": args.variant,
        "declared": {"prompt_version": args.prompt, "model": args.model, "rerank_input_include_context": declared_flag},
        "observed": {
            "policy": {"contextual_retrieval": policy.get("contextual_retrieval"), "context_prompt_version": policy.get("context_prompt_version")},
            "provenance": state.provenance(contextual),
            "retrieval": {"rerank_input_include_context": flag},
        },
    }
    target = work / f"state-{args.variant}.json"
    _write_json(target, document)
    print(f"  state of {args.variant} written to {target.name}: policy {document['observed']['policy']['contextual_retrieval']}, "
          f"{sum(p['units'] for p in document['observed']['provenance'])} units, rerank-input flag {flag!r}", file=client._out)
    return 0


class _Parser(argparse.ArgumentParser):
    def error(self, message):
        raise UsageError(message)


def _parser():
    parser = _Parser(prog="eval-service.py", description="The harness's client of data-quality-go.")
    sub = parser.add_subparsers(dest="command", required=True, parser_class=_Parser)
    for name in ("coverage", "gates"):
        sub.add_parser(name).add_argument("work_dir")
    sub.add_parser("record").add_argument("records_dir")
    state = sub.add_parser("state")
    state.add_argument("work_dir")
    state.add_argument("variant")
    state.add_argument("--prompt", default=None)
    state.add_argument("--model", default=None)
    state.add_argument("--rerank-input-context", choices=["0", "1"], default=None)
    return parser


def main(argv=None, *, transport=None, mint=None, state=None, environ=None, stdout=None) -> int:
    out = stdout or sys.stdout
    if hasattr(out, "reconfigure"):
        out.reconfigure(encoding="utf-8", errors="replace")
    try:
        return _main(argv, transport, mint, state, os.environ if environ is None else environ, out)
    except UsageError as exc:
        print(f"eval-service.py: {exc}", file=sys.stderr)
        return 2
    except ServiceError as exc:
        print(f"eval-service.py: {exc}", file=sys.stderr)
        return 1
    except Exception as exc:  # the ingest module's errors carry no secrets by construction
        if type(exc).__name__ == "IngestError":
            print(f"eval-service.py: {exc}", file=sys.stderr)
            return 1
        raise


def _main(argv, transport, mint, state, environ, out) -> int:
    args = _parser().parse_args(sys.argv[1:] if argv is None else argv)
    ingest = _sibling("eval-ingest-corpus.py")
    gate = _sibling("eval-gate.py")
    if args.command == "state":
        if not VARIANT_RE.match(args.variant):
            raise UsageError(f"{args.variant!r} is not a variant name (lower case letters, digits, . _ -)")
        if args.variant in KNOWN_NOT_BUILT:
            raise UsageError(f"{args.variant}: {KNOWN_NOT_BUILT[args.variant]}")
    credential = _credential(environ)
    transport = transport or ingest.DockerCurlTransport()
    mint = mint or ingest.make_minter(credential)
    client = Client(ingest, transport, mint, out)
    if args.command in ("coverage", "gates"):
        return cmd_evidence(client, Path(args.work_dir), args.command)
    if args.command == "record":
        return cmd_record(client, gate, Path(args.records_dir))
    return cmd_state(client, state or LiveState(ingest), Path(args.work_dir), args)


if __name__ == "__main__":
    sys.exit(main())
