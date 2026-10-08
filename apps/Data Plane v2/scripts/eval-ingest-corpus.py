#!/usr/bin/env python3
"""The committed, deterministic ingest of the retrieval-evaluation corpus (CR-T14).

## Why this exists

The evaluation corpus (95 documents, 1,164 chunks: this repository's own markdown) was assembled once,
by a process that no file recorded (docs/retrieval-fusion-evidence-2026-08-26.md, round 11). When the
organisations were lost the retrieval-quality gate could not be rebuilt, and its judgments, which key on
document ids, could not be reproduced: a gate nobody can re-run protects nothing. This script records the
process, and makes every part of it checkable:

  * the corpus is a FROZEN SNAPSHOT committed under scripts/eval-corpus/ (cut from git history, so it is the
    corpus the judgments were written against and not whatever the working tree says today), pinned file by
    file by sha256 in scripts/eval-corpus-manifest.json. Live repository paths change with every commit;
    a snapshot does not;
  * `verify` (no network, no cost) fails on a changed hash, a missing or extra file, a wrong digest;
  * `apply` ingests through documents-api with the `corpus-seeder` principal, IDEMPOTENTLY: each document
    carries a key derived from the corpus id and the document's key, so a second run creates nothing and
    makes no call that could cost anything;
  * document ids are server-assigned, so the judgments are keyed by the document's manifest key
    (carried in its metadata) and resolved to ids per organisation by eval-build-golden-set.py.

## The two organisations

org-corpus-baseline holds the corpus RAW (its processing policy is `off`, so no context is generated) and
org-corpus-contextual holds it with contextual retrieval: the A/B no longer needs a deployment toggle.
The baseline policy is set to `off` BEFORE its first document (an operator-only, audited, reason-required
call), or contexts would be generated and paid for a baseline that must not have any. `apply` refuses to
ingest into a baseline that is not raw, and into a contextual organisation whose policy is off, unless
`--set-policy` is given (it then sets the policy, and reads it back).

## What it costs, and why that is dark by default

Ingesting makes provider calls: one embedding per chunk in each organisation and, in the contextual one,
one contextualization call per chunk (about 1,164 calls, about 0.15 USD for the corpus of 2026-08-26: owner
decision OD8). `apply` therefore needs `--yes-paid-reseed OD8` (the decision, by name) AND `--max-chunks N`
(a cap, which it compares with its own estimate before the first call). `plan` and `verify` cost nothing.

## Commands

    eval-ingest-corpus.py verify                         # offline: manifest and snapshot agree
    eval-ingest-corpus.py freeze --ref <git ref>         # (re)cut the snapshot and the manifest from git history
    eval-ingest-corpus.py plan   [--org baseline|contextual|both]
    eval-ingest-corpus.py apply  [--org ...] [--set-policy] --yes-paid-reseed OD8 --max-chunks N
    eval-ingest-corpus.py status [--org ...] [--wait SECONDS]   # indexing progress

It reads DATA_PLANE_CORPUS_SEEDER_API_KEY from the environment (Control Plane publishes it in
.env.generated-secrets; eval-nightly.sh exports it), never from a file of its own and never to a log.
HTTP goes through `docker exec` into the services' own containers, the way the other eval scripts reach
them; state is read with psql in the Postgres container (SELECT only).

Exit status: 0 ok, 1 a problem with the corpus, the organisation or a call, 2 the command line or the
environment is wrong.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import re
import shlex
import shutil
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
MANIFEST_PATH = HERE / "eval-corpus-manifest.json"
SNAPSHOT_DIR = HERE / "eval-corpus"
SCHEMA = "eval-corpus-manifest-v1"

ORGS = {"baseline": "org-corpus-baseline", "contextual": "org-corpus-contextual"}
# What the script asks the corpus-seeder principal for: write and read documents, and read or change the
# processing policy of the two evaluation organisations (a test pins that the real registry grants these).
SCOPES = ["documents:read", "documents:write", "data:quality:admin"]

# The rules of documents-api's validate.go. A test parses that file and pins that these are its constants.
MAX_TITLE_BYTES = 1024
MAX_SOURCE_CHARS = 256
MAX_TYPE_CHARS = 64
MAX_CONTENT_BYTES = 5 * 1024 * 1024
MAX_IDEMPOTENCY_KEY = 256
IDEMPOTENCY_KEY_PATTERN = re.compile(r"^[A-Za-z0-9_\-\.\:]+$")
ZDR_CLASSIFICATION = "internal"

DOCUMENT_TYPE = "markdown"
# PRIVATE, not org-visible: an org-visible document is fed to graph extraction, a paid model call per
# chunk that is not part of this corpus' budget. The corpus of 2026-08 was private too (the 12 documents
# of the graph experiment were flipped on purpose, later). A service principal sees its own private
# documents, which is all the evaluation needs.
VISIBILITY = "private"

# Estimate basis, measured on the corpus of 2026-08-26: 1,164 chunks from 1,840,513 bytes of markdown, and
# the design's price of a contextualization call (gpt-4o-mini, about 600 tokens in, 60 out).
CHARS_PER_CHUNK = 1580
USD_PER_CONTEXTUAL_CALL = 0.00013
ESTIMATE_NOTE = ("an estimate, not a measurement: 1,580 characters per chunk (1,164 chunks from 1.84 MB, 2026-08-26) and "
                 "0.00013 USD per contextualization call (gpt-4o-mini, the design's price); embeddings come on top")

APPROVAL = "OD8"

# The selection that reproduces the corpus the judgments were written against: the markdown files directly
# under apps/ and one directory below it (the plane overviews, ADRs and ownership documents), and everything
# under the Data Plane's docs/. Agent-instruction files are not documentation.
DEFAULT_SELECTION = {
    "root": "apps/",
    "suffix": ".md",
    "slash_counts": [1, 2],
    "prefixes": ["apps/Data Plane v2/docs/"],
    "exclude_basenames": ["CLAUDE.md", "AGENTS.md", "ONBOARDING.md"],
}

POSTGRES_CONTAINER = "data-plane-v2-postgres-1"


def docker_argv(*rest: str) -> list[str]:
    """The docker command line. EVAL_DOCKER overrides the executable (a podman, `docker --context staging`, or the
    stand-in of the tests, which cannot put a program called docker on a Windows PATH); it is split like a shell would."""
    try:
        return [*shlex.split(os.environ.get("EVAL_DOCKER") or "docker"), *rest]
    except ValueError:
        raise IngestError("EVAL_DOCKER is not a command line (an unbalanced quote?); unset it to use docker")

SERVICES = {  # port -> (container, why)
    8010: "data-plane-v2-documents-api-1",
    8013: "data-plane-v2-data-quality-1",
}
DOCUMENTS_URL = "http://127.0.0.1:8010/v1/documents/"
POLICY_URL = "http://127.0.0.1:8013/v1/admin/orgs/{org}/processing-policy"

ORG_RE = re.compile(r"^[A-Za-z0-9_-]{1,128}$")
CORPUS_ID_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{2,63}$")


class IngestError(Exception):
    """A problem with the corpus, the git history or a service (exit 1)."""


class UsageError(Exception):
    """The command line or the environment is wrong (exit 2)."""


# ── the manifest ─────────────────────────────────────────────────────────────

def sha256_hex(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def title_of(key: str) -> str:
    """The title a document is ingested under: the file name's stem, dashes and underscores as spaces. The
    judgments of the golden set name documents by this title, so it is part of the contract."""
    stem = os.path.splitext(os.path.basename(key))[0]
    return re.sub(r"[-_]+", " ", stem).strip()


def document_entry(*, key: str, path: str, content: bytes, origin: str, source_ref: str | None = None) -> dict:
    entry = {"key": key, "title": title_of(key), "path": path, "sha256": sha256_hex(content), "bytes": len(content), "origin": origin}
    if source_ref:
        entry["source_ref"] = source_ref
    return entry


def manifest_digest(documents: list[dict]) -> str:
    """The identity of the corpus: what it holds, not where the snapshot keeps it."""
    canonical = [{"key": d["key"], "title": d["title"], "sha256": d["sha256"], "bytes": d["bytes"], "origin": d["origin"]}
                 for d in sorted(documents, key=lambda d: d["key"])]
    return sha256_hex(json.dumps(canonical, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8"))


def build_manifest(corpus_id: str, documents: list[dict], freeze: dict) -> dict:
    documents = sorted(documents, key=lambda d: d["key"])
    return {
        "schema": SCHEMA,
        "corpus_id": corpus_id,
        "description": ("The frozen corpus of the retrieval-quality evaluation: this repository's own markdown documents as they were when "
                        "the judgments of the golden set were written, plus a small synthetic supplement. Every file is pinned by sha256; "
                        "`eval-ingest-corpus.py verify` fails on any change."),
        "snapshot_dir": "eval-corpus",
        "freeze": freeze,
        "ingest": {"source": f"eval-corpus:{corpus_id}", "type": DOCUMENT_TYPE, "visibility": VISIBILITY, "zdr_classification": ZDR_CLASSIFICATION},
        "estimate_basis": {"chars_per_chunk": CHARS_PER_CHUNK, "usd_per_contextual_call": USD_PER_CONTEXTUAL_CALL},
        "totals": {"documents": len(documents), "bytes": sum(d["bytes"] for d in documents)},
        "manifest_digest": manifest_digest(documents),
        "documents": documents,
    }


def load_manifest(path: Path) -> dict:
    try:
        manifest = json.loads(Path(path).read_text(encoding="utf-8"))
    except (OSError, ValueError) as exc:
        raise IngestError(f"cannot read the manifest {path}: {exc}")
    if not isinstance(manifest, dict):
        raise IngestError(f"{path} is not a manifest")
    return manifest


def _inside(base: Path, candidate: Path) -> bool:
    try:
        candidate.resolve().relative_to(base.resolve())
        return True
    except ValueError:
        return False


def verify(manifest_path, snapshot_dir) -> list[str]:
    """Every deviation between the manifest and the snapshot ([] when they agree byte for byte)."""
    try:
        manifest = load_manifest(Path(manifest_path))
    except IngestError as exc:
        return [str(exc)]
    snapshot = Path(snapshot_dir)
    problems: list[str] = []
    if manifest.get("schema") != SCHEMA:
        problems.append(f"schema must be {SCHEMA}")
    corpus_id = manifest.get("corpus_id")
    if not isinstance(corpus_id, str) or not CORPUS_ID_RE.match(corpus_id):
        problems.append("corpus_id is missing or invalid")
    documents = manifest.get("documents")
    if not isinstance(documents, list) or not documents:
        problems.append("the manifest lists no documents")
        return problems

    seen: set[str] = set()
    listed: set[str] = set()
    total = 0
    for doc in documents:
        key = doc.get("key") if isinstance(doc, dict) else None
        if not isinstance(key, str) or not key or key.startswith("/") or ".." in key.split("/") or "\\" in key:
            problems.append(f"a document has an invalid key {key!r}")
            continue
        if key in seen:
            problems.append(f"duplicate key {key}")
            continue
        seen.add(key)
        for field in ("title", "path", "sha256", "bytes", "origin"):
            if field not in doc:
                problems.append(f"{key}: the entry has no {field}")
        path = doc.get("path", "")
        target = snapshot / path
        if not path or not _inside(snapshot, target):
            problems.append(f"{key}: the path {path!r} escapes the snapshot directory")
            continue
        listed.add(Path(path).as_posix())
        if not target.is_file():
            problems.append(f"{key}: the snapshot file {path} is missing")
            continue
        data = target.read_bytes()
        total += len(data)
        if sha256_hex(data) != doc.get("sha256"):
            problems.append(f"{key}: sha256 of {path} is {sha256_hex(data)[:16]}..., the manifest pins {str(doc.get('sha256'))[:16]}...")
        if len(data) != doc.get("bytes"):
            problems.append(f"{key}: {path} has {len(data)} bytes, the manifest says {doc.get('bytes')}")
        if not data:
            problems.append(f"{key}: the document is empty (documents-api refuses empty content)")
        elif len(data) > MAX_CONTENT_BYTES:
            problems.append(f"{key}: the document is larger than documents-api accepts ({MAX_CONTENT_BYTES} bytes)")
        else:
            try:
                data.decode("utf-8")
            except UnicodeDecodeError:
                problems.append(f"{key}: the document is not valid UTF-8 (documents-api refuses it)")
        if doc.get("title") != title_of(key) and doc.get("origin") == "repo":
            problems.append(f"{key}: the title {doc.get('title')!r} is not the one the key gives ({title_of(key)!r})")
    if snapshot.is_dir():
        for found in sorted(p for p in snapshot.rglob("*") if p.is_file()):
            relative = found.relative_to(snapshot).as_posix()
            if relative.endswith(".gitattributes") or relative == "README.md":
                continue
            if relative not in listed:
                problems.append(f"{relative} is in the snapshot but not in the manifest")
    if manifest.get("manifest_digest") != manifest_digest([d for d in documents if isinstance(d, dict) and {"key", "title", "sha256", "bytes", "origin"} <= set(d)]):
        problems.append("manifest_digest does not match the documents the manifest lists")
    totals = manifest.get("totals")
    if isinstance(totals, dict) and (totals.get("documents") != len(documents) or totals.get("bytes") != sum(d.get("bytes", 0) for d in documents)):
        problems.append("totals do not match the documents the manifest lists")
    return problems


# ── what ingesting costs ─────────────────────────────────────────────────────

def chunks_of(size_bytes: int) -> int:
    return max(1, math.ceil(size_bytes / CHARS_PER_CHUNK))


def estimate(manifest: dict, documents: list[dict] | None = None) -> dict:
    documents = manifest["documents"] if documents is None else documents
    chunks = sum(chunks_of(d["bytes"]) for d in documents)
    return {
        "documents": len(documents), "bytes": sum(d["bytes"] for d in documents), "chunks": chunks,
        "contextual_calls": chunks, "usd": round(chunks * USD_PER_CONTEXTUAL_CALL, 4), "note": ESTIMATE_NOTE,
    }


# ── what is sent ─────────────────────────────────────────────────────────────

def idempotency_key(corpus_id: str, key: str) -> str:
    """Deterministic and independent of the content: a changed document under the same key would be an
    in-place update (a re-embed, paid), which the frozen snapshot never needs and `apply` refuses as drift."""
    return f"evalcorpus:{corpus_id}:{sha256_hex(key.encode('utf-8'))[:24]}"


def document_payload(doc: dict, text: str, corpus_id: str) -> dict:
    return {
        "source": f"eval-corpus:{corpus_id}",
        "type": DOCUMENT_TYPE,
        "title": doc["title"],
        "content": text,
        "visibility": VISIBILITY,
        "zdr_classification": ZDR_CLASSIFICATION,
        "idempotency_key": idempotency_key(corpus_id, doc["key"]),
        "metadata": {"eval_corpus_id": corpus_id, "corpus_key": doc["key"], "corpus_sha256": doc["sha256"], "origin": doc["origin"]},
    }


# ── freezing: the snapshot is cut from git history ───────────────────────────

def _git(repo: Path, *args: str, binary: bool = False):
    try:
        done = subprocess.run(["git", "-c", "core.autocrlf=false", "-C", str(repo), *args], check=True, capture_output=True)
    except (OSError, subprocess.CalledProcessError) as exc:
        detail = getattr(exc, "stderr", b"") or b""
        raise IngestError(f"git {' '.join(args[:2])} failed: {detail.decode('utf-8', 'replace').strip() or exc}")
    return done.stdout if binary else done.stdout.decode("utf-8", "replace")


def _selected(path: str, selection: dict) -> bool:
    if not path.startswith(selection["root"]) or not path.endswith(selection["suffix"]):
        return False
    if os.path.basename(path) in selection["exclude_basenames"]:
        return False
    return path.count("/") in selection["slash_counts"] or any(path.startswith(p) for p in selection["prefixes"])


def freeze(*, repo_root, ref: str, snapshot_dir, manifest_path, corpus_id: str, additions=(), selection: dict | None = None) -> dict:
    """Cut the snapshot and the manifest from `ref`. Deterministic: the same ref gives the same bytes."""
    repo, snapshot, selection = Path(repo_root), Path(snapshot_dir), selection or DEFAULT_SELECTION
    if not CORPUS_ID_RE.match(corpus_id):
        raise IngestError(f"{corpus_id!r} is not a corpus id")
    sha = _git(repo, "rev-parse", "--verify", f"{ref}^{{commit}}").strip()
    when = _git(repo, "show", "-s", "--format=%cI", sha).strip()
    listing = _git(repo, "ls-tree", "-r", "-z", "--name-only", sha)
    paths = sorted(p for p in listing.split("\0") if p and _selected(p, selection))

    blobs: dict[str, tuple[bytes, str]] = {}
    for path in paths:
        blobs[path] = (_git(repo, "cat-file", "blob", f"{sha}:{path}", binary=True), sha)
    resolved_additions = []
    for addition in additions:
        add_sha = _git(repo, "rev-parse", "--verify", f"{addition['ref']}^{{commit}}").strip()
        blobs[addition["path"]] = (_git(repo, "cat-file", "blob", f"{add_sha}:{addition['path']}", binary=True), add_sha)
        resolved_additions.append({"path": addition["path"], "ref": add_sha})

    repo_dir = snapshot / "repo"
    if repo_dir.exists():
        shutil.rmtree(repo_dir)
    documents = []
    for path, (data, source_ref) in sorted(blobs.items()):
        target = repo_dir / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
        documents.append(document_entry(key=path, path=f"repo/{path}", content=data, origin="repo", source_ref=source_ref))
    synthetic = snapshot / "synthetic"
    if synthetic.is_dir():
        for file in sorted(synthetic.glob("*.md")):
            data = file.read_bytes()
            documents.append(document_entry(key=f"synthetic/{file.name}", path=f"synthetic/{file.name}", content=data, origin="synthetic"))
    manifest = build_manifest(corpus_id, documents, {
        "source_ref": sha, "source_ref_date": when, "selection": selection, "additions": resolved_additions,
        "note": ("repo documents are the git blobs of source_ref (a document with its own source_ref was taken from that commit), "
                 "byte for byte; synthetic documents are authored for this corpus and live in eval-corpus/synthetic/"),
    })
    Path(manifest_path).write_text(json.dumps(manifest, indent=1, ensure_ascii=False) + "\n", encoding="utf-8", newline="\n")
    return manifest


# ── the outside world: psql, docker exec ─────────────────────────────────────

class PsqlState:
    """What the database says about an organisation's corpus documents. SELECT only."""

    def __init__(self, run=subprocess.run, container: str = POSTGRES_CONTAINER):
        self._run, self._container = run, container

    @staticmethod
    def _org(org: str) -> str:
        if not ORG_RE.match(org or ""):
            raise IngestError(f"{org!r} is not an organisation id")
        return org

    @staticmethod
    def query_for(org: str) -> str:
        org = PsqlState._org(org)
        return ("SELECT row_to_json(t) FROM (SELECT idempotency_key AS key, document_id::text AS document_id, "
                "md5(content) AS md5, title FROM documents "
                f"WHERE org_id = '{org}' AND deleted_at IS NULL AND idempotency_key LIKE 'evalcorpus:%') t")

    def _psql(self, sql: str) -> list[str]:
        script = 'psql -U "$POSTGRES_USER" -d "$POSTGRES_DB" -v ON_ERROR_STOP=1 -t -A -c "$1"'
        try:
            done = self._run(docker_argv("exec", self._container, "sh", "-c", script, "psql", sql),
                             capture_output=True, text=True, encoding="utf-8", timeout=120)
        except (OSError, subprocess.SubprocessError):
            raise IngestError("psql could not be run in the Postgres container")
        if done.returncode != 0:
            raise IngestError(f"psql failed: {(done.stderr or '').strip()[:300]}")
        return [line for line in (done.stdout or "").splitlines() if line.strip()]

    def existing(self, org: str) -> dict:
        rows = {}
        for line in self._psql(self.query_for(org)):
            row = json.loads(line)
            rows[row["key"]] = {"document_id": row["document_id"], "md5": row["md5"], "title": row["title"]}
        return rows

    def progress(self, org: str) -> dict:
        org = self._org(org)
        counts = ("SELECT row_to_json(t) FROM (SELECT "
                  f"(SELECT count(*) FROM documents WHERE org_id = '{org}' AND deleted_at IS NULL) AS documents, "
                  f"(SELECT count(*) FROM knowledge_units WHERE org_id = '{org}') AS units, "
                  f"(SELECT count(*) FROM knowledge_units WHERE org_id = '{org}' AND embedding_status = 'done') AS embedded")
        context = (f", (SELECT coalesce(json_object_agg(s, n), '{{}}'::json) FROM (SELECT coalesce(context_status, 'none') AS s, count(*) AS n "
                   f"FROM knowledge_units WHERE org_id = '{org}' GROUP BY 1) c) AS context_status")
        try:
            lines = self._psql(counts + context + ") t")
        except IngestError as exc:
            if "context_status" not in str(exc):
                raise
            # The provenance migration (20261003130000_context_provenance) is not applied: there is no context status to
            # show yet, and the rest of the progress is still the truth.
            lines = self._psql(counts + ") t")
            row = json.loads(lines[0]) if lines else {}
            row["context_status"] = {"unavailable (migration 20261003130000_context_provenance is not applied)": 0}
            return row
        return json.loads(lines[0]) if lines else {"documents": 0, "units": 0, "embedded": 0, "context_status": {}}


