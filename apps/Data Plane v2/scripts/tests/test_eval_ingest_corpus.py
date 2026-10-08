"""Tests of scripts/eval-ingest-corpus.py: the committed, deterministic ingest of the evaluation corpus (CR-T14).

Until this script existed the corpus (95 documents, 1,164 chunks) had been assembled once by a process no
file recorded, so a regression gate that needed it could not be rebuilt and its judgments, which key on
document ids, could not be reproduced. The rules proved here:

  * the manifest pins every document of a FROZEN snapshot by sha256, and the check fails on a changed hash,
    a missing file, an extra file, a wrong digest, a path that escapes the snapshot;
  * the ingest is idempotent: a second run creates nothing and makes no call that could cost anything;
  * the paid part (about 1,164 chunk calls) is dark by default: it needs an explicit approval flag AND a cap;
  * the baseline organisation must be RAW before a single document goes in, the contextual one must not be;
  * what is sent satisfies the rules of documents-api, read from its own source, for every committed document;
  * the corpus-seeder principal may do what the script asks, read from the REAL registry.
"""
from __future__ import annotations

import copy
import hashlib
import io
import json
import os
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from _scripts import PLANE, REPO, SCRIPTS, load

ingest = load("eval-ingest-corpus.py")

MANIFEST = SCRIPTS / "eval-corpus-manifest.json"
SNAPSHOT = SCRIPTS / "eval-corpus"
VALIDATE_GO = PLANE / "services" / "documents-api-go" / "internal" / "validate" / "validate.go"
REGISTRY = REPO / "apps" / "Control Plane" / "config" / "plane-service-principals.json"


def _remove_tree(path: Path) -> None:
    """rmtree that also removes what git makes read-only (its objects): on Windows `rmtree(path, True)` silently leaves
    such a directory behind, and every run of the freeze tests then left ten temporary git repositories in the temp dir."""
    def make_writable_and_retry(function, target, _error):
        os.chmod(target, stat.S_IWRITE)
        function(target)
    if sys.version_info >= (3, 12):
        shutil.rmtree(path, onexc=make_writable_and_retry)
    else:
        shutil.rmtree(path, onerror=make_writable_and_retry)


def tmp_dir(test: unittest.TestCase, prefix: str = "evalingest-") -> Path:
    path = Path(tempfile.mkdtemp(prefix=prefix))
    test.addCleanup(_remove_tree, path)
    return path


def mini_corpus(root: Path, files: dict[str, bytes], *, corpus_id: str = "dpv2-eval-corpus-test") -> Path:
    """Write a snapshot directory and a manifest for it with the script's own freeze-time helpers."""
    snap = root / "eval-corpus"
    documents = []
    for key, content in files.items():
        path = snap / "repo" / key
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)
        documents.append(ingest.document_entry(key=key, path=f"repo/{key}", content=content, origin="repo"))
    manifest = ingest.build_manifest(corpus_id, documents, freeze={"source_ref": "0" * 40})
    manifest_path = root / "manifest.json"
    manifest_path.write_text(json.dumps(manifest, indent=1), encoding="utf-8")
    return manifest_path


class TheCommittedManifest(unittest.TestCase):
    def test_it_verifies_against_the_committed_snapshot(self):
        self.assertTrue(MANIFEST.exists(), "scripts/eval-corpus-manifest.json is missing: freeze the corpus (eval-ingest-corpus.py freeze)")
        self.assertEqual(ingest.verify(MANIFEST, SNAPSHOT), [])

    def test_it_describes_a_corpus_of_the_size_the_approval_assumed(self):
        manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
        estimate = ingest.estimate(manifest)
        # OD8 priced "about 1,164 chunk calls, about 0.15 USD" for the 95-document corpus of 2026-08-26.
        self.assertTrue(90 <= estimate["documents"] <= 130, estimate)
        self.assertLessEqual(estimate["chunks"], 1500, estimate)
        self.assertGreaterEqual(estimate["chunks"], 900, estimate)

    def test_every_document_satisfies_the_rules_of_documents_api_read_from_its_own_source(self):
        rules = documents_api_rules()
        manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
        for doc in manifest["documents"]:
            content = (SNAPSHOT / doc["path"]).read_bytes()
            payload = ingest.document_payload(doc, content.decode("utf-8"), manifest["corpus_id"])
            with self.subTest(key=doc["key"]):
                self.assertTrue(payload["title"] and len(payload["title"].encode()) <= rules["MaxTitleLength"])
                self.assertLessEqual(len(payload["source"]), rules["MaxSourceLength"])
                self.assertLessEqual(len(payload["type"]), rules["MaxTypeLength"])
                self.assertTrue(payload["content"])
                self.assertLessEqual(len(content), rules["MaxContentLength"])
                self.assertLessEqual(len(payload["idempotency_key"]), rules["MaxIdempotencyKeyLen"])
                self.assertRegex(payload["idempotency_key"], rules["idempotency_pattern"])
                self.assertIn(payload["zdr_classification"], rules["valid_zdr"])
                self.assertIn(payload["visibility"], ("private", "org", "shared"))

    def test_the_keys_and_titles_are_what_the_golden_builder_resolves(self):
        manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
        keys = [d["key"] for d in manifest["documents"]]
        self.assertEqual(len(keys), len(set(keys)), "duplicate keys")
        for doc in manifest["documents"]:
            self.assertEqual(doc["title"], ingest.title_of(doc["key"]), doc["key"])

    def test_the_corpus_seeder_may_do_what_the_script_asks_of_it_in_the_real_registry(self):
        principal = json.loads(REGISTRY.read_text(encoding="utf-8"))["corpus-seeder"]
        self.assertEqual(set(principal["orgIds"]), set(ingest.ORGS.values()))
        self.assertFalse(principal["allowAnyOrg"])
        granted = set(principal["scopesByAudience"]["data-plane"])
        self.assertLessEqual(set(ingest.SCOPES), granted, "the script asks for a scope the corpus-seeder does not hold")
        self.assertEqual(principal["retentionByAudience"]["data-plane"], "persistent",
                         "the corpus is ingested under a persistent posture: documents-api refuses a ZDR token")


