"""Tests for the Excel inputs loader. BigQuery is faked: no network, no credentials.

What they guard: a workbook that changed shape must stop the loader instead of loading a table with a
column missing, and `check` must say which rows differ, because the workbooks are business rules.
"""

from __future__ import annotations

import csv
import json
import sys
import zipfile
from pathlib import Path
from xml.sax.saxutils import escape

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import excel_inputs as ei

REAL_INPUTS = Path(__file__).resolve().parent.parent / "inputs" / "excel"


def _letters(index: int) -> str:
    letters = ""
    index += 1
    while index:
        index, rest = divmod(index - 1, 26)
        letters = chr(ord("A") + rest) + letters
    return letters


def make_xlsx(path: Path, sheets: dict[str, list[list[object]]], *, inline: bool = False) -> Path:
    """A minimal workbook: cells are str (shared or inline), int/float (numbers) or None (absent)."""
    shared: list[str] = []
    sheet_xml = []
    for rows in sheets.values():
        xml_rows = []
        for r, row in enumerate(rows, start=1):
            cells = []
            for c, value in enumerate(row):
                ref = f"{_letters(c)}{r}"
                if value is None:
                    continue
                if isinstance(value, str) and inline:
                    cells.append(f'<c r="{ref}" t="inlineStr"><is><t>{escape(value)}</t></is></c>')
                elif isinstance(value, str):
                    shared.append(value)
                    cells.append(f'<c r="{ref}" t="s"><v>{len(shared) - 1}</v></c>')
                else:
                    cells.append(f'<c r="{ref}"><v>{value}</v></c>')
            xml_rows.append(f'<row r="{r}">{"".join(cells)}</row>')
        sheet_xml.append(
            '<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">'
            f"<sheetData>{''.join(xml_rows)}</sheetData></worksheet>"
        )
    names = list(sheets)
    workbook = (
        '<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" '
        'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>'
        + "".join(
            f'<sheet name="{n}" sheetId="{i}" r:id="rId{i}"/>' for i, n in enumerate(names, 1)
        )
        + "</sheets></workbook>"
    )
    rels = (
        '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        + "".join(
            f'<Relationship Id="rId{i}" Type="worksheet" Target="worksheets/sheet{i}.xml"/>'
            for i in range(1, len(names) + 1)
        )
        + "</Relationships>"
    )
    with zipfile.ZipFile(path, "w") as book:
        book.writestr("xl/workbook.xml", workbook)
        book.writestr("xl/_rels/workbook.xml.rels", rels)
        if shared:
            items = "".join(f"<si><t>{escape(s)}</t></si>" for s in shared)
            book.writestr(
                "xl/sharedStrings.xml",
                f'<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">{items}</sst>',
            )
        for i, xml in enumerate(sheet_xml, 1):
            book.writestr(f"xl/worksheets/sheet{i}.xml", xml)
    return path


# ---------------------------------------------------------------------------------------------
# Reading
# ---------------------------------------------------------------------------------------------


@pytest.mark.parametrize("inline", [False, True])
def test_reads_text_numbers_and_gaps(tmp_path: Path, inline: bool) -> None:
    book = make_xlsx(
        tmp_path / "a.xlsx",
        {
            "Data": [
                ["id", "name", "n"],
                ["1661", "Aquatiq  ", 3000.0],
                [None, "only name", None],
                [],
            ]
        },
        inline=inline,
    )

    rows = ei.read_sheet(book, "Data")

    assert rows[0] == ["id", "name", "n"]
    assert rows[1] == ["1661", "Aquatiq", "3000"]  # text trimmed, 3000.0 read as 3000
    assert rows[2] == [None, "only name"]  # a gap before a value is filled with None
    assert rows[3] == []


def test_column_letters_past_z(tmp_path: Path) -> None:
    row: list[object] = [None] * 27 + ["far"]
    book = make_xlsx(tmp_path / "a.xlsx", {"Data": [row]})

    assert ei.read_sheet(book, "Data")[0][27] == "far"


def test_rich_text_runs_are_joined(tmp_path: Path) -> None:
    book = make_xlsx(tmp_path / "a.xlsx", {"Data": [["x"]]})
    rewritten = tmp_path / "rich.xlsx"  # the same book with a rich-text shared string table
    with zipfile.ZipFile(book) as source, zipfile.ZipFile(rewritten, "w") as target:
        for name in source.namelist():
            data = source.read(name)
            if name == "xl/sharedStrings.xml":
                data = (
                    b'<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">'
                    b"<si><r><t>Hygiene </t></r><r><t>Systems</t></r></si></sst>"
                )
            target.writestr(name, data)

    assert ei.read_sheet(rewritten, "Data") == [["Hygiene Systems"]]


def test_missing_sheet_names_the_sheets_that_exist(tmp_path: Path) -> None:
    book = make_xlsx(tmp_path / "a.xlsx", {"One": [["x"]], "Two": [["y"]]})

    with pytest.raises(ei.ExcelInputError, match=r"'Three'.*\['One', 'Two'\]"):
        ei.read_sheet(book, "Three")


# ---------------------------------------------------------------------------------------------
# From sheet to table rows
# ---------------------------------------------------------------------------------------------