class DockerCurlTransport:
    """HTTP into a service's own container, the way the other eval scripts reach it. The token travels in the
    exec environment and is never part of an error message."""

    def __init__(self, run=subprocess.run):
        self._run = run

    def request(self, method: str, url: str, token: str, body: dict | None = None, headers: dict | None = None):
        port = int(re.search(r":(\d+)/", url).group(1))
        container = SERVICES[port]
        extra = []
        for name, value in (headers or {}).items():
            if not re.match(r"^[A-Za-z0-9-]+$", name) or re.search(r"[\r\n]", str(value)):
                raise IngestError(f"{method} {url}: an invalid header")
            extra += ["-H", f"{name}: {value}"]
        # $1 is the method, $2 the URL, the rest extra curl arguments (the headers); the token is in the exec environment.
        script = ('method="$1"; url="$2"; shift 2; '
                  'curl -sS -m 180 -X "$method" "$url" -H "content-type: application/json" -H "authorization: Bearer $TOK" '
                  + ("--data-binary @- " if body is not None else "") + '"$@" -w "\\n%{http_code}"')
        payload = json.dumps(body).encode("utf-8") if body is not None else b""
        try:
            done = self._run(docker_argv("exec", "-i", "-e", f"TOK={token}", container, "sh", "-c", script, "sh", method, url, *extra),
                             input=payload, capture_output=True, timeout=240)
        except (OSError, subprocess.SubprocessError):
            raise IngestError(f"{method} {url}: the request could not be made")
        text = (done.stdout or b"").decode("utf-8", "replace")
        raw, _, status = text.rpartition("\n")
        try:
            code = int(status.strip())
        except ValueError:
            raise IngestError(f"{method} {url}: no HTTP status came back")
        try:
            parsed = json.loads(raw) if raw.strip() else {}
        except ValueError:
            parsed = {"raw": raw[:200]}
        return code, parsed