def documents_api_rules() -> dict:
    """The constants of documents-api's validate.go, parsed from the source: the stub of the service the
    tests use below is only as good as these, so they are not retyped here."""
    source = VALIDATE_GO.read_text(encoding="utf-8")
    rules = {name: eval(expr, {}, {}) for name, expr in re.findall(r"^\s*(Max\w+)\s*=\s*([0-9 *]+)", source, re.M)}
    pattern = re.search(r"idempotencyKeyPattern\s*=\s*regexp\.MustCompile\(`([^`]+)`\)", source)
    rules["idempotency_pattern"] = pattern.group(1)
    zdr = re.search(r"validZDR\s*=\s*map\[string\]bool\{(.*?)\n\}", source, re.S)
    rules["valid_zdr"] = set(re.findall(r'"([a-z]*)":', zdr.group(1)))
    return rules


class TheRulesOfDocumentsApiAreTheOnesTheScriptAssumes(unittest.TestCase):
    def test_the_constants_the_script_mirrors_are_the_services_own(self):
        rules = documents_api_rules()
        self.assertEqual(ingest.MAX_TITLE_BYTES, rules["MaxTitleLength"])
        self.assertEqual(ingest.MAX_SOURCE_CHARS, rules["MaxSourceLength"])
        self.assertEqual(ingest.MAX_TYPE_CHARS, rules["MaxTypeLength"])
        self.assertEqual(ingest.MAX_CONTENT_BYTES, rules["MaxContentLength"])
        self.assertEqual(ingest.MAX_IDEMPOTENCY_KEY, rules["MaxIdempotencyKeyLen"])
        self.assertEqual(ingest.IDEMPOTENCY_KEY_PATTERN.pattern, rules["idempotency_pattern"])
        self.assertTrue(ingest.ZDR_CLASSIFICATION in rules["valid_zdr"])


class VerifyFailsOnAnyDeviation(unittest.TestCase):
    def setUp(self):
        self.root = tmp_dir(self)
        self.files = {"apps/a.md": b"# a\nalpha\n", "apps/X/b.md": b"# b\nbeta\n"}
        self.manifest_path = mini_corpus(self.root, self.files)
        self.snapshot = self.root / "eval-corpus"

    def verify(self):
        return ingest.verify(self.manifest_path, self.snapshot)

    def manifest(self) -> dict:
        return json.loads(self.manifest_path.read_text(encoding="utf-8"))

    def write_manifest(self, manifest: dict) -> None:
        self.manifest_path.write_text(json.dumps(manifest, indent=1), encoding="utf-8")

    def test_a_clean_corpus_verifies(self):
        self.assertEqual(self.verify(), [])

    def test_a_changed_file_hash_fails_and_names_the_file(self):
        (self.snapshot / "repo" / "apps" / "a.md").write_bytes(b"# a\nalpha, edited\n")
        problems = self.verify()
        self.assertTrue(any("apps/a.md" in p and "sha256" in p for p in problems), problems)

    def test_a_single_flipped_byte_is_enough(self):
        path = self.snapshot / "repo" / "apps" / "X" / "b.md"
        data = bytearray(path.read_bytes())
        data[3] ^= 1
        path.write_bytes(bytes(data))
        self.assertTrue(self.verify())

    def test_a_line_ending_change_fails(self):
        path = self.snapshot / "repo" / "apps" / "a.md"
        path.write_bytes(path.read_bytes().replace(b"\n", b"\r\n"))
        self.assertTrue(any("apps/a.md" in p for p in self.verify()))

    def test_a_missing_file_fails(self):
        (self.snapshot / "repo" / "apps" / "a.md").unlink()
        self.assertTrue(any("apps/a.md" in p and "missing" in p for p in self.verify()))

    def test_a_file_that_is_not_in_the_manifest_fails(self):
        (self.snapshot / "repo" / "apps" / "extra.md").write_bytes(b"# extra\n")
        self.assertTrue(any("extra.md" in p and "not in the manifest" in p for p in self.verify()))

    def test_a_manifest_whose_digest_does_not_match_its_documents_fails(self):
        manifest = self.manifest()
        manifest["manifest_digest"] = "0" * 64
        self.write_manifest(manifest)
        self.assertTrue(any("manifest_digest" in p for p in self.verify()))

    def test_a_wrong_byte_count_fails(self):
        manifest = self.manifest()
        manifest["documents"][0]["bytes"] += 1
        manifest["manifest_digest"] = ingest.manifest_digest(manifest["documents"])
        self.write_manifest(manifest)
        self.assertTrue(any("bytes" in p for p in self.verify()))

    def test_a_path_that_escapes_the_snapshot_fails(self):
        (self.root / "outside.md").write_bytes(b"# outside\n")
        manifest = self.manifest()
        manifest["documents"][0]["path"] = "../outside.md"
        manifest["documents"][0]["sha256"] = hashlib.sha256(b"# outside\n").hexdigest()
        manifest["documents"][0]["bytes"] = len(b"# outside\n")
        manifest["manifest_digest"] = ingest.manifest_digest(manifest["documents"])
        self.write_manifest(manifest)
        problems = self.verify()
        self.assertTrue(any("escapes" in p for p in problems), problems)

    def test_duplicate_keys_fail(self):
        manifest = self.manifest()
        manifest["documents"].append(copy.deepcopy(manifest["documents"][0]))
        manifest["manifest_digest"] = ingest.manifest_digest(manifest["documents"])
        self.write_manifest(manifest)
        self.assertTrue(any("duplicate key" in p for p in self.verify()))

    def test_an_empty_or_non_utf8_document_fails(self):
        root = tmp_dir(self)
        for content in (b"", b"\xff\xfe\x00bad"):
            with self.subTest(content=content):
                manifest_path = mini_corpus(root, {"apps/c.md": content})
                self.assertTrue(ingest.verify(manifest_path, root / "eval-corpus"))

    def test_a_manifest_that_is_not_json_or_has_no_documents_fails(self):
        self.manifest_path.write_text("{ not json", encoding="utf-8")
        self.assertTrue(self.verify())
        self.write_manifest({"schema": "eval-corpus-manifest-v1", "corpus_id": "x", "documents": []})
        self.assertTrue(self.verify())


