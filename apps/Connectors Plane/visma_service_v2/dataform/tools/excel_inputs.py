#!/usr/bin/env python3
"""Load the finance workbooks (the Excel inputs) into visma_bronze_excel, or check that the tables match.

The workbooks in dataform/inputs/excel/ carry business logic the warehouse follows: which entity belongs
to which business unit and company id (reference_data.xlsx), the group chart of accounts
(reference_data.xlsx), and the product numbers that are charges such as freight, travel and packaging
fees and not goods (Omkostninger.xlsx). Dataform reads them from the raw_* tables of visma_bronze_excel.

  check   compare every table with its workbook sheet, row by row; exit 1 on any difference
  load    replace every table with the content of its sheet

Edit the workbook, run `load`, then run Dataform. `check` is what to run when in doubt: it also says
which rows differ. Nothing here guesses: a sheet whose header row is not the expected one stops the
command, so a column added to a workbook is not silently dropped.

No spreadsheet library: an .xlsx file is a zip of XML and these sheets are plain cells (some other
workbooks of the group carry styles that openpyxl refuses). Uses the `bq` command and the gcloud login of
whoever runs it.

Usage: dataform/tools/excel_inputs.py {check,load} [--only raw_cost_products] [--inputs-dir DIR]
"""

from __future__ import annotations

import argparse
import csv
import json
import re
import subprocess
import sys
import tempfile
import zipfile
from collections import Counter
from collections.abc import Callable, Sequence
from dataclasses import dataclass
from pathlib import Path
from xml.etree import ElementTree

PROJECT, LOCATION, DATASET = "aquatiq-datavarehus", "europe-north2", "visma_bronze_excel"
INPUTS_DIR = Path(__file__).resolve().parent.parent / "inputs" / "excel"
NULL_MARKER = r"\N"

NS = {
    "m": "http://schemas.openxmlformats.org/spreadsheetml/2006/main",
    "r": "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
    "rel": "http://schemas.openxmlformats.org/package/2006/relationships",
}

Row = tuple[str | None, ...]


class ExcelInputError(RuntimeError):
    """A workbook or table is not what the loader expects."""


@dataclass(frozen=True)
class ExcelInput:
    workbook: str
    sheet: str
    table: str
    header: tuple[str, ...]  # the sheet's header row, as written in the workbook
    columns: tuple[str, ...]  # the table's columns, same order; all STRING


INPUTS: tuple[ExcelInput, ...] = (
    ExcelInput(
        workbook="reference_data.xlsx",
        sheet="EntityMapping",
        table="raw_entity_mapping",
        header=(
            "company_id",
            "entity_code",
            "entity_name",
            "business_unit_code",
            "business_unit_name",
            "group_name",
        ),
        columns=(
            "company_id",
            "entity_code",
            "entity_name",
            "business_unit_code",
            "business_unit_name",
            "group_name",
        ),
    ),
    ExcelInput(
        workbook="reference_data.xlsx",
        sheet="ChartOfAccounts",
        table="raw_charts_of_accounts",
        header=(
            "bs_pl",
            "fsli_1",
            "fsli_2",
            "fsli_3",
            "ebitda_flag",
            "net_income_flag",
            "cf1",
            "cf2",
            "cf3",
            "nwc",
            "capex",
            "intercompany_flag",
            "account_code_1",
            "account_code_2",
            "account_code_3",
        ),
        columns=(
            "bs_pl",
            "fsli_1",
            "fsli_2",
            "fsli_3",
            "ebitda_flag",
            "net_income_flag",
            "cf1",
            "cf2",
            "cf3",
            "nwc",
            "capex",
            "intercompany_flag",
            "account_code_1",
            "account_code_2",
            "account_code_3",
        ),
    ),
    ExcelInput(
        workbook="Omkostninger.xlsx",
        sheet="Sheet1",
        table="raw_cost_products",
        header=("Produktnummer", "X"),
        columns=("product_number", "marker"),
    ),
)


# ---------------------------------------------------------------------------------------------
# Reading a workbook
# ---------------------------------------------------------------------------------------------


def _text(element: ElementTree.Element | None) -> str:
    """All text under a shared string or inline string, rich-text runs joined."""
    if element is None:
        return ""
    return "".join(t.text or "" for t in element.iterfind(".//m:t", NS))