_MINT_JS = """
const body = JSON.stringify({orgId: process.env.ORG, scopes: JSON.parse(process.env.SCOPES), reason: process.env.REASON});
fetch("http://127.0.0.1:3011/api/data-plane/internal-token", {method: "POST",
  headers: {"content-type": "application/json", "x-service-id": "corpus-seeder", "x-service-api-key": process.env.CRED}, body})
 .then(async r => { if (r.status >= 300) { process.stderr.write("HTTP " + r.status); return; } const j = await r.json(); process.stdout.write(j.token); })
 .catch(e => process.stderr.write("ERR " + e.message));
"""


def make_minter(credential: str, run=subprocess.run):
    def mint(org: str, scopes, reason: str) -> str:
        if not ORG_RE.match(org):
            raise IngestError(f"{org!r} is not an organisation id")
        try:
            done = run(docker_argv("exec", "-e", f"CRED={credential}", "-e", f"ORG={org}", "-e", f"SCOPES={json.dumps(list(scopes))}",
                               "-e", f"REASON={reason}", "auth-service", "node", "-e", _MINT_JS),
                       capture_output=True, text=True, encoding="utf-8", timeout=60)
        except (OSError, subprocess.SubprocessError):
            raise IngestError("the token request could not be made (is auth-service up?)")
        token = (done.stdout or "").strip()
        if not token:
            raise IngestError(f"no token for {org}: {(done.stderr or '').strip()[:120] or 'empty answer'} (is the corpus-seeder credential current?)")
        return token
    return mint


