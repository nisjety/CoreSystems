"""Tests for scripts/gates/scenario-touch.py (EP-14 S8). Run from the repository root with pytest."""
import importlib.util
import json
import subprocess
from pathlib import Path

import pytest

GATE = Path(__file__).resolve().parents[1] / "scenario-touch.py"
_spec = importlib.util.spec_from_file_location("scenario_touch", GATE)
gate = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(gate)

REPO_ROOT = GATE.parents[2]
DIGEST = "a" * 64
OTHER_DIGEST = "b" * 64
CASE = gate.CASES + "baseline/13-compaction.yaml"
CASSETTE = gate.CASSETTES + "session-1.jsonl"
SOURCE = "apps/Model Plane/python/eval-lab-py/src/eval_lab/types.py"


def exception(digest=DIGEST):
    return {"manifest_sha256": digest, "reason": "reviewed bootstrap", "reviewed_by": "reviewer"}


def write_exceptions(repo, document):
    path = repo / gate.EXCEPTIONS
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(document), encoding="utf-8")


def git(repo, *args):
    subprocess.run(
        [
            "git",
            "-C",
            str(repo),
            "-c",
            "user.name=Gate Test",
            "-c",
            "user.email=gate@example.invalid",
            "-c",
            "commit.gpgsign=false",
            *args,
        ],
        check=True,
        capture_output=True,
        text=True,
    )


def test_an_unchanged_manifest_passes():
    code, _ = gate.decide(["README.md", SOURCE], DIGEST, [])
    assert code == gate.OK


def test_a_manifest_change_without_a_scenario_is_red():
    code, message = gate.decide([gate.MANIFEST], DIGEST, [])
    assert code == gate.FAIL
    assert gate.MANIFEST in message
    assert DIGEST in message


def test_a_manifest_change_with_a_case_is_green():
    code, _ = gate.decide([gate.MANIFEST, CASE], DIGEST, [])
    assert code == gate.OK


def test_a_manifest_change_with_a_cassette_is_green():
    code, _ = gate.decide([gate.MANIFEST, CASSETTE], DIGEST, [])
    assert code == gate.OK


def test_a_pin_change_alone_is_a_manifest_change():
    code, _ = gate.decide([gate.PIN], DIGEST, [])
    assert code == gate.FAIL


def test_eval_source_changes_are_not_scenarios():
    code, _ = gate.decide([gate.MANIFEST, SOURCE], DIGEST, [])
    assert code == gate.FAIL


def test_a_reviewed_exception_for_this_digest_is_green():
    code, _ = gate.decide([gate.MANIFEST], DIGEST, [exception()])
    assert code == gate.OK


def test_an_exception_for_another_digest_does_not_cover_this_change():
    code, _ = gate.decide([gate.MANIFEST], DIGEST, [exception(OTHER_DIGEST)])
    assert code == gate.FAIL


def test_a_deleted_manifest_cannot_be_excepted():
    code, _ = gate.decide([gate.MANIFEST], None, [exception()])
    assert code == gate.FAIL


def test_a_malformed_exceptions_file_is_refused(tmp_path):
    write_exceptions(
        tmp_path,
        {"schema": gate.EXCEPTIONS_SCHEMA, "exceptions": [{"manifest_sha256": "xyz", "reason": "r", "reviewed_by": "r"}]},
    )
    with pytest.raises(gate.GateError):
        gate.load_exceptions(tmp_path)


def test_an_exception_without_a_reviewer_is_refused(tmp_path):
    write_exceptions(
        tmp_path,
        {"schema": gate.EXCEPTIONS_SCHEMA, "exceptions": [{"manifest_sha256": DIGEST, "reason": "r", "reviewed_by": " "}]},
    )
    with pytest.raises(gate.GateError):
        gate.load_exceptions(tmp_path)


def test_the_manifest_digest_ignores_line_endings(tmp_path):
    path = tmp_path / gate.MANIFEST
    path.parent.mkdir(parents=True)
    path.write_bytes(b'{"a": 1}\n')
    lf = gate.manifest_digest(tmp_path)
    path.write_bytes(b'{"a": 1}\r\n')
    assert gate.manifest_digest(tmp_path) == lf


def test_the_committed_exceptions_file_loads_empty():
    assert gate.load_exceptions(REPO_ROOT) == []


def test_the_committed_manifest_digest_matches_its_pin():
    pin = (REPO_ROOT / gate.PIN).read_text(encoding="utf-8").split()[0]
    assert gate.manifest_digest(REPO_ROOT) == pin


def test_the_gate_reads_the_pull_request_diff_against_its_base(tmp_path):
    git(tmp_path, "init", "-q", "-b", "main")
    manifest = tmp_path / gate.MANIFEST
    manifest.parent.mkdir(parents=True)
    manifest.write_text('{"schema": "x", "entries": []}\n', encoding="utf-8")
    git(tmp_path, "add", "-A")
    git(tmp_path, "commit", "-q", "-m", "base")
    git(tmp_path, "checkout", "-q", "-b", "feature")
    manifest.write_text('{"schema": "x", "entries": [1]}\n', encoding="utf-8")
    git(tmp_path, "commit", "-q", "-am", "prompt change")
    assert gate.main(["--base", "main", "--repo", str(tmp_path)]) == gate.FAIL

    case = tmp_path / CASE
    case.parent.mkdir(parents=True)
    case.write_text("name: touched\n", encoding="utf-8")
    git(tmp_path, "add", "-A")
    git(tmp_path, "commit", "-q", "-m", "scenario")
    assert gate.main(["--base", "main", "--repo", str(tmp_path)]) == gate.OK


def test_an_unknown_base_cannot_run(tmp_path):
    git(tmp_path, "init", "-q", "-b", "main")
    (tmp_path / "f.txt").write_text("x", encoding="utf-8")
    git(tmp_path, "add", "-A")
    git(tmp_path, "commit", "-q", "-m", "base")
    assert gate.main(["--base", "origin/does-not-exist", "--repo", str(tmp_path)]) == gate.CANNOT_RUN