def _shared_strings(book: zipfile.ZipFile) -> list[str]:
    if "xl/sharedStrings.xml" not in book.namelist():
        return []
    root = ElementTree.fromstring(book.read("xl/sharedStrings.xml"))
    return [_text(si) for si in root.iterfind("m:si", NS)]


def _sheet_path(book: zipfile.ZipFile, sheet: str) -> str:
    workbook = ElementTree.fromstring(book.read("xl/workbook.xml"))
    relations = ElementTree.fromstring(book.read("xl/_rels/workbook.xml.rels"))
    targets = {
        rel.get("Id"): rel.get("Target", "") for rel in relations.iterfind("rel:Relationship", NS)
    }
    names = []
    for element in workbook.iterfind("m:sheets/m:sheet", NS):
        names.append(element.get("name"))
        if element.get("name") == sheet:
            target = targets[element.get(f"{{{NS['r']}}}id")]
            return target.lstrip("/") if target.startswith("/") else f"xl/{target}"
    raise ExcelInputError(f"sheet {sheet!r} not found; the workbook has {names}")


def _column_index(reference: str) -> int:
    letters = re.match(r"[A-Z]+", reference)
    if letters is None:
        raise ExcelInputError(f"cell reference {reference!r} has no column")
    index = 0
    for letter in letters.group():
        index = index * 26 + (ord(letter) - ord("A") + 1)
    return index - 1


def _cell_value(cell: ElementTree.Element, shared: list[str]) -> str | None:
    kind = cell.get("t")
    value = cell.find("m:v", NS)
    if kind == "inlineStr":
        text = _text(cell.find("m:is", NS))
    elif value is None or value.text is None:
        return None
    elif kind == "s":
        text = shared[int(value.text)]
    elif kind == "b":
        text = "1" if value.text == "1" else "0"
    elif kind == "e":
        return None  # an error value such as #N/A is no data
    else:
        text = re.sub(r"^(-?\d+)\.0+$", r"\1", value.text)  # 3000.0 is 3000
    text = text.strip()
    return text or None


def read_sheet(path: Path, sheet: str) -> list[list[str | None]]:
    """Every row of a sheet as a list of cell texts (None for an empty cell), gaps filled."""
    with zipfile.ZipFile(path) as book:
        shared = _shared_strings(book)
        root = ElementTree.fromstring(book.read(_sheet_path(book, sheet)))
    rows: list[list[str | None]] = []
    for row in root.iterfind("m:sheetData/m:row", NS):
        cells = {
            _column_index(c.get("r", "")): _cell_value(c, shared) for c in row.iterfind("m:c", NS)
        }
        rows.append([cells.get(i) for i in range(max(cells) + 1)] if cells else [])
    return rows


def table_rows(spec: ExcelInput, rows: Sequence[Sequence[str | None]]) -> list[Row]:
    """The data rows of a sheet as table rows. Stops if the sheet is not laid out as expected."""
    if not rows:
        raise ExcelInputError(f"{spec.workbook} {spec.sheet}: the sheet is empty")
    width = len(spec.columns)
    found = tuple(cell or "" for cell in rows[0][:width])
    if found != spec.header:
        raise ExcelInputError(
            f"{spec.workbook} {spec.sheet}: expected the header {list(spec.header)}, found {list(found)}"
        )
    if any(cell for cell in rows[0][width:]):
        raise ExcelInputError(
            f"{spec.workbook} {spec.sheet}: the header has columns beyond {width}"
        )
    data: list[Row] = []
    for number, row in enumerate(rows[1:], start=2):
        if any(cell for cell in row[width:]):
            raise ExcelInputError(
                f"{spec.workbook} {spec.sheet} row {number}: a value beyond column {width}; "
                "add the column to excel_inputs.py and to the table"
            )
        cells = tuple((list(row) + [None] * width)[:width])
        if any(cells):
            data.append(cells)
    return data


# ---------------------------------------------------------------------------------------------
# BigQuery (through the bq command)
# ---------------------------------------------------------------------------------------------

Runner = Callable[[list[str]], str]


def run_bq(args: list[str]) -> str:
    done = subprocess.run(["bq", *args], capture_output=True, text=True, check=False)
    if done.returncode != 0:
        raise ExcelInputError(
            f"bq {' '.join(args[:4])}: {done.stderr.strip() or done.stdout.strip()}"
        )
    return done.stdout