# ── the ingest ───────────────────────────────────────────────────────────────

def _labels(choice: str) -> list[str]:
    return ["baseline", "contextual"] if choice == "both" else [choice]


def _read_policy(transport, token: str, org: str) -> str:
    status, body = transport.request("GET", POLICY_URL.format(org=org), token)
    if status != 200 or not isinstance(body, dict) or "policy" not in body:
        raise IngestError(f"{org}: the processing policy could not be read (HTTP {status}); is the CR-T11 data-quality image deployed?")
    return body["policy"].get("contextual_retrieval", "")


def _set_policy(transport, token: str, org: str, label: str, current: dict | None, out) -> None:
    carry = {k: (current or {}).get(k) for k in ("graph_extraction", "budget_mode", "context_prompt_version", "monthly_budget_usd")}
    body = {"contextual_retrieval": "off" if label == "baseline" else "inherit",
            "graph_extraction": carry["graph_extraction"] or "inherit", "budget_mode": carry["budget_mode"] or "inherit"}
    if carry["context_prompt_version"]:
        body["context_prompt_version"] = carry["context_prompt_version"]
    if carry["monthly_budget_usd"] is not None:
        body["monthly_budget_usd"] = carry["monthly_budget_usd"]
    if label == "baseline":
        body["reason"] = "evaluation baseline organization: raw chunks by design, so the A/B measures contextual retrieval (CR-T14)"
    status, answer = transport.request("PUT", POLICY_URL.format(org=org), token, body)
    if status != 200:
        raise IngestError(f"{org}: the processing policy could not be set (HTTP {status} {answer.get('code', '') if isinstance(answer, dict) else ''})")
    print(f"  {org}: contextual_retrieval set to {body['contextual_retrieval']}", file=out)