SPEC = ei.ExcelInput("w.xlsx", "S", "raw_t", header=("Nr", "X"), columns=("nr", "marker"))


def test_table_rows_drops_empty_rows_and_pads_short_rows() -> None:
    rows = [["Nr", "X"], ["1A1", "x"], [], ["1A2"], [None, None]]

    assert ei.table_rows(SPEC, rows) == [("1A1", "x"), ("1A2", None)]


def test_a_changed_header_stops_the_load() -> None:
    with pytest.raises(ei.ExcelInputError, match="expected the header"):
        ei.table_rows(SPEC, [["Product", "X"], ["1A1", "x"]])


def test_a_value_beyond_the_known_columns_stops_the_load() -> None:
    with pytest.raises(ei.ExcelInputError, match=r"row 3.*beyond column 2"):
        ei.table_rows(SPEC, [["Nr", "X"], ["1A1", "x"], ["1A2", "x", "a new column"]])


def test_an_empty_sheet_stops_the_load() -> None:
    with pytest.raises(ei.ExcelInputError, match="empty"):
        ei.table_rows(SPEC, [])


# ---------------------------------------------------------------------------------------------
# BigQuery side, faked
# ---------------------------------------------------------------------------------------------


def test_difference_counts_duplicates() -> None:
    only_workbook, only_table = ei.difference([("a",), ("a",), ("b",)], [("a",), ("c",)])

    assert only_workbook == [("a",), ("b",)]
    assert only_table == [("c",)]


def test_read_table_selects_the_columns_in_order() -> None:
    seen: list[list[str]] = []

    def run(args: list[str]) -> str:
        seen.append(args)
        return json.dumps([{"marker": "x", "nr": "1A1"}, {"nr": "1A2", "marker": None}])

    rows = ei.read_table(SPEC, run)

    assert rows == [("1A1", "x"), ("1A2", None)]
    assert "FROM `aquatiq-datavarehus.visma_bronze_excel.raw_t`" in seen[0][-1]
    assert "SELECT `nr`, `marker`" in seen[0][-1]


def test_load_table_replaces_the_table_and_marks_nulls(tmp_path: Path) -> None:
    captured: dict[str, object] = {}

    def run(args: list[str]) -> str:
        captured["args"] = args
        captured["csv"] = Path(args[-2]).read_text(encoding="utf-8")
        return ""

    ei.load_table(SPEC, [("1A1", "x"), ('say "hi", ok', None)], run)

    args = captured["args"]
    assert isinstance(args, list)
    assert "--replace" in args
    assert "--location=europe-north2" in args
    assert args[-1] == "nr:STRING,marker:STRING"
    assert args[-3] == "aquatiq-datavarehus:visma_bronze_excel.raw_t"
    lines = list(csv.reader(str(captured["csv"]).splitlines()))
    assert lines == [["nr", "marker"], ["1A1", "x"], ['say "hi", ok', "\\N"]]


def workbook_for(tmp_path: Path, rows: list[list[object]]) -> Path:
    make_xlsx(tmp_path / SPEC.workbook, {SPEC.sheet: rows})
    return tmp_path


def test_check_is_clean_when_the_table_equals_the_workbook(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    folder = workbook_for(tmp_path, [["Nr", "X"], ["1A1", "x"]])

    clean = ei.check([SPEC], folder, lambda _args: json.dumps([{"nr": "1A1", "marker": "x"}]))

    assert clean
    assert "OK     raw_t: 1 rows" in capsys.readouterr().out


def test_check_lists_the_rows_that_differ(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    folder = workbook_for(tmp_path, [["Nr", "X"], ["1A1", "x"], ["1A2", "x"]])

    clean = ei.check(
        [SPEC],
        folder,
        lambda _args: json.dumps([{"nr": "1A1", "marker": "x"}, {"nr": "OLD", "marker": "x"}]),
    )

    out = capsys.readouterr().out
    assert not clean
    assert "DIFFER raw_t: 1 row(s) only in the workbook, 1 only in the table" in out
    assert "workbook only: ['1A2', 'x']" in out
    assert "table only: ['OLD', 'x']" in out


def test_main_reports_a_missing_workbook_and_an_unknown_table(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    assert ei.main(["check", "--inputs-dir", str(tmp_path)]) == 2
    assert "not found" in capsys.readouterr().err
    assert ei.main(["check", "--only", "nope", "--inputs-dir", str(tmp_path)]) == 2
    assert "unknown table" in capsys.readouterr().err


# ---------------------------------------------------------------------------------------------
# The real workbooks
# ---------------------------------------------------------------------------------------------


@pytest.mark.skipif(
    not (REAL_INPUTS / "reference_data.xlsx").exists(), reason="workbooks not in this checkout"
)
def test_the_real_workbooks_parse_to_the_expected_tables() -> None:
    counts = {spec.table: len(ei.workbook_table_rows(spec, REAL_INPUTS)) for spec in ei.INPUTS}

    assert counts == {
        "raw_entity_mapping": 6,
        "raw_charts_of_accounts": 113,
        "raw_cost_products": 68,
    }
    entities = ei.workbook_table_rows(ei.INPUTS[0], REAL_INPUTS)
    assert ("1665", "ACHEM", "Aquatiq Chemistry AS", "01", "Chemistry", "Aquatiq") in entities