def read_table(spec: ExcelInput, run: Runner = run_bq) -> list[Row]:
    columns = ", ".join(f"`{c}`" for c in spec.columns)
    sql = f"SELECT {columns} FROM `{PROJECT}.{DATASET}.{spec.table}`"
    out = run(
        [
            f"--project_id={PROJECT}",
            f"--location={LOCATION}",
            "query",
            "--use_legacy_sql=false",
            "--format=json",
            "--max_rows=100000",
            sql,
        ]
    )
    return [tuple(record.get(c) for c in spec.columns) for record in json.loads(out or "[]")]


def load_table(spec: ExcelInput, rows: Sequence[Row], run: Runner = run_bq) -> None:
    schema = ",".join(f"{c}:STRING" for c in spec.columns)
    with tempfile.TemporaryDirectory() as folder:
        path = Path(folder) / f"{spec.table}.csv"
        with path.open("w", newline="", encoding="utf-8") as handle:
            writer = csv.writer(handle)
            writer.writerow(spec.columns)
            writer.writerows(
                [NULL_MARKER if cell is None else cell for cell in row] for row in rows
            )
        run(
            [
                f"--project_id={PROJECT}",
                f"--location={LOCATION}",
                "load",
                "--replace",
                "--source_format=CSV",
                "--skip_leading_rows=1",
                f"--null_marker={NULL_MARKER}",
                f"{PROJECT}:{DATASET}.{spec.table}",
                str(path),
                schema,
            ]
        )


# ---------------------------------------------------------------------------------------------
# Commands
# ---------------------------------------------------------------------------------------------


def difference(workbook_rows: Sequence[Row], table: Sequence[Row]) -> tuple[list[Row], list[Row]]:
    """(rows only in the workbook, rows only in the table), as multisets so duplicates count."""
    wanted, have = Counter(workbook_rows), Counter(table)
    return sorted((wanted - have).elements(), key=str), sorted((have - wanted).elements(), key=str)


def selected(only: str | None) -> list[ExcelInput]:
    if not only:
        return list(INPUTS)
    names = {name.strip() for name in only.split(",")}
    unknown = names - {spec.table for spec in INPUTS}
    if unknown:
        raise ExcelInputError(
            f"unknown table(s) {sorted(unknown)}; known: {[s.table for s in INPUTS]}"
        )
    return [spec for spec in INPUTS if spec.table in names]


def workbook_table_rows(spec: ExcelInput, inputs_dir: Path) -> list[Row]:
    path = inputs_dir / spec.workbook
    if not path.exists():
        raise ExcelInputError(f"{path} not found")
    return table_rows(spec, read_sheet(path, spec.sheet))


def check(specs: Sequence[ExcelInput], inputs_dir: Path, run: Runner = run_bq) -> bool:
    """Print the state of every table against its workbook. True when all match."""
    clean = True
    for spec in specs:
        wanted = workbook_table_rows(spec, inputs_dir)
        missing, extra = difference(wanted, read_table(spec, run))
        if not missing and not extra:
            print(f"OK     {spec.table}: {len(wanted)} rows equal {spec.workbook} / {spec.sheet}")
            continue
        clean = False
        print(
            f"DIFFER {spec.table}: {len(missing)} row(s) only in the workbook, {len(extra)} only in the table"
        )
        for label, rows in (("workbook only", missing), ("table only", extra)):
            for row in rows[:10]:
                print(f"         {label}: {list(row)}")
    return clean


def load(specs: Sequence[ExcelInput], inputs_dir: Path, run: Runner = run_bq) -> None:
    for spec in specs:
        rows = workbook_table_rows(spec, inputs_dir)
        load_table(spec, rows, run)
        print(f"loaded {spec.table}: {len(rows)} rows from {spec.workbook} / {spec.sheet}")


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("command", choices=("check", "load"))
    parser.add_argument("--only", help="comma-separated table names, default all")
    parser.add_argument("--inputs-dir", type=Path, default=INPUTS_DIR)
    args = parser.parse_args(argv)
    try:
        specs = selected(args.only)
        if args.command == "load":
            load(specs, args.inputs_dir)
            return 0
        return 0 if check(specs, args.inputs_dir) else 1
    except ExcelInputError as error:
        print(f"error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