def _ensure_policy(transport, token: str, org: str, label: str, set_policy: bool, out) -> None:
    status, body = transport.request("GET", POLICY_URL.format(org=org), token)
    if status != 200 or not isinstance(body, dict) or "policy" not in body:
        raise IngestError(f"{org}: the processing policy could not be read (HTTP {status}); is the CR-T11 data-quality image deployed?")
    current = body["policy"]
    value = current.get("contextual_retrieval", "")
    wrong = (value != "off") if label == "baseline" else (value == "off")
    if not wrong:
        print(f"  {org}: contextual_retrieval is {value}, as {label} requires", file=out)
        return
    if label == "baseline":
        why = f"the baseline organization is not raw: its contextual_retrieval policy is {value!r}, not 'off', so contexts would be generated (and paid for) for it"
    else:
        why = f"the contextual organization's contextual_retrieval policy is 'off': no context would be generated for it"
    if not set_policy:
        raise IngestError(f"{org}: {why}. Run with --set-policy to set it (an operator-only, audited call), or set it by hand.")
    _set_policy(transport, token, org, label, current, out)
    if (_read_policy(transport, token, org) == "off") != (label == "baseline"):
        raise IngestError(f"{org}: the policy did not take effect when read back")


def _plan_org(manifest: dict, snapshot: Path, state, org: str) -> dict:
    existing = state.existing(org)
    create, present, drift = [], [], []
    for doc in manifest["documents"]:
        stored = existing.get(idempotency_key(manifest["corpus_id"], doc["key"]))
        text = (snapshot / doc["path"]).read_bytes().decode("utf-8")
        if stored is None:
            create.append(doc)
        elif stored["md5"] == hashlib.md5(text.encode("utf-8")).hexdigest() and stored["title"] == doc["title"]:
            present.append(doc)
        else:
            drift.append(doc)
    return {"create": create, "present": present, "drift": drift}