class Freeze(unittest.TestCase):
    """The snapshot is cut from git history, so that the corpus is the one the judgments were written against
    and not whatever the working tree says today."""

    def git(self, repo: Path, *args: str) -> str:
        return subprocess.run(["git", "-c", "core.autocrlf=false", "-c", "user.name=t", "-c", "user.email=t@example.invalid", "-C", str(repo), *args],
                              check=True, capture_output=True, text=True).stdout.strip()

    def setUp(self):
        self.repo = tmp_dir(self, "evalfreeze-repo-")
        self.git(self.repo, "init", "-q")
        files = {
            "apps/top.md": b"# top\nv1\n", "apps/Control Plane/CP.md": b"# cp\nv1\n",
            "apps/Data Plane v2/docs/guide.md": b"# guide\nv1\n", "apps/Data Plane v2/docs/sub/deep.md": b"# deep\nv1\n",
            "apps/Model Plane/CLAUDE.md": b"# agent instructions\n", "apps/Model Plane/ONBOARDING.md": b"# onboarding\n",
            "apps/A/B/too-deep.md": b"# too deep\n", "apps/notes.txt": b"not markdown\n", "docs/outside.md": b"# outside\n",
        }
        for name, data in files.items():
            path = self.repo / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        self.git(self.repo, "add", "-A")
        self.git(self.repo, "commit", "-q", "-m", "one")
        self.first = self.git(self.repo, "rev-parse", "HEAD")
        (self.repo / "apps" / "top.md").write_bytes(b"# top\nv2 changed later\n")
        (self.repo / "apps" / "later.md").write_bytes(b"# later\nadded in the second commit\n")
        self.git(self.repo, "add", "-A")
        self.git(self.repo, "commit", "-q", "-m", "two")
        self.second = self.git(self.repo, "rev-parse", "HEAD")
        self.out = tmp_dir(self, "evalfreeze-out-")

    def freeze(self, ref: str, **extra) -> dict:
        return ingest.freeze(repo_root=self.repo, ref=ref, snapshot_dir=self.out / "eval-corpus", manifest_path=self.out / "manifest.json",
                             corpus_id="dpv2-eval-corpus-test", **extra)

    def keys(self) -> list[str]:
        return [d["key"] for d in json.loads((self.out / "manifest.json").read_text(encoding="utf-8"))["documents"]]

    def test_it_takes_the_bytes_of_the_ref_not_of_the_working_tree(self):
        self.freeze(self.first)
        self.assertEqual((self.out / "eval-corpus" / "repo" / "apps" / "top.md").read_bytes(), b"# top\nv1\n")
        self.assertNotIn("apps/later.md", self.keys())

    def test_it_applies_the_selection_rule(self):
        self.freeze(self.first)
        self.assertEqual(sorted(self.keys()), ["apps/Control Plane/CP.md", "apps/Data Plane v2/docs/guide.md",
                                               "apps/Data Plane v2/docs/sub/deep.md", "apps/top.md"])

    def test_it_is_deterministic(self):
        self.freeze(self.first)
        first = (self.out / "manifest.json").read_bytes()
        self.freeze(self.first)
        self.assertEqual(first, (self.out / "manifest.json").read_bytes())

    def test_it_records_where_it_came_from_and_the_result_verifies(self):
        manifest = self.freeze(self.first)
        self.assertEqual(manifest["freeze"]["source_ref"], self.first)
        self.assertEqual(ingest.verify(self.out / "manifest.json", self.out / "eval-corpus"), [])

    def test_an_addition_comes_from_its_own_ref(self):
        self.freeze(self.first, additions=[{"path": "apps/later.md", "ref": self.second}])
        self.assertIn("apps/later.md", self.keys())
        self.assertEqual((self.out / "eval-corpus" / "repo" / "apps" / "later.md").read_bytes(), b"# later\nadded in the second commit\n")

    def test_refreezing_removes_what_the_new_selection_no_longer_holds(self):
        self.freeze(self.second)
        self.assertIn("apps/later.md", self.keys())
        self.freeze(self.first)
        self.assertNotIn("apps/later.md", self.keys())
        self.assertFalse((self.out / "eval-corpus" / "repo" / "apps" / "later.md").exists())

    def test_synthetic_documents_are_kept_and_listed(self):
        synthetic = self.out / "eval-corpus" / "synthetic"
        synthetic.mkdir(parents=True)
        (synthetic / "no-test.md").write_bytes("# Testdokument\nDette er en syntetisk tekst.\n".encode("utf-8"))
        manifest = self.freeze(self.first)
        entry = next(d for d in manifest["documents"] if d["key"] == "synthetic/no-test.md")
        self.assertEqual(entry["origin"], "synthetic")
        self.assertEqual(entry["title"], "no test")
        self.assertEqual(ingest.verify(self.out / "manifest.json", self.out / "eval-corpus"), [])

    def test_an_unknown_ref_is_a_clear_failure(self):
        with self.assertRaises(ingest.IngestError):
            self.freeze("does-not-exist")


