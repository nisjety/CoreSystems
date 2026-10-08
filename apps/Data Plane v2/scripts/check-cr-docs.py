"""Documents lint for contextual retrieval (design task CR-T16).

The contextual retrieval decisions live in prose: ownership rows and a rule in the
master ownership matrix, four entries in the Model Plane decision ledger, a
runbook, evidence documents and a corrected gap table. Prose drifts silently, and
the 2026-10-03 audit found it had already drifted (a gap row that said "closed"
for a path that does not exist, a ZDR note about a knob that was added months
ago, comments that called a feature off by default while it was running).

This lint asserts the shape of those documents, not their wording. It checks:

  * the matrix rows, rule 16 and the two amended rows are present, and the rules
    are numbered without a gap;
  * the four ledger entries exist with a valid state, above the older entries;
  * the runbook exists, says what state it is in, and covers the eight incident
    procedures, and never tells an operator to run the Quickwit rebuild script
    without saying it must not clear (the script clears by default);
  * the runbook's status table and error-class list are the ones the CHECK constraints
    of the provenance migration accept (the migration is the authority; a list that
    drifts sends an operator to a status the table refuses);
  * no data-protection or Data Plane document says contextual retrieval is off
    by default, or names a column that does not exist;
  * the Model Plane ZDR note does not say the Anthropic attestation knob is
    missing, the gap table does not call the cost ledger closed, and the honest
    limits do not tie a limit to a metering change that does not apply to it;
  * every checked file has one line-ending style and no NUL bytes.

Run from anywhere:

    python "apps/Data Plane v2/scripts/check-cr-docs.py"            # the checks
    python "apps/Data Plane v2/scripts/check-cr-docs.py" --selftest  # prove each check can fail
    python "apps/Data Plane v2/scripts/check-cr-docs.py" --citations # also run the citation checker

Exit status: 0 all good, 1 at least one violation, 2 usage error.
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]

MATRIX = "apps/master-ownership-matrix.md"
LEDGER = "apps/Model Plane/docs/decisions/ledger.md"
ZDR_NOTE = "apps/Model Plane/docs/ZDR.md"
GAP_DATA = "apps/Data Plane v2/docs/gap-data.md"
RUNBOOK = "apps/Data Plane v2/docs/contextual-retrieval-runbook.md"
HONEST_LIMITS = "docs/compliance/evidence/honest-limits.md"
CODEBASE = "apps/CODEBASE_INFORMATION_SYSTEM.md"
MIGRATION = "apps/Data Plane v2/infra/postgres/migrations/20261003130000_context_provenance.sql"

EVIDENCE_DOCS = [
    "docs/compliance/evidence/data-flow-map.md",
    "docs/compliance/evidence/honest-limits.md",
    "docs/compliance/evidence/subprocessors-from-code.md",
    "docs/compliance/evidence/claims-vs-code.md",
]

# Documents scanned for the stale statements. The ledger is scanned only in its
# newest block (see LEDGER_TOP_LINES): older entries are history and are not edited.
STALE_SCAN = [
    MATRIX,
    CODEBASE,
    ZDR_NOTE,
    GAP_DATA,
    RUNBOOK,
    "apps/Data Plane v2/DATA_PLANE_STATUS.md",
    "apps/Data Plane v2/docs/retrieval-fusion-evidence-2026-08-26.md",
    *EVIDENCE_DOCS,
]

# Files whose line endings are checked (the touched ones that exist).
EOL_FILES = [
    MATRIX,
    CODEBASE,
    LEDGER,
    ZDR_NOTE,
    GAP_DATA,
    RUNBOOK,
    "apps/Data Plane v2/DATA_PLANE_STATUS.md",
    "apps/Data Plane v2/docs/retrieval-fusion-evidence-2026-08-26.md",
    "apps/Data Plane v2/scripts/check-cr-docs.py",
    *EVIDENCE_DOCS,
    "docs/compliance/evidence/README.md",
    "docs/compliance/evidence/live-reads-contextual-retrieval-2026-10-06.txt",
]

LEDGER_STATES = ("proposed", "implemented", "deferred", "rejected", "archived")
LEDGER_TOP_LINES = 700

# One key phrase per ledger entry; each must appear in a `## date - title` heading.
LEDGER_TITLES = {
    "contextual retrieval stays on": "decision 1: always on, bounded inline plus a reconciler",
    "tenant retention posture": "decision 2: the org posture reaches background work",
    "background provider metering": "decision 3: metering architecture and the rejected alternatives",
    "graph extraction": "decision 4: the graph extraction state machine",
}

RUNBOOK_SECTIONS = {
    "429 storm": r"429|rate.?limit",
    "stuck backlog": r"stuck backlog",
    "ZDR org report": r"zdr org|org reports? zdr|zdr report",
    "prompt rollout and rollback": r"prompt rollout",
    "budget exhausted": r"budget exhausted",
    "kill switch and purge": r"kill switch",
    "Quickwit parity repair": r"quickwit parity",
    "cost spike triage": r"cost spike",
}


class Report:
    def __init__(self) -> None:
        self.violations: list[str] = []

    def add(self, where: str, what: str) -> None:
        self.violations.append(f"{where}: {what}")


def read_text(root: Path, rel: str) -> str | None:
    path = root / rel
    if not path.is_file():
        return None
    return path.read_bytes().decode("utf-8", errors="replace")


def lines_of(text: str) -> list[str]:
    return text.replace("\r\n", "\n").split("\n")


# ---------------------------------------------------------------- matrix


def check_matrix(text: str | None, report: Report) -> None:
    if text is None:
        report.add(MATRIX, "file is missing")
        return
    rows = [line for line in lines_of(text) if line.startswith("|")]
    for needle in ("Contextual retrieval", "Background provider metering"):
        if not any(re.match(r"\|\s*" + re.escape(needle), row) for row in rows):
            report.add(MATRIX, f"no ownership row starts with '{needle}'")
    embedding = next((r for r in rows if re.match(r"\|\s*Embedding generation\b", r)), None)
    if embedding is None or "contextual retrieval" not in embedding.lower():
        report.add(MATRIX, "the 'Embedding generation' row does not mention contextual retrieval")
    cost = next((r for r in rows if re.match(r"\|\s*Cost ledger", r)), None)
    if cost is None or "rule 16" not in cost.lower() or "cost-core" not in cost.lower():
        report.add(MATRIX, "the 'Cost ledger / budgets' row does not say the Data Plane's spend reaches cost-core under rule 16")

    numbers = [int(m.group(1)) for line in lines_of(text) if (m := re.match(r"^(\d+)\. \*\*", line))]
    if not numbers:
        report.add(MATRIX, "no numbered rules found")
        return
    if numbers != list(range(1, len(numbers) + 1)):
        report.add(MATRIX, f"rule numbers are not 1..N without a gap: {numbers}")
    rule16 = next((line for line in lines_of(text) if re.match(r"^16\. \*\*", line)), None)
    if rule16 is None:
        report.add(MATRIX, "rule 16 is missing")
    else:
        lowered = rule16.lower()
        for phrase in ("retention posture", "metered", "never lowered"):
            if phrase not in lowered:
                report.add(MATRIX, f"rule 16 does not contain '{phrase}'")


# ---------------------------------------------------------------- ledger


def check_ledger(text: str | None, report: Report) -> None:
    if text is None:
        report.add(LEDGER, "file is missing")
        return
    lines = lines_of(text)
    headings = [(i, line) for i, line in enumerate(lines) if re.match(r"^## \d{4}-\d{2}-\d{2} ", line)]
    top = [(i, line) for i, line in headings if i < LEDGER_TOP_LINES * 2]
    for phrase, label in LEDGER_TITLES.items():
        hit = next(((i, line) for i, line in top if phrase in line.lower()), None)
        if hit is None:
            report.add(LEDGER, f"no entry heading contains '{phrase}' ({label})")
            continue
        index = hit[0]
        window = "\n".join(lines[index + 1 : index + 6])
        state = re.search(r"\*\*State:\*\*\s*`(\w+)`", window)
        if state is None or state.group(1) not in LEDGER_STATES:
            report.add(LEDGER, f"entry '{phrase}' has no valid '**State:** `...`' line within five lines of its heading")
    # The four entries are newer than the older history: they sit above the first 2026-09 entry.
    older = next((i for i, line in headings if re.match(r"^## 2026-0[0-9]-", line)), None)
    if older is not None:
        for phrase in LEDGER_TITLES:
            hit = next((i for i, line in headings if phrase in line.lower()), None)
            if hit is not None and hit > older:
                report.add(LEDGER, f"entry '{phrase}' sits below older entries (newest first)")


# ---------------------------------------------------------------- runbook


def check_runbook(text: str | None, report: Report) -> None:
    if text is None:
        report.add(RUNBOOK, "file is missing")
        return
    lines = lines_of(text)
    head = "\n".join(lines[:15]).lower()
    if "status:" not in head:
        report.add(RUNBOOK, "the first 15 lines do not state a Status")
    if "not legal advice" in head:
        pass
    headings = [line.lower() for line in lines if re.match(r"^#{2,3} ", line)]
    for label, pattern in RUNBOOK_SECTIONS.items():
        if not any(re.search(pattern, heading) for heading in headings):
            report.add(RUNBOOK, f"no heading for the procedure '{label}'")
    for number, line in enumerate(lines, 1):
        if "rebuild-quickwit" in line:
            lowered = line.lower()
            if not ("--no-clear" in lowered or "never" in lowered or "do not" in lowered or "not run" in lowered):
                report.add(RUNBOOK, f"line {number} mentions rebuild-quickwit without saying it must not clear")
    quickwit = "\n".join(lines).lower()
    if "documents.indexed" not in quickwit:
        report.add(RUNBOOK, "the Quickwit parity repair does not name documents.indexed")


def sql_check_values(sql: str, constraint: str) -> set[str] | None:
    """The quoted values of the IN (...) list of `ADD CONSTRAINT <constraint> CHECK (...)`, or None."""
    match = re.search(r"ADD CONSTRAINT\s+" + re.escape(constraint) + r"\b(.*?)\);", sql, flags=re.S)
    if match is None:
        return None
    listed = re.search(r"\bIN\s*\((.*?)\)", match.group(1), flags=re.S)
    if listed is None:
        return None
    return set(re.findall(r"'([a-z_]+)'", listed.group(1)))


def check_runbook_vocabulary(runbook: str | None, migration: str | None, report: Report) -> None:
    """The runbook's status and error-class vocabulary is the migration's, not a copy that can drift.

    The CHECK constraints of the provenance migration are the authority for what `context_status`
    and `context_error_class` may hold. A runbook that lists a class the table refuses, or omits
    one it accepts, sends an operator to a query that returns nothing or to a status that cannot
    be written. The status table must have one row per accepted status and no other row (a status
    named only in prose does not count); the paragraph that opens with "Error
    classes" must list exactly the accepted classes.
    """
    if runbook is None:
        return
    if migration is None:
        report.add(RUNBOOK, f"names a migration that is not in the tree: {MIGRATION}")
        return
    statuses = sql_check_values(migration, "knowledge_units_context_status_chk")
    classes = sql_check_values(migration, "knowledge_units_context_error_class_chk")
    if statuses is None or classes is None:
        report.add(MIGRATION, "the status or error-class CHECK constraint could not be read (renamed?)")
        return
    table = set(re.findall(r"^\|\s*`([a-z_]+)`\s*\|", runbook, flags=re.M))
    for status in sorted(table - statuses):
        report.add(RUNBOOK, f"the status table has a row for `{status}`, which the migration refuses")
    for status in sorted(statuses - table):
        report.add(RUNBOOK, f"the status table has no row for `{status}`, which the migration accepts")
    paragraph = next((p for _, p in paragraphs(runbook) if p.startswith("Error classes")), None)
    if paragraph is None:
        report.add(RUNBOOK, "has no paragraph that starts with 'Error classes'")
        return
    listed = set(re.findall(r"`([a-z_]+)`", paragraph))
    for name in sorted(classes - listed):
        report.add(RUNBOOK, f"the error-class paragraph omits `{name}`, which the migration accepts")
    for name in sorted(listed - classes):
        report.add(RUNBOOK, f"the error-class paragraph lists `{name}`, which the migration refuses")


# ---------------------------------------------------------------- stale statements


def paragraphs(text: str) -> list[tuple[int, str]]:
    """Units of prose: a paragraph, a list item or a table row, each with its first line number.

    A bullet list or a table has no blank lines, so splitting on blank lines alone would pair
    sentences that have nothing to do with each other (a flag that is off by default in one item
    and contextual retrieval in the next).
    """
    out: list[tuple[int, str]] = []
    start, buffer = 1, []

    def flush() -> None:
        nonlocal buffer
        if buffer:
            out.append((start, "\n".join(buffer)))
            buffer = []

    for number, line in enumerate(lines_of(text), 1):
        starts_unit = bool(re.match(r"\s*(?:[-*+] |\d+\. |\|)", line))
        if line.strip() == "":
            flush()
        elif starts_unit:
            flush()
            start = number
            buffer.append(line)
        else:
            if not buffer:
                start = number
            buffer.append(line)
    flush()
    return out


def check_stale(name: str, text: str | None, report: Report, top_lines: int | None = None) -> None:
    if text is None:
        return
    if top_lines is not None:
        text = "\n".join(lines_of(text)[:top_lines])
    for number, line in enumerate(lines_of(text), 1):
        if "contextualized_text" in line:
            report.add(name, f"line {number} names a column that does not exist (the column is chunk_context)")
    for start, para in paragraphs(text):
        lowered = para.lower()
        if "contextual" in lowered and re.search(r"off[ -]by[ -]default", lowered):
            report.add(name, f"line {start}: a paragraph about contextual retrieval says it is off by default")


# ---------------------------------------------------------------- targeted corrections


def check_zdr_note(text: str | None, report: Report) -> None:
    if text is None:
        report.add(ZDR_NOTE, "file is missing")
        return
    lowered = text.lower()
    for phrase in ("with no knob at all", "add the anthropic attestation knob", "the anthropic knob, step 2"):
        if phrase in lowered:
            report.add(ZDR_NOTE, f"still says the Anthropic attestation knob is missing ('{phrase}')")
    if "AZURE_ANTHROPIC_ZDR_" not in text:
        report.add(ZDR_NOTE, "does not name the AZURE_ANTHROPIC_ZDR_* attestation fields")


def check_gap_data(text: str | None, report: Report) -> None:
    if text is None:
        report.add(GAP_DATA, "file is missing")
        return
    row = next((line for line in lines_of(text) if line.startswith("| DATA-15 ")), None)
    if row is None:
        report.add(GAP_DATA, "DATA-15 row is missing")
        return
    cells = [c.strip() for c in row.strip().strip("|").split("|")]
    status = cells[4] if len(cells) > 4 else ""
    if re.match(r"\*{0,2}\s*CLOSED", status.strip().upper()):
        report.add(GAP_DATA, "DATA-15 is still marked CLOSED although no usage event reaches cost-core")
    if "cost-core" not in row.lower():
        report.add(GAP_DATA, "DATA-15 does not say where usage does and does not reach")


def check_honest_limits(text: str | None, report: Report) -> None:
    if text is None:
        report.add(HONEST_LIMITS, "file is missing")
        return
    lowered = text.lower()
    if "once metering lands" in lowered or "changes once metering" in lowered:
        report.add(HONEST_LIMITS, "ties a limit to metering landing (L19 is about the ChatGPT-account route)")
    l19 = next((line for line in lines_of(text) if line.startswith("- **L19.")), None)
    if l19 is None or "that route" not in l19.lower():
        report.add(HONEST_LIMITS, "L19 is missing or no longer about the ChatGPT-account route")
    for limit in ("L33", "L34", "L35"):
        if not re.search(r"^- \*\*" + limit + r"\.", text, flags=re.M):
            report.add(HONEST_LIMITS, f"new limit {limit} is missing")


# ---------------------------------------------------------------- line endings


def check_eol(name: str, raw: bytes | None, report: Report) -> None:
    if raw is None:
        return
    if b"\x00" in raw:
        report.add(name, "contains NUL bytes (a zero-filled file after an abrupt shutdown)")
    crlf = raw.count(b"\r\n")
    bare_lf = raw.count(b"\n") - crlf
    if crlf and bare_lf:
        report.add(name, f"mixed line endings: {crlf} CRLF and {bare_lf} bare LF")


# ---------------------------------------------------------------- driver


def run_checks(root: Path) -> Report:
    report = Report()
    check_matrix(read_text(root, MATRIX), report)
    check_ledger(read_text(root, LEDGER), report)
    check_runbook(read_text(root, RUNBOOK), report)
    check_runbook_vocabulary(read_text(root, RUNBOOK), read_text(root, MIGRATION), report)
    for rel in STALE_SCAN:
        check_stale(rel, read_text(root, rel), report)
    check_stale(LEDGER, read_text(root, LEDGER), report, top_lines=LEDGER_TOP_LINES * 2)
    check_zdr_note(read_text(root, ZDR_NOTE), report)
    check_gap_data(read_text(root, GAP_DATA), report)
    check_honest_limits(read_text(root, HONEST_LIMITS), report)
    for rel in EOL_FILES:
        path = root / rel
        check_eol(rel, path.read_bytes() if path.is_file() else None, report)
    return report


def run_citations(root: Path) -> int:
    tool = root / "docs/compliance/evidence/tools/check-citations.mjs"
    if not tool.is_file():
        print("citation checker not found", file=sys.stderr)
        return 1
    status = 0
    for rel in EVIDENCE_DOCS:
        result = subprocess.run(["node", str(tool), str(root / rel)], capture_output=True, text=True)
        sys.stdout.write(result.stdout)
        sys.stderr.write(result.stderr)
        status = status or result.returncode
    return status


# ---------------------------------------------------------------- self test


def selftest() -> int:
    """Each check must be able to fail: feed it a document that breaks exactly its rule."""
    failures: list[str] = []

    def expect(label: str, fn, text, needle: str) -> None:
        report = Report()
        fn(text, report)
        if not any(needle in v for v in report.violations):
            failures.append(f"{label}: expected a violation containing '{needle}', got {report.violations}")

    def expect_clean(label: str, fn, text) -> None:
        report = Report()
        fn(text, report)
        if report.violations:
            failures.append(f"{label}: expected no violation, got {report.violations}")

    good_matrix = "\n".join(
        [
            "| Embedding generation | contextual retrieval (next row) |",
            "| Contextual retrieval (x) | a |",
            "| Cost ledger / budgets | cost-core under rule 16 |",
            "| Background provider metering (x) | a |",
            "1. **A.** a",
            "2. **B.** b",
            "16. **Retention posture.** metered; ZDR is never lowered organisation-wide",
        ]
    )
    # Rule numbers must be contiguous, so the clean case has all 16 rules.
    good_matrix = good_matrix.replace("2. **B.** b\n16.", "\n".join(f"{n}. **R{n}.** r" for n in range(2, 16)) + "\n16.")
    expect_clean("matrix clean", lambda t, r: check_matrix(t, r), good_matrix)
    expect("matrix rule gap", check_matrix, good_matrix.replace("5. **R5.** r\n", ""), "not 1..N without a gap")
    expect("matrix no rule 16", check_matrix, good_matrix.replace("16. **Retention", "17. **Retention"), "rule 16")
    expect("matrix no row", check_matrix, good_matrix.replace("| Contextual retrieval (x)", "| Something else"), "Contextual retrieval")

    good_ledger = "\n".join(
        [
            "## 2026-10-03 — Contextual retrieval stays on",
            "",
            "**State:** `proposed`",
            "## 2026-10-03 — The tenant retention posture",
            "**State:** `proposed`",
            "## 2026-10-03 — Background provider metering",
            "**State:** `proposed`",
            "## 2026-10-03 — Graph extraction state machine",
            "**State:** `proposed`",
            "## 2026-09-03 — older",
        ]
    )
    expect_clean("ledger clean", check_ledger, good_ledger)
    expect("ledger missing", check_ledger, good_ledger.replace("Graph extraction", "Other"), "graph extraction")
    expect("ledger bad state", check_ledger, good_ledger.replace("`proposed`", "`maybe`", 1), "valid '**State:**")
    expect(
        "ledger order",
        check_ledger,
        "## 2026-09-03 — older\n" + good_ledger.replace("## 2026-09-03 — older", ""),
        "newest first",
    )

    good_runbook = "\n".join(
        [
            "Status: proposed",
            "",
            "## 1. Provider 429 storm",
            "## 2. Stuck backlog",
            "## 3. ZDR org report",
            "## 4. Prompt rollout and rollback",
            "## 5. Budget exhausted",
            "## 6. Kill switch and purge",
            "## 7. Quickwit parity repair",
            "Re-announce documents.indexed. Do not run rebuild-quickwit.sh.",
            "## 8. Cost spike triage",
        ]
    )
    expect_clean("runbook clean", check_runbook, good_runbook)
    expect("runbook missing section", check_runbook, good_runbook.replace("## 5. Budget exhausted", ""), "budget exhausted")
    expect("runbook unqualified rebuild", check_runbook, good_runbook + "\nRun scripts/rebuild-quickwit.sh", "rebuild-quickwit")
    expect("runbook no status", check_runbook, good_runbook.replace("Status: proposed", "x"), "Status")

    good_sql = (
        "ALTER TABLE t ADD CONSTRAINT knowledge_units_context_status_chk CHECK (\n"
        "    context_status IS NULL OR context_status IN (\n        'pending', 'ready'\n    )\n);\n"
        "ALTER TABLE t ADD CONSTRAINT knowledge_units_context_error_class_chk CHECK (\n"
        "    context_error_class IS NULL OR context_error_class IN (\n        'timeout', 'internal'\n    )\n);\n"
    )
    good_vocab = "| `pending` | x |\n| `ready` | y |\n\nError classes are a closed set: `timeout`, `internal`.\n"

    def vocab(runbook: str | None, sql: str | None, report: Report) -> None:
        check_runbook_vocabulary(runbook, sql, report)

    expect_clean("vocabulary clean", lambda t, r: vocab(t, good_sql, r), good_vocab)
    expect("vocabulary omitted class", lambda t, r: vocab(t, good_sql, r), good_vocab.replace("`internal`", ""), "omits `internal`")
    expect(
        "vocabulary refused class",
        lambda t, r: vocab(t, good_sql, r),
        good_vocab.replace("`internal`.", "`internal`, `bogus`."),
        "refuses",
    )
    expect("vocabulary unnamed status", lambda t, r: vocab(t, good_sql, r), good_vocab.replace("`ready`", "ready"), "no row for `ready`")
    # A status that is only mentioned in prose is not a row of the table: the check reads the table.
    expect(
        "vocabulary status only in prose",
        lambda t, r: vocab(t, good_sql, r),
        good_vocab.replace("| `ready` | y |", "| ready | y |") + "\nThe unit becomes `ready` later.\n",
        "no row for `ready`",
    )
    expect(
        "vocabulary refused status row",
        lambda t, r: vocab(t, good_sql, r),
        good_vocab.replace("| `ready` | y |", "| `ready` | y |\n| `bogus` | z |"),
        "row for `bogus`",
    )
    expect("vocabulary no migration", lambda t, r: vocab(t, None, r), good_vocab, "not in the tree")
    expect("vocabulary renamed constraint", lambda t, r: vocab(t, good_sql.replace("_status_chk", "_state_chk"), r), good_vocab, "could not be read")

    stale_report = Report()
    check_stale("x.md", "Contextual retrieval is OFF by default here.\n\nOther.", stale_report)
    check_stale("y.md", "Requires knowledge_units.contextualized_text.", stale_report)
    if len(stale_report.violations) != 2:
        failures.append(f"stale scan: expected 2 violations, got {stale_report.violations}")
    clean_report = Report()
    check_stale("z.md", "Video captions are off by default.\n\nContextual retrieval is on.", clean_report)
    if clean_report.violations:
        failures.append(f"stale scan: false positive {clean_report.violations}")

    expect("zdr note stale", check_zdr_note, "azure-anthropic ... with no knob at all", "knob")
    expect_clean("zdr note clean", check_zdr_note, "AZURE_ANTHROPIC_ZDR_DIGEST is the evidence field")
    expect(
        "gap closed",
        check_gap_data,
        "| DATA-15 | x | absent | usage events to cost-core | **CLOSED** | y |",
        "CLOSED",
    )
    expect_clean(
        "gap partial",
        check_gap_data,
        "| DATA-15 | x | absent | usage events to cost-core | **PARTIAL** | does not reach cost-core |",
    )
    expect("limits metering", check_honest_limits, "x once metering lands", "metering landing")

    eol_report = Report()
    check_eol("m.md", b"a\r\nb\nc\r\n", eol_report)
    check_eol("n.md", b"a\x00b\n", eol_report)
    check_eol("ok.md", b"a\r\nb\r\n", eol_report)
    if len(eol_report.violations) != 2:
        failures.append(f"eol: expected 2 violations, got {eol_report.violations}")

    if failures:
        for failure in failures:
            print("SELFTEST FAIL:", failure)
        return 1
    print("selftest: every check fails on a document that breaks its rule, and passes on a clean one")
    return 0


def main(argv: list[str]) -> int:
    args = argv[1:]
    if any(a not in ("--selftest", "--citations") for a in args):
        print(__doc__)
        return 2
    if "--selftest" in args:
        return selftest()
    report = run_checks(ROOT)
    for violation in report.violations:
        print("VIOLATION", violation)
    status = 1 if report.violations else 0
    if "--citations" in args:
        status = run_citations(ROOT) or status
    print(f"check-cr-docs: {len(report.violations)} violation(s)")
    return status


if __name__ == "__main__":
    sys.exit(main(sys.argv))