def _credential(environ) -> str:
    credential = (environ.get("DATA_PLANE_CORPUS_SEEDER_API_KEY") or "").strip()
    if not credential:
        raise UsageError("DATA_PLANE_CORPUS_SEEDER_API_KEY is not set: export it (Control Plane publishes it in .env.generated-secrets; "
                         "eval-nightly.sh exports it)")
    return credential


def _print_plan(org: str, label: str, plan: dict, manifest: dict, out) -> None:
    total = len(manifest["documents"])
    print(f"{org} ({label}): {len(plan['present'])} of {total} documents present, {len(plan['create'])} to create, "
          f"{len(plan['drift'])} drifted", file=out)
    for doc in plan["drift"]:
        print(f"    drift: {doc['key']} exists with other content or another title", file=out)


def cmd_plan_or_apply(args, manifest, snapshot, transport, state, mint, environ, out, apply: bool) -> int:
    labels = _labels(args.org)
    credential = _credential(environ)
    if apply:
        if args.yes_paid_reseed != APPROVAL:
            raise UsageError(f"apply makes paid provider calls (embeddings, and contextualization in the contextual organization): "
                             f"it needs --yes-paid-reseed {APPROVAL} (the owner's decision, by name) and --max-chunks N. "
                             f"`plan` shows what it would do and costs nothing.")
        if not args.max_chunks or args.max_chunks < 1:
            raise UsageError("apply needs --max-chunks N, a cap that is compared with the estimate before the first call")
    problems = verify(args.manifest, snapshot)
    if problems:
        print("the corpus does not match its manifest; nothing was sent:", file=out)
        for problem in problems:
            print(f"  - {problem}", file=out)
        return 1

    plans = {label: _plan_org(manifest, snapshot, state, ORGS[label]) for label in labels}
    work = max((estimate(manifest, plans[label]["create"])["chunks"] for label in labels), default=0)
    est_all = estimate(manifest)
    print(f"corpus {manifest['corpus_id']} ({manifest['manifest_digest'][:12]}): {est_all['documents']} documents, "
          f"{est_all['bytes']} bytes, estimated {est_all['chunks']} chunks, about {est_all['usd']:.2f} USD of contextualization "
          f"calls (the contextual organization only); {est_all['note']}", file=out)
    for label in labels:
        _print_plan(ORGS[label], label, plans[label], manifest, out)
    if apply and work > args.max_chunks:
        raise UsageError(f"the work to do is an estimated {work} chunks per organization, above --max-chunks {args.max_chunks}: "
                         f"raise the cap only if the owner approved that volume")

    tokens = {}
    for label in labels:
        org = ORGS[label]
        tokens[label] = mint(org, SCOPES, f"evaluation corpus {'ingest' if apply else 'plan'} (CR-T14)")

    if not apply:
        for label in labels:
            org = ORGS[label]
            value = _read_policy(transport, tokens[label], org)
            ok = (value == "off") if label == "baseline" else (value != "off")
            print(f"  {org}: contextual_retrieval is {value!r}" + ("" if ok else f" (the {label} organization needs "
                  f"{'off' if label == 'baseline' else 'not off'}: apply --set-policy would fix it)"), file=out)
        if not any(plans[label]["create"] for label in labels):
            print("nothing to do: every document is already present", file=out)
        return 0

    failed = 0
    for label in labels:
        org, plan = ORGS[label], plans[label]
        _ensure_policy(transport, tokens[label], org, label, args.set_policy, out)
        if plan["drift"]:
            print(f"{org}: {len(plan['drift'])} document(s) exist with other content: drift. The ingest never overwrites a document it did "
                  f"not create (an update would re-embed, and pay). Delete them (or the organization's corpus) deliberately and run again.", file=out)
            return 1
        if not plan["create"]:
            continue
        consecutive = 0
        for doc in plan["create"]:
            text = (snapshot / doc["path"]).read_bytes().decode("utf-8")
            status, body = transport.request("POST", DOCUMENTS_URL, tokens[label], document_payload(doc, text, manifest["corpus_id"]))
            if status in (200, 201):
                consecutive = 0
                continue
            failed += 1
            consecutive += 1
            print(f"  FAILED {doc['key']}: HTTP {status}", file=out)
            if consecutive >= 3:
                print(f"{org}: three failures in a row; stopping this organization (a rerun continues where it stopped)", file=out)
                break
    done = sum(len(plans[label]["create"]) for label in labels)
    if failed:
        print(f"{failed} document(s) failed; a rerun creates only what is still missing", file=out)
        return 1
    if not done:
        print("nothing to do: every document is already present, and no call was made", file=out)
    else:
        print(f"created {done} document(s). Indexing is asynchronous: `status --wait 1800` shows it finishing, and the golden "
              f"builder resolves the judgments afterwards.", file=out)
    return 0


