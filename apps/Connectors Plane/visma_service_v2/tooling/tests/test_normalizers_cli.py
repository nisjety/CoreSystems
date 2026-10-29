"""``vsv2-normalize``: apply a registered normalizer to a JSONL stream."""

from __future__ import annotations

import json

import pytest
from typer.testing import CliRunner

from vsv2_tooling.normalizers.cli import app

runner = CliRunner()

DM_VAT_ZONE = {"vatZoneId": "01", "defaultVatCategoryId": "3", "description": "Innenlands"}


@pytest.mark.unit
def test_normalizes_a_file_to_a_file(tmp_path) -> None:
    src, dst = tmp_path / "in.jsonl", tmp_path / "out.jsonl"
    src.write_text(
        json.dumps(DM_VAT_ZONE) + "\n\n" + json.dumps({**DM_VAT_ZONE, "vatZoneId": "02"})
    )

    result = runner.invoke(
        app,
        ["--normalizer", "normalize_vat_zone", "--input", str(src), "--output", str(dst)],
    )

    assert result.exit_code == 0, result.output
    rows = [json.loads(line) for line in dst.read_text().splitlines()]
    assert [r["id"] for r in rows] == ["01", "02"]  # the blank line is skipped
    assert all(r["_source"] == "datamart" for r in rows)
    assert "normalized 2 record(s)" in result.output


@pytest.mark.unit
def test_reads_stdin_and_writes_stdout_when_no_files_are_given() -> None:
    result = runner.invoke(
        app, ["--normalizer", "normalize_vat_zone"], input=json.dumps(DM_VAT_ZONE) + "\n"
    )

    assert result.exit_code == 0, result.output
    assert json.loads(result.stdout.splitlines()[0])["id"] == "01"
    assert "normalized 1 record(s)" in result.stderr  # progress goes to stderr, data to stdout


@pytest.mark.unit
def test_an_unknown_normalizer_lists_the_available_ones_and_exits_one() -> None:
    result = runner.invoke(app, ["--normalizer", "normalize_nothing"], input="{}\n")

    assert result.exit_code == 1
    assert "Unknown normalizer: 'normalize_nothing'" in result.output
    assert "normalize_supplier" in result.output


@pytest.mark.unit
def test_an_empty_stream_normalizes_zero_records() -> None:
    result = runner.invoke(app, ["--normalizer", "normalize_vat_zone"], input="")

    assert result.exit_code == 0
    assert "normalized 0 record(s)" in result.output