class FakeServices:
    """documents-api and data-quality-go as far as the ingest relies on them, with the create semantics of
    documents-api (201 created, 200 for the same key with the same or new content) and the policy endpoints.
    It validates what it receives with the constants parsed from the real service's source."""

    def __init__(self, test: unittest.TestCase, baseline_policy: str = "inherit", contextual_policy: str = "inherit"):
        self.test = test
        self.rules = documents_api_rules()
        self.docs: dict[str, dict[str, dict]] = {org: {} for org in ingest.ORGS.values()}
        self.policy = {ingest.ORGS["baseline"]: baseline_policy, ingest.ORGS["contextual"]: contextual_policy}
        self.requests: list[tuple[str, str, str]] = []
        self.minted: list[tuple[str, tuple[str, ...]]] = []
        registry = json.loads(REGISTRY.read_text(encoding="utf-8"))["corpus-seeder"]
        self.allowed_scopes = set(registry["scopesByAudience"]["data-plane"])
        self.allowed_orgs = set(registry["orgIds"])
        self.fail_on_key: str | None = None

    # the minter: the auth-service of Control Plane, with the real registry's rules
    def mint(self, org: str, scopes, reason: str) -> str:
        self.test.assertIn(org, self.allowed_orgs, "the principal is not allowed to act for this organization")
        self.test.assertLessEqual(set(scopes), self.allowed_scopes, "a scope the principal does not hold")
        self.test.assertTrue(reason)
        self.minted.append((org, tuple(scopes)))
        return f"tok:{org}"

    def request(self, method: str, url: str, token: str, body: dict | None = None):
        org = token.split(":", 1)[1]
        self.requests.append((method, url, org))
        if "/v1/documents" in url and method == "POST":
            return self.create(org, body or {})
        if "/processing-policy" in url:
            return self.policy_call(org, method, url, body)
        return 404, {"error": "not found"}

    def create(self, org: str, body: dict):
        rules = self.rules
        for field, limit in (("title", rules["MaxTitleLength"]),):
            if not body.get(field) or len(body[field].encode()) > limit:
                return 400, {"error": f"{field} invalid"}
        for field in ("source", "type", "content"):
            if not body.get(field):
                return 400, {"error": f"{field} is required"}
        if len(body["source"]) > rules["MaxSourceLength"] or len(body["type"]) > rules["MaxTypeLength"]:
            return 400, {"error": "too long"}
        if len(body["content"].encode()) > rules["MaxContentLength"]:
            return 400, {"error": "content too large"}
        key = body.get("idempotency_key", "")
        if key and (len(key) > rules["MaxIdempotencyKeyLen"] or not re.match(rules["idempotency_pattern"], key)):
            return 400, {"error": "idempotency_key invalid"}
        if body.get("zdr_classification", "") not in rules["valid_zdr"]:
            return 400, {"error": "zdr_classification invalid"}
        if body.get("visibility", "") not in ("", "private", "org", "shared"):
            return 400, {"error": "visibility invalid"}
        if self.fail_on_key and key == self.fail_on_key:
            return 500, {"error": "failed to create document"}
        existing = self.docs[org].get(key)
        if existing:
            if existing["content"] == body["content"] and existing["title"] == body["title"]:
                return 200, {"document_id": existing["document_id"], "org_id": org}
            existing.update(content=body["content"], title=body["title"])
            return 200, {"document_id": existing["document_id"], "org_id": org}
        doc_id = f"00000000-0000-4000-8000-{len(self.docs[org]) + 1:012d}"
        self.docs[org][key] = {"document_id": doc_id, "content": body["content"], "title": body["title"], "metadata": body.get("metadata"),
                               "visibility": body.get("visibility"), "zdr_classification": body.get("zdr_classification")}
        return 201, {"document_id": doc_id, "org_id": org}

    def policy_call(self, org: str, method: str, url: str, body):
        if f"/orgs/{org}/" not in url:
            return 403, {"error": "tenant_mismatch", "code": "tenant_mismatch"}
        if method == "GET":
            return 200, {"org_id": org, "policy": {"contextual_retrieval": self.policy[org], "source": "policy"}}
        if method == "PUT":
            self.test.assertIn("contextual_retrieval", body)
            self.test.assertIn("graph_extraction", body)
            self.test.assertIn("budget_mode", body)
            if body["contextual_retrieval"] == "off":
                self.test.assertTrue(body.get("reason"), "an off policy needs a reason")
            self.policy[org] = body["contextual_retrieval"]
            return 200, {"org_id": org, "policy": {"contextual_retrieval": self.policy[org], "source": "policy"}}
        return 405, {}

    # the state reader: what psql would say about the documents of the organization
    def existing(self, org: str) -> dict:
        return {key: {"document_id": d["document_id"], "md5": hashlib.md5(d["content"].encode()).hexdigest(), "title": d["title"]}
                for key, d in self.docs[org].items()}

    def posts(self) -> int:
        return sum(1 for m, u, _ in self.requests if m == "POST" and "/v1/documents" in u)