def wait_until_indexed(state, org: str, timeout: float, sleep, clock, out) -> bool:
    deadline = clock() + timeout
    while True:
        progress = state.progress(org)
        done = progress["units"] > 0 and progress["embedded"] == progress["units"]
        print(f"  {org}: {progress['documents']} documents, {progress['embedded']}/{progress['units']} units embedded, "
              f"context {progress['context_status']}", file=out)
        if done:
            return True
        if clock() >= deadline:
            return False
        sleep(15)


def cmd_status(args, state, out, sleep, clock) -> int:
    ok = True
    for label in _labels(args.org):
        org = ORGS[label]
        if args.wait:
            ok = wait_until_indexed(state, org, args.wait, sleep, clock, out) and ok
        else:
            progress = state.progress(org)
            print(f"  {org}: {progress['documents']} documents, {progress['embedded']}/{progress['units']} units embedded, "
                  f"context {progress['context_status']}", file=out)
    return 0 if ok else 1


# ── command line ─────────────────────────────────────────────────────────────

class _Parser(argparse.ArgumentParser):
    def error(self, message):
        raise UsageError(message)


def _parser():
    parser = _Parser(prog="eval-ingest-corpus.py", description="The committed ingest of the retrieval-evaluation corpus.")
    parser.add_argument("--manifest", default=str(MANIFEST_PATH))
    parser.add_argument("--snapshot", default=str(SNAPSHOT_DIR))
    sub = parser.add_subparsers(dest="command", required=True, parser_class=_Parser)
    sub.add_parser("verify", help="offline: the manifest and the snapshot agree")
    freeze_p = sub.add_parser("freeze", help="cut the snapshot and the manifest from a git ref")
    freeze_p.add_argument("--ref", required=True)
    freeze_p.add_argument("--repo-root", default=str(HERE.parents[2]))
    freeze_p.add_argument("--corpus-id", default=None, help="default: the manifest's own, so a refreeze keeps the identity")
    freeze_p.add_argument("--add", action="append", default=[], metavar="PATH@REF", help="a document that is not in --ref but in another commit")
    for name in ("plan", "apply", "status"):
        p = sub.add_parser(name)
        p.add_argument("--org", choices=["baseline", "contextual", "both"], default="both")
        if name == "apply":
            p.add_argument("--set-policy", action="store_true")
            p.add_argument("--yes-paid-reseed", default="")
            p.add_argument("--max-chunks", type=int, default=0)
        if name == "status":
            p.add_argument("--wait", type=float, default=0)
    return parser


