"""Tests for the Dataform deploy tool. The API is faked: no network, no credentials.

The bug these guard against: a failing listing was caught and read as an empty folder, so push
rewrote every file, --prune removed nothing, and commit never deleted a file from main.
"""

from __future__ import annotations

import base64
import sys
import urllib.parse
from collections.abc import Callable
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import deploy

TREE = {
    "": [{"directory": "definitions"}, {"file": "workflow_settings.yaml"}],
    "definitions": [{"directory": "definitions/assertions"}, {"file": "definitions/a.sqlx"}],
    "definitions/assertions": [{"file": "definitions/assertions/x.sqlx"}],
}


def _query(url: str) -> dict[str, str]:
    return dict(urllib.parse.parse_qsl(urllib.parse.urlsplit(url).query))


def tree_api(
    tree: dict[str, list[dict]], failing: dict[str, int] | None = None
) -> Callable[..., dict]:
    """A deploy.api stand-in that lists `tree`, and fails with the given status for the given folders."""
    failing = failing or {}

    def api(method: str, url: str, body: dict | None = None) -> dict:
        folder = _query(url).get("path", "")
        if folder in failing:
            raise deploy.ApiError(failing[folder], method, url, "fake failure")
        return {"directoryEntries": tree.get(folder, [])}

    return api


# ─── listing ──────────────────────────────────────────────────────────────────