def run_cli(test: unittest.TestCase, services: FakeServices, argv: list[str], manifest_path: Path, snapshot: Path):
    out, err = io.StringIO(), io.StringIO()
    import contextlib
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        code = ingest.main(["--manifest", str(manifest_path), "--snapshot", str(snapshot), *argv],
                           transport=services, state=services, mint=services.mint, environ={"DATA_PLANE_CORPUS_SEEDER_API_KEY": "present"},
                           sleep=lambda _s: None)
    return code, out.getvalue() + err.getvalue()


class Apply(unittest.TestCase):
    def setUp(self):
        self.root = tmp_dir(self)
        self.files = {f"apps/doc{i}.md": f"# doc {i}\nbody of document {i}\n".encode() for i in range(6)}
        self.manifest_path = mini_corpus(self.root, self.files)
        self.snapshot = self.root / "eval-corpus"

    def run_ingest(self, services, *argv):
        return run_cli(self, services, list(argv), self.manifest_path, self.snapshot)

    PAID = ["--yes-paid-reseed", "OD8", "--max-chunks", "1500"]

    def test_the_plan_reads_but_never_writes(self):
        services = FakeServices(self)
        code, out = self.run_ingest(services, "plan")
        self.assertEqual(code, 0, out)
        self.assertEqual(services.posts(), 0)
        self.assertEqual(sum(len(d) for d in services.docs.values()), 0)
        self.assertIn("6 documents", out)
        self.assertIn("estimated", out)
        self.assertIn("org-corpus-baseline", out)

    def test_apply_without_the_approval_does_nothing_and_says_why(self):
        services = FakeServices(self)
        code, out = self.run_ingest(services, "apply", "--set-policy")
        self.assertEqual(code, 2, out)
        self.assertIn("--yes-paid-reseed", out)
        self.assertEqual(services.posts(), 0)
        self.assertEqual(services.minted, [])

    def test_the_approval_must_name_the_decision_and_carry_a_cap(self):
        services = FakeServices(self)
        for argv in (["apply", "--yes-paid-reseed", "yes", "--max-chunks", "1500"],
                     ["apply", "--yes-paid-reseed", "OD8"],
                     ["apply", "--yes-paid-reseed", "OD8", "--max-chunks", "0"]):
            with self.subTest(argv=argv):
                code, out = self.run_ingest(services, *argv)
                self.assertEqual(code, 2, out)
        self.assertEqual(services.posts(), 0)

    def test_a_cap_below_the_estimate_refuses_before_any_call(self):
        services = FakeServices(self)
        code, out = self.run_ingest(services, "apply", "--set-policy", "--yes-paid-reseed", "OD8", "--max-chunks", "1")
        self.assertEqual(code, 2, out)
        self.assertIn("estimated", out)
        self.assertEqual(services.posts(), 0)
        self.assertEqual(services.minted, [])

    def test_apply_ingests_both_organizations_baseline_raw_first(self):
        services = FakeServices(self)
        code, out = self.run_ingest(services, "apply", "--set-policy", *self.PAID)
        self.assertEqual(code, 0, out)
        for org in ingest.ORGS.values():
            self.assertEqual(len(services.docs[org]), 6, org)
        self.assertEqual(services.policy[ingest.ORGS["baseline"]], "off")
        self.assertEqual(services.policy[ingest.ORGS["contextual"]], "inherit")
        # the baseline policy was set to off BEFORE its first document, or contexts would be generated (and paid) for it
        baseline_events = [(m, u) for m, u, o in services.requests if o == ingest.ORGS["baseline"]]
        first_post = next(i for i, (m, u) in enumerate(baseline_events) if m == "POST" and "/v1/documents" in u)
        self.assertTrue(any(m == "PUT" for m, _ in baseline_events[:first_post]), baseline_events)

    def test_the_second_run_creates_nothing_and_posts_nothing(self):
        services = FakeServices(self)
        self.assertEqual(self.run_ingest(services, "apply", "--set-policy", *self.PAID)[0], 0)
        posted = services.posts()
        before = copy.deepcopy(services.docs)
        code, out = self.run_ingest(services, "apply", "--set-policy", *self.PAID)
        self.assertEqual(code, 0, out)
        self.assertEqual(services.posts(), posted, "the second run made a write call")
        self.assertEqual(services.docs, before)
        self.assertIn("nothing to do", out)

    def test_a_partial_run_continues_where_it_stopped(self):
        services = FakeServices(self)
        services.fail_on_key = ingest.idempotency_key("dpv2-eval-corpus-test", "apps/doc3.md")
        code, out = self.run_ingest(services, "apply", "--set-policy", *self.PAID)
        self.assertNotEqual(code, 0, out)
        self.assertIn("apps/doc3.md", out)
        created = sum(len(d) for d in services.docs.values())
        services.fail_on_key = None
        code, out = self.run_ingest(services, "apply", "--set-policy", *self.PAID)
        self.assertEqual(code, 0, out)
        for org in ingest.ORGS.values():
            self.assertEqual(len(services.docs[org]), 6)
        self.assertGreater(sum(len(d) for d in services.docs.values()), created)

    def test_a_document_that_exists_with_other_content_is_drift_and_is_refused(self):
        services = FakeServices(self)
        self.assertEqual(self.run_ingest(services, "apply", "--set-policy", *self.PAID)[0], 0)
        key = ingest.idempotency_key("dpv2-eval-corpus-test", "apps/doc2.md")
        services.docs[ingest.ORGS["contextual"]][key]["content"] = "someone edited this"
        posted = services.posts()
        code, out = self.run_ingest(services, "apply", "--set-policy", *self.PAID)
        self.assertEqual(code, 1, out)
        self.assertIn("drift", out)
        self.assertIn("apps/doc2.md", out)
        self.assertEqual(services.posts(), posted, "the ingest must not overwrite a document it did not create")
        self.assertEqual(services.docs[ingest.ORGS["contextual"]][key]["content"], "someone edited this")

    def test_a_baseline_that_is_not_raw_is_refused_without_set_policy(self):
        services = FakeServices(self, baseline_policy="inherit")
        code, out = self.run_ingest(services, "apply", *self.PAID)
        self.assertEqual(code, 1, out)
        self.assertIn("not raw", out)
        self.assertEqual(len(services.docs[ingest.ORGS["baseline"]]), 0)
        self.assertEqual(services.policy[ingest.ORGS["baseline"]], "inherit", "the policy was changed without --set-policy")

    def test_a_contextual_organization_whose_policy_is_off_is_refused_without_set_policy(self):
        services = FakeServices(self, baseline_policy="off", contextual_policy="off")
        code, out = self.run_ingest(services, "apply", "--org", "contextual", *self.PAID)
        self.assertEqual(code, 1, out)
        self.assertIn("off", out)
        self.assertEqual(len(services.docs[ingest.ORGS["contextual"]]), 0)

    def test_set_policy_gives_the_baseline_an_off_policy_with_a_reason_and_reads_it_back(self):
        services = FakeServices(self, baseline_policy="inherit")
        code, out = self.run_ingest(services, "apply", "--org", "baseline", "--set-policy", *self.PAID)
        self.assertEqual(code, 0, out)
        self.assertEqual(services.policy[ingest.ORGS["baseline"]], "off")
        self.assertEqual(len(services.docs[ingest.ORGS["baseline"]]), 6)
        self.assertEqual(len(services.docs[ingest.ORGS["contextual"]]), 0)

    def test_the_token_asks_for_the_organization_and_scopes_it_needs_and_no_other(self):
        services = FakeServices(self)
        self.assertEqual(self.run_ingest(services, "apply", "--set-policy", *self.PAID)[0], 0)
        self.assertEqual({org for org, _ in services.minted}, set(ingest.ORGS.values()))
        for _, scopes in services.minted:
            self.assertLessEqual(set(scopes), set(ingest.SCOPES))

    def test_every_document_carries_the_key_the_golden_builder_resolves_it_by(self):
        services = FakeServices(self)
        self.assertEqual(self.run_ingest(services, "apply", "--set-policy", *self.PAID)[0], 0)
        stored = services.docs[ingest.ORGS["contextual"]]
        manifest = json.loads(self.manifest_path.read_text(encoding="utf-8"))
        for doc in manifest["documents"]:
            record = stored[ingest.idempotency_key(manifest["corpus_id"], doc["key"])]
            self.assertEqual(record["metadata"]["corpus_key"], doc["key"])
            self.assertEqual(record["metadata"]["eval_corpus_id"], manifest["corpus_id"])
            self.assertEqual(record["metadata"]["corpus_sha256"], doc["sha256"])

    def test_every_document_is_sent_private_and_internal(self):
        # An ORG-visible document is fed to graph extraction, a paid model call per chunk that is not in OD8's budget;
        # `internal` is a classification the contextual and the keyword paths accept (v_context_coverage counts it).
        services = FakeServices(self)
        self.assertEqual(self.run_ingest(services, "apply", "--set-policy", *self.PAID)[0], 0)
        for org in ingest.ORGS.values():
            self.assertEqual(len(services.docs[org]), 6)
            for key, record in services.docs[org].items():
                self.assertEqual(record["visibility"], "private", f"{org} {key}")
                self.assertEqual(record["zdr_classification"], "internal", f"{org} {key}")

    def test_a_policy_that_does_not_take_effect_stops_the_ingest_before_its_first_document(self):
        # The service answers 200 to the PUT but the policy it reads back is not the one that was asked for:
        # contexts would be generated, and paid for, for the baseline. Nothing may be sent.
        services = FakeServices(self, baseline_policy="inherit")
        honest = services.policy_call

        def ignores_the_put(org, method, url, body):
            if method == "PUT":
                return 200, {"org_id": org, "policy": {"contextual_retrieval": services.policy[org], "source": "policy"}}
            return honest(org, method, url, body)

        services.policy_call = ignores_the_put
        code, out = self.run_ingest(services, "apply", "--org", "baseline", "--set-policy", *self.PAID)
        self.assertEqual(code, 1, out)
        self.assertIn("read back", out)
        self.assertEqual(services.posts(), 0)

    def test_a_missing_credential_is_a_usage_error_and_never_echoes_anything(self):
        services = FakeServices(self)
        out, err = io.StringIO(), io.StringIO()
        import contextlib
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = ingest.main(["--manifest", str(self.manifest_path), "--snapshot", str(self.snapshot), "apply", *self.PAID],
                               transport=services, state=services, environ={}, sleep=lambda _s: None)
        self.assertEqual(code, 2)
        self.assertIn("DATA_PLANE_CORPUS_SEEDER_API_KEY", out.getvalue() + err.getvalue())

    def test_verify_runs_first_so_unpinned_bytes_are_never_sent(self):
        (self.snapshot / "repo" / "apps" / "doc1.md").write_bytes(b"tampered")
        services = FakeServices(self)
        code, out = self.run_ingest(services, "apply", "--set-policy", *self.PAID)
        self.assertEqual(code, 1, out)
        self.assertEqual(services.posts(), 0)
        self.assertEqual(services.minted, [])


