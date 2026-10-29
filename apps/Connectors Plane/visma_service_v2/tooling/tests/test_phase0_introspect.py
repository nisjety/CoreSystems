"""Smoke tests for Phase 0 CLI wiring."""

from __future__ import annotations

import json
from pathlib import Path
from types import SimpleNamespace

import pytest
from typer.testing import CliRunner

from vsv2_tooling.discovery import introspect
from vsv2_tooling.discovery.cli import app


@pytest.mark.unit
def test_help_lists_all_subcommands() -> None:
    runner = CliRunner()
    result = runner.invoke(app, ["--help"])
    assert result.exit_code == 0
    for cmd in (
        "introspect",
        "companies",
        "coverage",
        "probe",
        "generate-queries",
        "validate-queries",
        "review-candidates",
    ):
        assert cmd in result.stdout


@pytest.mark.unit
def test_introspect_writes_json_and_sdl_and_closes_clients(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    schema = {
        "types": [
            {
                "kind": "OBJECT",
                "name": "Query",
                "fields": [
                    {
                        "name": "accounts",
                        "type": {"kind": "LIST", "ofType": {"kind": "OBJECT", "name": "Account"}},
                    },
                ],
            },
            {"kind": "ENUM", "name": "Status", "enumValues": [{"name": "ACTIVE"}]},
        ],
    }
    closed: list[str] = []

    class FakeOAuth:
        def __init__(self, _env: object) -> None:
            pass

        def close(self) -> None:
            closed.append("oauth")

    class FakeGraphQL:
        def __init__(self, _env: object, _oauth: object) -> None:
            pass

        def execute(self, query: str) -> dict:
            assert "IntrospectionQuery" in query
            return {"__schema": schema}

        def close(self) -> None:
            closed.append("graphql")

    monkeypatch.setattr(
        introspect.VismaEnv,
        "from_env",
        lambda: SimpleNamespace(datamart_url="https://example.test/graphql"),
    )
    monkeypatch.setattr(introspect, "OAuth", FakeOAuth)
    monkeypatch.setattr(introspect, "DatamartGraphQL", FakeGraphQL)
    out = tmp_path / "datamart.sdl"

    introspect.run(str(out))

    assert "type Query" in out.read_text()
    assert "enum Status" in out.read_text()
    assert json.loads(out.with_suffix(".introspection.json").read_text()) == {"__schema": schema}
    assert closed == ["graphql", "oauth"]


@pytest.mark.unit
def test_list_companies_reports_ids_without_mutating_etl(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    closed: list[str] = []

    class FakeOAuth:
        def __init__(self, _env: object) -> None:
            pass

        def close(self) -> None:
            closed.append("oauth")

    class FakeGraphQL:
        def __init__(self, _env: object, _oauth: object) -> None:
            pass

        def execute(self, query: str) -> dict:
            assert "accounts(pageSize: 100)" in query
            return {"accounts": [{"companyId": 99}, {"companyId": 99}]}

        def close(self) -> None:
            closed.append("graphql")

    monkeypatch.setattr(introspect.VismaEnv, "from_env", lambda: object())
    monkeypatch.setattr(introspect, "OAuth", FakeOAuth)
    monkeypatch.setattr(introspect, "DatamartGraphQL", FakeGraphQL)

    introspect.list_companies()

    assert "99" in capsys.readouterr().out
    assert closed == ["graphql", "oauth"]