@pytest.mark.unit
def test_listing_uses_the_full_paths_the_api_gives(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(deploy, "api", tree_api(TREE))

    assert deploy.list_files("workspace") == {
        "workflow_settings.yaml",
        "definitions/a.sqlx",
        "definitions/assertions/x.sqlx",
    }


@pytest.mark.unit
def test_the_repository_names_nested_entries_by_their_name_in_the_folder(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """main is listed with bare names below the root; the same files must come out as full paths."""
    bare = {
        "": [{"directory": "definitions"}, {"file": "workflow_settings.yaml"}],
        "definitions": [{"directory": "assertions"}, {"file": "a.sqlx"}],
        "definitions/assertions": [{"file": "x.sqlx"}],
    }
    monkeypatch.setattr(deploy, "api", tree_api(bare))

    assert deploy.list_files("repository") == {
        "workflow_settings.yaml",
        "definitions/a.sqlx",
        "definitions/assertions/x.sqlx",
    }


@pytest.mark.unit
@pytest.mark.parametrize(
    ("folder", "name", "path"),
    [
        ("", "definitions", "definitions"),
        ("definitions", "gold", "definitions/gold"),
        ("definitions", "definitions/gold", "definitions/gold"),
        ("definitions/gold", "x.sqlx", "definitions/gold/x.sqlx"),
    ],
)
def test_an_entry_is_joined_onto_its_folder_only_when_it_is_a_bare_name(
    folder: str, name: str, path: str
) -> None:
    assert deploy.entry_path(folder, name) == path


@pytest.mark.unit
def test_a_failing_folder_raises_instead_of_counting_as_empty(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(deploy, "api", tree_api(TREE, failing={"definitions/assertions": 404}))

    with pytest.raises(deploy.ApiError) as raised:
        deploy.list_files("workspace")

    assert raised.value.status == 404


@pytest.mark.unit
def test_every_page_of_a_folder_is_read(monkeypatch: pytest.MonkeyPatch) -> None:
    pages = [
        {"directoryEntries": [{"file": "definitions/a.sqlx"}], "nextPageToken": "2"},
        {"directoryEntries": [{"file": "definitions/b.sqlx"}]},
    ]
    calls: list[str] = []

    def api(method: str, url: str, body: dict | None = None) -> dict:
        calls.append(url)
        return pages[len(calls) - 1]

    monkeypatch.setattr(deploy, "api", api)

    assert deploy.list_files("workspace") == {"definitions/a.sqlx", "definitions/b.sqlx"}
    assert _query(calls[1])["pageToken"] == "2"


# ─── main ─────────────────────────────────────────────────────────────────────


@pytest.mark.unit
def test_a_repository_with_no_main_yet_is_printed_as_empty(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    def api(method: str, url: str, body: dict | None = None) -> dict:
        raise deploy.ApiError(404, method, url, "no main")

    monkeypatch.setattr(deploy, "api", api)

    assert deploy.main_files() == {}
    assert "treating the repository as empty" in capsys.readouterr().out


@pytest.mark.unit
def test_any_other_failure_on_main_stops_the_commit(monkeypatch: pytest.MonkeyPatch) -> None:
    def api(method: str, url: str, body: dict | None = None) -> dict:
        raise deploy.ApiError(500, method, url, "backend error")

    monkeypatch.setattr(deploy, "api", api)

    with pytest.raises(deploy.ApiError):
        deploy.main_files()


@pytest.mark.unit
def test_main_lists_only_project_files(monkeypatch: pytest.MonkeyPatch) -> None:
    tree = {
        "": [{"directory": "definitions"}, {"directory": "docs"}],
        "definitions": [{"file": "definitions/a.sqlx"}, {"file": "definitions/data.csv"}],
        "docs": [{"file": "docs/notes.md"}],
    }
    contents = {"definitions/a.sqlx": b"A"}

    def api(method: str, url: str, body: dict | None = None) -> dict:
        if ":readFile" in url:
            path = _query(url)["path"]
            return {"contents": base64.b64encode(contents[path]).decode()}
        return tree_api(tree)(method, url, body)

    monkeypatch.setattr(deploy, "api", api)

    assert deploy.main_files() == {"definitions/a.sqlx": b"A"}


# ─── commit planning ──────────────────────────────────────────────────────────


@pytest.mark.unit
def test_commit_writes_what_differs_and_deletes_only_project_files_gone_from_the_project() -> None:
    local = {
        "definitions/a.sqlx": b"new",
        "definitions/same.sqlx": b"same",
        "workflow_settings.yaml": b"w",
    }
    main = {
        "definitions/a.sqlx": b"old",
        "definitions/same.sqlx": b"same",
        "definitions/gone.sqlx": b"x",
        "definitions/data.csv": b"not a project file",
        "docs/notes.md": b"not sent",
        "tools/plan.py": b"not sent",
    }

    operations = deploy.plan_commit(local, main)

    assert set(operations) == {
        "definitions/a.sqlx",
        "workflow_settings.yaml",
        "definitions/gone.sqlx",
    }
    assert operations["definitions/a.sqlx"] == {
        "writeFile": {"contents": base64.b64encode(b"new").decode()}
    }
    assert operations["definitions/gone.sqlx"] == {"deleteFile": {}}


@pytest.mark.unit
def test_an_identical_main_needs_no_commit() -> None:
    files = {"definitions/a.sqlx": b"a", "workflow_settings.yaml": b"w"}

    assert deploy.plan_commit(files, dict(files)) == {}


# ─── push ─────────────────────────────────────────────────────────────────────


@pytest.mark.unit
def test_push_writes_only_what_changed_and_prunes_only_with_prune(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    local = {"definitions/a.sqlx": b"A", "definitions/b.sqlx": b"B"}
    workspace = {"definitions/a.sqlx": b"A", "definitions/old.sqlx": b"O"}
    tree = {
        "": [{"directory": "definitions"}],
        "definitions": [{"file": "definitions/a.sqlx"}, {"file": "definitions/old.sqlx"}],
    }
    writes: list[str] = []
    removes: list[str] = []

    def api(method: str, url: str, body: dict | None = None) -> dict:
        if ":queryDirectoryContents" in url:
            return tree_api(tree)(method, url, body)
        if ":readFile" in url:
            return {"contents": base64.b64encode(workspace[_query(url)["path"]]).decode()}
        if ":writeFile" in url:
            writes.append(body["path"])
            return {}
        if ":removeFile" in url:
            removes.append(body["path"])
            return {}
        raise AssertionError(f"unexpected call {method} {url}")

    monkeypatch.setattr(deploy, "api", api)
    monkeypatch.setattr(deploy, "local_files", lambda: local)

    deploy.push(prune=False)
    assert writes == ["definitions/b.sqlx"]
    assert removes == []

    writes.clear()
    deploy.push(prune=True)
    assert removes == ["definitions/old.sqlx"]