class Estimate(unittest.TestCase):
    def test_it_is_a_function_of_the_bytes_and_says_what_it_assumes(self):
        manifest = {"documents": [{"bytes": 1580 * 10}, {"bytes": 1580 * 2}]}
        estimate = ingest.estimate(manifest)
        self.assertEqual(estimate["documents"], 2)
        self.assertEqual(estimate["chunks"], 12)
        self.assertEqual(estimate["contextual_calls"], 12)
        self.assertGreater(estimate["usd"], 0)
        self.assertIn("estimate", estimate["note"])


class StateReaderSql(unittest.TestCase):
    def test_organization_ids_are_validated_before_they_reach_sql(self):
        for bad in ("org'; drop table documents; --", "org with space", ""):
            with self.assertRaises(ingest.IngestError):
                ingest.PsqlState.query_for(bad)

    def test_the_query_is_read_only_and_scoped_to_the_organization_and_the_corpus(self):
        sql = ingest.PsqlState.query_for("org-corpus-baseline")
        self.assertTrue(sql.lstrip().upper().startswith("SELECT"))
        self.assertIn("org_id = 'org-corpus-baseline'", sql)
        self.assertIn("deleted_at IS NULL", sql)
        for forbidden in ("INSERT", "UPDATE", "DELETE", "DROP", "TRUNCATE", "ALTER", "GRANT"):
            self.assertIsNone(re.search(rf"{forbidden}", sql.upper()), forbidden)


