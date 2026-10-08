"""Tests of scripts/eval-service.py: the harness's client of data-quality-go (CR-T14).

It fetches the evidence the gate judges (coverage, release gates, the state of a variant) and sends the
records the gate wrote. What it must get right:

  * a token is minted per organisation, for the one scope it needs, and never printed;
  * evidence that could not be read is never written (a missing file fails the gate, a stale one would not);
  * an invalid record is refused BEFORE it is sent, and a refused or conflicting one is reported, not swallowed;
  * the state it captures is exactly the shape the promotion gate judges, which a test proves by judging it.
"""
from __future__ import annotations

import contextlib
import io
import json
import shutil
import tempfile
import unittest
from pathlib import Path

from _scripts import FIXTURES, load
from test_eval_gate import fixture
from test_eval_gate_promotion import RUN_ID

service = load("eval-service.py")
gate = load("eval-gate.py")
ORGS = {"baseline": "org-corpus-baseline", "contextual": "org-corpus-contextual"}


class FakeDataQuality:
    """data-quality-go as far as the harness relies on it. Bodies of the two evidence routes are the real ones
    (the pinned goldens of eval-evidence-v1); the results route enforces what the real one does: operator
    service token of the organisation, the key header, and the validation rules (through the real gate mirror)."""

    def __init__(self, test: unittest.TestCase):
        self.test = test
        self.requests: list[dict] = []
        self.minted: list[tuple[str, tuple[str, ...]]] = []
        self.status_for: dict[str, int] = {}
        self.stored: dict[tuple[str, str], dict] = {}
        self.policy = {"org-corpus-baseline": "off", "org-corpus-contextual": "inherit"}

    def mint(self, org, scopes, reason):
        self.test.assertTrue(reason)
        self.minted.append((org, tuple(scopes)))
        return f"tok:{org}"

    def request(self, method, url, token, body=None, headers=None):
        org = token.split(":", 1)[1]
        path = url.split("8013", 1)[1]
        self.requests.append({"method": method, "path": path, "org": org, "body": body, "headers": headers or {}})
        forced = self.status_for.get(path)
        if forced:
            return forced, {"error": "forced"}
        label = "baseline" if org == ORGS["baseline"] else "contextual"
        if path == "/v1/quality/context-coverage":
            name = "coverage.baseline-off.golden.json" if label == "baseline" else "coverage.contextual-ready.golden.json"
            return 200, fixture(name)
        if path == "/v1/quality/gates":
            return 200, fixture("gates.in-sync.golden.json")
        if path.endswith("/processing-policy"):
            return 200, {"org_id": org, "policy": {"contextual_retrieval": self.policy[org], "context_prompt_version": "ctx-v2",
                                                   "source": "policy"}}
        if path == "/v1/evals/results" and method == "POST":
            return self.record(org, body, headers or {})
        return 404, {}

    def record(self, org, body, headers):
        key = headers.get("Idempotency-Key", "")
        if not 8 <= len(key) <= 120:  # the real handler's bound (quality.go requestIdempotencyKey), not the table's 128
            return 400, {"code": "invalid_idempotency_key"}
        problems = gate.validate_record(body)
        if problems:
            return 400, {"code": "invalid_record", "problems": problems}
        previous = self.stored.get((org, key))
        if previous is not None:
            return (200, {"eval_id": "x"}) if previous == body else (409, {"code": "idempotency_conflict"})
        self.stored[(org, key)] = body
        return 201, {"eval_id": "y"}

    # psql: what is stored about the units of an organisation
    def provenance(self, org):
        return [{"prompt_version": "ctx-v2", "model": "gpt-4o-mini", "context_status": "ready", "units": 1164}]

    def retrieval_flag(self):
        return "0"


def run(test, fake, argv, environ=None):
    out, err = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        code = service.main(argv, transport=fake, mint=fake.mint, state=fake, environ={"DATA_PLANE_CORPUS_SEEDER_API_KEY": "present"} if environ is None else environ)
    return code, out.getvalue() + err.getvalue()


def tmp(test) -> Path:
    path = Path(tempfile.mkdtemp(prefix="evalservice-"))
    test.addCleanup(shutil.rmtree, path, True)
    return path