def main(argv=None, *, transport=None, state=None, mint=None, environ=None, sleep=time.sleep, clock=time.monotonic, stdout=None) -> int:
    out = stdout or sys.stdout
    try:
        return _main(argv, transport, state, mint, os.environ if environ is None else environ, sleep, clock, out)
    except UsageError as exc:
        print(f"eval-ingest-corpus.py: {exc}", file=sys.stderr)
        return 2
    except IngestError as exc:
        print(f"eval-ingest-corpus.py: {exc}", file=sys.stderr)
        return 1


def _main(argv, transport, state, mint, environ, sleep, clock, out) -> int:
    args = _parser().parse_args(sys.argv[1:] if argv is None else argv)
    snapshot = Path(args.snapshot)
    if args.command == "verify":
        problems = verify(args.manifest, snapshot)
        if problems:
            print("the corpus does not match its manifest:", file=out)
            for problem in problems:
                print(f"  - {problem}", file=out)
            return 1
        manifest = load_manifest(Path(args.manifest))
        print(f"corpus {manifest['corpus_id']}: {manifest['totals']['documents']} documents, {manifest['totals']['bytes']} bytes, "
              f"digest {manifest['manifest_digest'][:12]}: every file matches its pin", file=out)
        return 0
    if args.command == "freeze":
        corpus_id = args.corpus_id
        if corpus_id is None:
            try:
                corpus_id = load_manifest(Path(args.manifest))["corpus_id"]
            except (IngestError, KeyError):
                raise UsageError("--corpus-id is required when there is no manifest to keep the identity of")
        additions = []
        for item in args.add:
            path, _, ref = item.rpartition("@")
            if not path or not ref:
                raise UsageError("--add takes PATH@REF")
            additions.append({"path": path, "ref": ref})
        manifest = freeze(repo_root=args.repo_root, ref=args.ref, snapshot_dir=snapshot, manifest_path=Path(args.manifest),
                          corpus_id=corpus_id, additions=additions)
        print(f"froze {manifest['totals']['documents']} documents ({manifest['totals']['bytes']} bytes) from {manifest['freeze']['source_ref'][:12]}; "
              f"digest {manifest['manifest_digest'][:12]}", file=out)
        return 0

    manifest = load_manifest(Path(args.manifest))
    if args.command == "status":
        return cmd_status(args, state or PsqlState(), out, sleep, clock)
    if transport is None:
        transport = DockerCurlTransport()
    if state is None:
        state = PsqlState()
    if mint is None:
        mint = make_minter(_credential(environ))
    return cmd_plan_or_apply(args, manifest, snapshot, transport, state, mint, environ, out, apply=args.command == "apply")


if __name__ == "__main__":
    sys.exit(main())