class Progress(unittest.TestCase):
    """`status` before the provenance migration is applied: the counts are still the truth, and the missing column is said."""

    def state_with(self, answers):
        import types
        calls = []

        def run(argv, **kwargs):
            sql = argv[-1]
            calls.append(sql)
            answer = answers(sql)
            return types.SimpleNamespace(stdout=answer.get("stdout", ""), stderr=answer.get("stderr", ""), returncode=answer.get("rc", 0))

        return ingest.PsqlState(run=run), calls

    def test_the_full_progress_when_the_migration_is_applied(self):
        row = {"documents": 104, "units": 1234, "embedded": 1200, "context_status": {"ready": 1100, "pending": 100}}
        state, calls = self.state_with(lambda sql: {"stdout": json.dumps(row) + chr(10)})
        self.assertEqual(state.progress("org-corpus-contextual"), row)
        self.assertEqual(len(calls), 1)

    def test_without_the_migration_the_counts_are_kept_and_the_gap_is_said(self):
        def answers(sql):
            if "context_status" in sql:
                return {"rc": 1, "stderr": 'ERROR:  column "context_status" does not exist'}
            return {"stdout": json.dumps({"documents": 104, "units": 1234, "embedded": 1234}) + chr(10)}

        state, _ = self.state_with(answers)
        progress = state.progress("org-corpus-contextual")
        self.assertEqual(progress["units"], 1234)
        self.assertTrue(any("migration" in key for key in progress["context_status"]))

    def test_another_database_error_is_not_swallowed(self):
        state, _ = self.state_with(lambda sql: {"rc": 1, "stderr": "ERROR:  connection refused"})
        with self.assertRaises(ingest.IngestError):
            state.progress("org-corpus-contextual")

    def test_waiting_ends_when_every_unit_is_embedded_and_not_before(self):
        states = iter([{"documents": 104, "units": 100, "embedded": 40, "context_status": {}},
                       {"documents": 104, "units": 100, "embedded": 90, "context_status": {}},
                       {"documents": 104, "units": 100, "embedded": 100, "context_status": {}}])

        class Fake:
            def progress(self, org):
                return next(states)

        slept = []
        ticks = iter(range(0, 1000, 10))
        done = ingest.wait_until_indexed(Fake(), "org-corpus-contextual", 3600, slept.append, lambda: next(ticks), io.StringIO())
        self.assertTrue(done)
        self.assertEqual(len(slept), 2)

    def test_waiting_gives_up_at_the_deadline_and_says_it_did_not_finish(self):
        class Stuck:
            def progress(self, org):
                return {"documents": 104, "units": 100, "embedded": 40, "context_status": {}}

        ticks = iter(range(0, 10000, 100))
        done = ingest.wait_until_indexed(Stuck(), "org-corpus-contextual", 250, lambda _s: None, lambda: next(ticks), io.StringIO())
        self.assertFalse(done)