class Evidence(unittest.TestCase):
    def test_coverage_writes_one_report_per_organisation_and_asks_for_the_admin_scope_only(self):
        fake, work = FakeDataQuality(self), tmp(self)
        code, out = run(self, fake, ["coverage", str(work)])
        self.assertEqual(code, 0, out)
        for label in ORGS:
            body = json.loads((work / f"coverage-{label}.json").read_text(encoding="utf-8"))
            self.assertEqual(body["org_id"], ORGS[label])
        self.assertEqual(sorted(fake.minted), sorted((org, ("data:quality:admin",)) for org in ORGS.values()))
        self.assertEqual({r["org"] for r in fake.requests}, set(ORGS.values()))

    def test_a_report_that_could_not_be_read_is_not_written_and_fails(self):
        fake, work = FakeDataQuality(self), tmp(self)
        (work / "coverage-contextual.json").write_text("{}", encoding="utf-8")  # a stale file from an earlier run
        fake.status_for["/v1/quality/context-coverage"] = 503
        code, out = run(self, fake, ["coverage", str(work)])
        self.assertEqual(code, 1, out)
        self.assertIn("503", out)
        self.assertFalse((work / "coverage-contextual.json").exists(), "a stale report must not survive a failed fetch")
        self.assertFalse((work / "coverage-baseline.json").exists())

    def test_gates_accepts_200_and_412_and_nothing_else(self):
        fake, work = FakeDataQuality(self), tmp(self)
        self.assertEqual(run(self, fake, ["gates", str(work)])[0], 0)
        fake.status_for["/v1/quality/gates"] = 412
        code, out = run(self, fake, ["gates", str(work)])
        self.assertEqual(code, 1, out)  # forced answers carry no report: a 412 WITHOUT a gate report is not evidence
        fake.status_for["/v1/quality/gates"] = 401
        self.assertEqual(run(self, fake, ["gates", str(work)])[0], 1)

    def test_a_412_with_a_real_report_is_the_evidence_that_a_gate_failed(self):
        class Failing(FakeDataQuality):
            def request(self, method, url, token, body=None, headers=None):
                if url.endswith("/v1/quality/gates"):
                    return 412, fixture("gates.parity-unavailable.golden.json")
                return super().request(method, url, token, body, headers)

        fake, work = Failing(self), tmp(self)
        code, out = run(self, fake, ["gates", str(work)])
        self.assertEqual(code, 0, out)
        body = json.loads((work / "gates-contextual.json").read_text(encoding="utf-8"))
        self.assertFalse(body["all_passed"])

    def test_the_credential_is_required_and_never_echoed(self):
        fake, work = FakeDataQuality(self), tmp(self)
        code, out = run(self, fake, ["coverage", str(work)], environ={})
        self.assertEqual(code, 2, out)
        self.assertIn("DATA_PLANE_CORPUS_SEEDER_API_KEY", out)
        code, out = run(self, fake, ["coverage", str(work)], environ={"DATA_PLANE_CORPUS_SEEDER_API_KEY": "s3cr3t-value"})
        self.assertNotIn("s3cr3t-value", out)


class Records(unittest.TestCase):
    def write_records(self, work: Path) -> Path:
        directory = work / "records"
        directory.mkdir()
        body = json.loads((FIXTURES / "harness-record-v1" / "record.cell.golden.json").read_text(encoding="utf-8"))
        for label, cell in (("contextual", "contextual"), ("baseline", "baseline")):
            record = {"org_label": label, "org_id": ORGS[label], "idempotency_key": f"harness-{RUN_ID}-{cell}",
                      "body": json.loads(json.dumps(body).replace("harness:contextual", f"harness:{cell}").replace('"cell": "contextual"', f'"cell": "{cell}"'))}
            (directory / f"record-{cell}.json").write_text(json.dumps(record), encoding="utf-8")
        return directory

    def test_every_record_is_sent_to_its_own_organisation_with_its_key(self):
        fake, work = FakeDataQuality(self), tmp(self)
        directory = self.write_records(work)
        code, out = run(self, fake, ["record", str(directory)])
        self.assertEqual(code, 0, out)
        sent = [r for r in fake.requests if r["method"] == "POST"]
        self.assertEqual(len(sent), 2)
        for request in sent:
            self.assertEqual(request["path"], "/v1/evals/results")
            self.assertTrue(request["headers"]["Idempotency-Key"].startswith("harness-"))
        self.assertEqual({r["org"] for r in sent}, set(ORGS.values()))
        self.assertIn("recorded 2", out)

    def test_a_replay_is_fine_and_a_changed_replay_is_reported(self):
        fake, work = FakeDataQuality(self), tmp(self)
        directory = self.write_records(work)
        self.assertEqual(run(self, fake, ["record", str(directory)])[0], 0)
        code, out = run(self, fake, ["record", str(directory)])
        self.assertEqual(code, 0, out)
        self.assertIn("already recorded", out)
        path = directory / "record-contextual.json"
        record = json.loads(path.read_text(encoding="utf-8"))
        record["body"]["scorecard"]["mean_mrr"] = 0.5
        path.write_text(json.dumps(record), encoding="utf-8")
        code, out = run(self, fake, ["record", str(directory)])
        self.assertEqual(code, 1, out)
        self.assertIn("conflict", out)

    def test_an_invalid_record_is_refused_before_it_is_sent(self):
        fake, work = FakeDataQuality(self), tmp(self)
        directory = self.write_records(work)
        path = directory / "record-contextual.json"
        record = json.loads(path.read_text(encoding="utf-8"))
        record["body"]["scorecard"]["mean_ndcg_at_10"] = 7
        path.write_text(json.dumps(record), encoding="utf-8")
        code, out = run(self, fake, ["record", str(directory)])
        self.assertEqual(code, 1, out)
        self.assertIn("record-contextual.json", out)
        self.assertEqual(sum(1 for r in fake.requests if r["method"] == "POST" and r["org"] == ORGS["contextual"]), 0)

    def test_a_service_that_refuses_is_reported_with_its_status(self):
        fake, work = FakeDataQuality(self), tmp(self)
        directory = self.write_records(work)
        fake.status_for["/v1/evals/results"] = 403
        code, out = run(self, fake, ["record", str(directory)])
        self.assertEqual(code, 1, out)
        self.assertIn("403", out)

    def test_nothing_to_record_is_a_failure_not_a_pass(self):
        fake, work = FakeDataQuality(self), tmp(self)
        empty = work / "records"
        empty.mkdir()
        code, out = run(self, fake, ["record", str(empty)])
        self.assertEqual(code, 1, out)
        self.assertIn("no records", out)

    def test_a_record_for_an_organisation_the_harness_does_not_know_is_refused(self):
        fake, work = FakeDataQuality(self), tmp(self)
        directory = self.write_records(work)
        path = directory / "record-contextual.json"
        record = json.loads(path.read_text(encoding="utf-8"))
        record["org_id"] = "org-somebody-else"
        path.write_text(json.dumps(record), encoding="utf-8")
        code, out = run(self, fake, ["record", str(directory)])
        self.assertEqual(code, 1, out)
        self.assertIn("org-somebody-else", out)


class VariantState(unittest.TestCase):
    def test_the_captured_state_is_what_the_promotion_gate_judges(self):
        fake, work = FakeDataQuality(self), tmp(self)
        code, out = run(self, fake, ["state", str(work), "ctx-v2", "--prompt", "ctx-v2"])
        self.assertEqual(code, 0, out)
        state = json.loads((work / "state-ctx-v2.json").read_text(encoding="utf-8"))
        self.assertEqual(state["schema"], "eval-state-v1")
        self.assertEqual(state["declared"], {"prompt_version": "ctx-v2", "model": None, "rerank_input_include_context": None})
        self.assertEqual(state["observed"]["policy"]["contextual_retrieval"], "inherit")
        self.assertEqual(state["observed"]["provenance"][0]["units"], 1164)
        self.assertEqual(gate.judge_state("ctx-v2", state), [], "the gate must accept the state that matches the declaration")

    def test_a_state_that_is_not_the_declared_one_is_captured_as_it_is_and_the_gate_refuses_it(self):
        fake, work = FakeDataQuality(self), tmp(self)
        code, _ = run(self, fake, ["state", str(work), "ctx-v1", "--prompt", "ctx-v1"])
        self.assertEqual(code, 0, "the capture records what IS; judging it is the gate's job")
        state = json.loads((work / "state-ctx-v1.json").read_text(encoding="utf-8"))
        self.assertTrue(gate.judge_state("ctx-v1", state))

    def test_the_rerank_input_flag_is_declared_and_observed(self):
        fake, work = FakeDataQuality(self), tmp(self)
        fake.retrieval_flag = lambda: "1"
        code, out = run(self, fake, ["state", str(work), "rerank-ctx", "--prompt", "ctx-v1", "--rerank-input-context", "1"])
        self.assertEqual(code, 0, out)
        state = json.loads((work / "state-rerank-ctx.json").read_text(encoding="utf-8"))
        self.assertIs(state["declared"]["rerank_input_include_context"], True)
        self.assertEqual(state["observed"]["retrieval"]["rerank_input_include_context"], "1")

    def test_breadcrumb_is_refused_because_it_is_not_built(self):
        fake, work = FakeDataQuality(self), tmp(self)
        code, out = run(self, fake, ["state", str(work), "breadcrumb"])
        self.assertEqual(code, 2, out)
        self.assertIn("CR-T18", out)
        self.assertFalse((work / "state-breadcrumb.json").exists())

    def test_a_variant_name_cannot_escape_the_work_directory(self):
        fake, work = FakeDataQuality(self), tmp(self)
        code, out = run(self, fake, ["state", str(work), "../escape", "--prompt", "ctx-v2"])
        self.assertEqual(code, 2, out)


if __name__ == "__main__":
    unittest.main()