class TheDockerTransport(unittest.TestCase):
    """The command that reaches a service: the token only in the exec environment, the headers as curl arguments, the
    right container for the port, and no secret in anything that is raised."""

    def fake_run(self, stdout=b'{"a": 1}' + bytes([10]) + b'201', exc=None):
        calls = []

        def run(argv, **kwargs):
            calls.append((argv, kwargs))
            if exc:
                raise exc
            import types
            return types.SimpleNamespace(stdout=stdout, stderr=b"", returncode=0)

        return run, calls

    def test_the_token_is_only_in_the_exec_environment_and_the_headers_are_curl_arguments(self):
        run, calls = self.fake_run()
        transport = ingest.DockerCurlTransport(run=run)
        status, body = transport.request("POST", "http://127.0.0.1:8013/v1/evals/results", "SECRETTOKEN", {"x": 1},
                                         {"Idempotency-Key": "harness-run-1"})
        self.assertEqual((status, body), (201, {"a": 1}))
        argv, kwargs = calls[0]
        script = argv[argv.index("-c") + 1]
        self.assertNotIn("SECRETTOKEN", script)
        self.assertIn("TOK=SECRETTOKEN", argv)
        self.assertEqual(argv[argv.index("-e") + 2], "data-plane-v2-data-quality-1")
        self.assertIn("Idempotency-Key: harness-run-1", argv)
        self.assertEqual(kwargs["input"], b'{"x": 1}')

    def test_the_docker_executable_can_be_overridden_and_defaults_to_docker(self):
        import os
        saved = os.environ.pop("EVAL_DOCKER", None)
        self.addCleanup(lambda: os.environ.__setitem__("EVAL_DOCKER", saved) if saved is not None else os.environ.pop("EVAL_DOCKER", None))
        self.assertEqual(ingest.docker_argv("exec", "x"), ["docker", "exec", "x"])
        os.environ["EVAL_DOCKER"] = "docker --context staging"
        self.assertEqual(ingest.docker_argv("exec", "x"), ["docker", "--context", "staging", "exec", "x"])
        run, calls = self.fake_run()
        ingest.DockerCurlTransport(run=run).request("GET", "http://127.0.0.1:8013/v1/quality/gates", "t")
        self.assertEqual(calls[0][0][:3], ["docker", "--context", "staging"])
        os.environ["EVAL_DOCKER"] = ""
        self.assertEqual(ingest.docker_argv("exec"), ["docker", "exec"])

    def test_a_docker_override_that_is_not_a_command_line_is_a_clear_error(self):
        import os
        saved = os.environ.get("EVAL_DOCKER")
        self.addCleanup(lambda: os.environ.__setitem__("EVAL_DOCKER", saved) if saved is not None else os.environ.pop("EVAL_DOCKER", None))
        os.environ["EVAL_DOCKER"] = 'docker --context "unbalanced'
        with self.assertRaises(ingest.IngestError) as raised:
            ingest.docker_argv("exec")
        self.assertIn("EVAL_DOCKER", str(raised.exception))
        build = load("eval-build-golden-set.py")
        with self.assertRaises(build.BuildError):
            build.docker_argv("exec")

    def test_each_port_reaches_its_own_container(self):
        for url, container in (("http://127.0.0.1:8010/v1/documents/", "data-plane-v2-documents-api-1"),
                               ("http://127.0.0.1:8013/v1/quality/gates", "data-plane-v2-data-quality-1")):
            run, calls = self.fake_run()
            ingest.DockerCurlTransport(run=run).request("GET", url, "t")
            self.assertIn(container, calls[0][0])

    def test_a_failure_never_carries_the_token(self):
        for exc in (OSError("docker: SECRETTOKEN"), subprocess.TimeoutExpired(["docker", "-e", "TOK=SECRETTOKEN"], 5)):
            run, _ = self.fake_run(exc=exc)
            with self.assertRaises(ingest.IngestError) as raised:
                ingest.DockerCurlTransport(run=run).request("GET", "http://127.0.0.1:8013/x/", "SECRETTOKEN")
            self.assertNotIn("SECRETTOKEN", str(raised.exception))

    def test_a_header_that_could_smuggle_another_is_refused(self):
        run, calls = self.fake_run()
        for headers in ({"X-A": "v" + chr(13) + chr(10) + "X-B: w"}, {"Bad Name": "v"}):
            with self.assertRaises(ingest.IngestError):
                ingest.DockerCurlTransport(run=run).request("GET", "http://127.0.0.1:8013/x/", "t", None, headers)
        self.assertEqual(calls, [])

    def test_an_answer_without_a_status_is_an_error_and_a_body_that_is_not_json_is_kept_short(self):
        run, _ = self.fake_run(stdout=b"curl: (7) failed to connect")
        with self.assertRaises(ingest.IngestError):
            ingest.DockerCurlTransport(run=run).request("GET", "http://127.0.0.1:8013/x/", "t")
        run, _ = self.fake_run(stdout=b"<html>" + b"x" * 1000 + bytes([10]) + b"502")
        status, body = ingest.DockerCurlTransport(run=run).request("GET", "http://127.0.0.1:8013/x/", "t")
        self.assertEqual(status, 502)
        self.assertLessEqual(len(body["raw"]), 200)


if __name__ == "__main__":
    unittest.main()
