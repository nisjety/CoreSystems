#!/usr/bin/env bash
# Expand/contract lint for Model Plane migrations (docs/runbooks/migration-policy.md).
#
# Past each service's cutoff a migration must be additive: a non-additive
# statement needs a `-- contract: <what>` marker and a matching revert file,
# and every new session-core migration needs `revert/NNNN_*.down.sql`. The
# lint is what makes SESSION_CORE_MIGRATION_IGNORE_MISSING safe to use: an
# older binary over a newer schema only works while the newer schema is
# additive.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

python3 - "$ROOT_DIR" <<'PY'
import pathlib
import re
import sys
import tempfile

# additive_from: first migration number the additive rule covers.
# revert_from: first migration number that always needs a revert file
# (None: only contract migrations need one).
# session-core 0044-0048 predate the policy; they carry reverts but drop
# constraints they immediately replace, so they are grandfathered from the
# additive rule. capability-core 0003-0005 have no down files and stay so.
SERVICES = [
    {"name": "session-core", "dir": "rust/services/session-core/migrations", "layout": "revert-dir", "additive_from": 49, "revert_from": 44},
    {"name": "capability-core", "dir": "go/services/capability-core/migrations", "layout": "up-down", "additive_from": 17, "revert_from": None},
    {"name": "cost-core", "dir": "go/services/cost-core/migrations", "layout": "up-down", "additive_from": 11, "revert_from": None},
    {"name": "letta-bridge", "dir": "go/services/letta-bridge/migrations", "layout": "up-down", "additive_from": 3, "revert_from": None},
]

MARKER = re.compile(r"^[ \t]*--[ \t]*contract:[ \t]*\S+", re.MULTILINE)
NON_ADDITIVE = [
    ("DROP", re.compile(r"\bDROP\b", re.I)),
    ("RENAME", re.compile(r"\bRENAME\b", re.I)),
    ("TRUNCATE", re.compile(r"\bTRUNCATE\b", re.I)),
    ("DELETE FROM", re.compile(r"\bDELETE\s+FROM\b", re.I)),
    ("ALTER COLUMN TYPE", re.compile(r"\bALTER\s+COLUMN\s+\S+\s+(SET\s+DATA\s+)?TYPE\b", re.I)),
    ("SET NOT NULL", re.compile(r"\bALTER\s+COLUMN\s+\S+\s+SET\s+NOT\s+NULL\b", re.I)),
]
ADD_COLUMN = re.compile(r"\bADD\s+COLUMN\b", re.I)
NOT_NULL = re.compile(r"\bNOT\s+NULL\b", re.I)
DEFAULTED = re.compile(r"\b(DEFAULT|GENERATED)\b", re.I)


def executable_sql(text):
    """The statements that run, without comments, string literals or function bodies."""
    text = re.sub(r"/\*.*?\*/", " ", text, flags=re.S)
    text = re.sub(r"--[^\n]*", " ", text)
    text = re.sub(r"\$(\w*)\$.*?\$\1\$", " ", text, flags=re.S)
    return re.sub(r"'(?:[^']|'')*'", "''", text)


def split_top_level(statement):
    parts, depth, current = [], 0, []
    for char in statement:
        depth += (char == "(") - (char == ")")
        if char == "," and depth == 0:
            parts.append("".join(current))
            current = []
        else:
            current.append(char)
    parts.append("".join(current))
    return parts


def non_additive_reasons(text):
    sql = executable_sql(text)
    reasons = [label for label, pattern in NON_ADDITIVE if pattern.search(sql)]
    for statement in sql.split(";"):
        for clause in split_top_level(statement):
            if ADD_COLUMN.search(clause) and NOT_NULL.search(clause) and not DEFAULTED.search(clause):
                reasons.append("ADD COLUMN NOT NULL without DEFAULT")
    return reasons


def migrations(root, service):
    directory = root / service["dir"]
    suffix = r"\.up\.sql" if service["layout"] == "up-down" else r"\.sql"
    for path in sorted(directory.iterdir()):
        match = re.fullmatch(r"(\d{4})_.+" + suffix, path.name)
        if match and path.is_file():
            yield int(match.group(1)), path


def revert_path(service, path):
    if service["layout"] == "revert-dir":
        return path.parent / "revert" / (path.stem + ".down.sql")
    return path.parent / (path.name[: -len(".up.sql")] + ".down.sql")


def lint(root, services):
    problems = []
    for service in services:
        for number, path in migrations(root, service):
            revert = revert_path(service, path)
            has_revert = revert.is_file() and revert.stat().st_size > 0
            relative = path.relative_to(root).as_posix()
            if service["revert_from"] is not None and number >= service["revert_from"] and not has_revert:
                problems.append(f"{relative}: missing revert file {revert.relative_to(root).as_posix()}")
            if number < service["additive_from"]:
                continue
            text = path.read_text(encoding="utf-8")
            reasons = non_additive_reasons(text)
            if not reasons:
                continue
            kinds = ", ".join(sorted(set(reasons)))
            if not MARKER.search(text):
                problems.append(f"{relative}: non-additive ({kinds}) without a '-- contract: <what>' marker")
            elif not has_revert:
                problems.append(f"{relative}: contract migration ({kinds}) without a matching revert file {revert.relative_to(root).as_posix()}")
    return problems


def write(root, relative, text):
    path = root / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def self_test():
    rust = {"name": "rs", "dir": "rs", "layout": "revert-dir", "additive_from": 10, "revert_from": 5}
    go = {"name": "go", "dir": "go", "layout": "up-down", "additive_from": 10, "revert_from": None}
    cases = [
        # (label, files, service, expected problem fragments; empty means clean)
        ("additive passes", {"rs/0010_a.sql": "CREATE TABLE t (id TEXT NOT NULL);", "rs/revert/0010_a.down.sql": "DROP TABLE t;"}, rust, []),
        ("pre-additive cutoff drop passes", {"rs/0006_a.sql": "ALTER TABLE t DROP COLUMN c;", "rs/revert/0006_a.down.sql": "SELECT 1;"}, rust, []),
        ("missing revert fails", {"rs/0010_a.sql": "CREATE TABLE t (id TEXT);"}, rust, ["missing revert file"]),
        ("drop without marker fails", {"rs/0010_a.sql": "ALTER TABLE t DROP COLUMN c;", "rs/revert/0010_a.down.sql": "SELECT 1;"}, rust, ["without a '-- contract: <what>' marker"]),
        ("marker without revert fails", {"rs/0010_a.sql": "-- contract: drop c\nALTER TABLE t DROP COLUMN c;"}, rust, ["missing revert file", "without a matching revert file"]),
        ("marker with revert passes", {"rs/0010_a.sql": "-- contract: drop c\nALTER TABLE t DROP COLUMN c;", "rs/revert/0010_a.down.sql": "ALTER TABLE t ADD COLUMN c TEXT;"}, rust, []),
        ("empty marker text fails", {"rs/0010_a.sql": "-- contract:\nALTER TABLE t DROP COLUMN c;", "rs/revert/0010_a.down.sql": "SELECT 1;"}, rust, ["without a '-- contract: <what>' marker"]),
        ("not null column without default fails", {"rs/0010_a.sql": "ALTER TABLE t ADD COLUMN IF NOT EXISTS c TEXT NOT NULL;", "rs/revert/0010_a.down.sql": "SELECT 1;"}, rust, ["ADD COLUMN NOT NULL without DEFAULT"]),
        ("not null column with default passes", {"rs/0010_a.sql": "ALTER TABLE t ADD COLUMN IF NOT EXISTS c TEXT NOT NULL DEFAULT '';", "rs/revert/0010_a.down.sql": "SELECT 1;"}, rust, []),
        ("second add column clause is checked", {"rs/0010_a.sql": "ALTER TABLE t ADD COLUMN a TEXT NOT NULL DEFAULT '', ADD COLUMN b INT NOT NULL;", "rs/revert/0010_a.down.sql": "SELECT 1;"}, rust, ["ADD COLUMN NOT NULL without DEFAULT"]),
        ("set not null fails", {"rs/0010_a.sql": "ALTER TABLE t ALTER COLUMN c SET NOT NULL;", "rs/revert/0010_a.down.sql": "SELECT 1;"}, rust, ["SET NOT NULL"]),
        ("column type change fails", {"rs/0010_a.sql": "ALTER TABLE t ALTER COLUMN c TYPE BIGINT;", "rs/revert/0010_a.down.sql": "SELECT 1;"}, rust, ["ALTER COLUMN TYPE"]),
        ("rename fails", {"rs/0010_a.sql": "ALTER TABLE t RENAME COLUMN a TO b;", "rs/revert/0010_a.down.sql": "SELECT 1;"}, rust, ["RENAME"]),
        ("delete fails", {"rs/0010_a.sql": "DELETE FROM t;", "rs/revert/0010_a.down.sql": "SELECT 1;"}, rust, ["DELETE FROM"]),
        ("drop in comment passes", {"rs/0010_a.sql": "-- DROP TABLE t would be wrong here\nCREATE INDEX i ON t (c); /* DROP INDEX j */", "rs/revert/0010_a.down.sql": "SELECT 1;"}, rust, []),
        ("drop in function body passes", {"rs/0010_a.sql": "CREATE FUNCTION f() RETURNS void AS $$ BEGIN DROP TABLE IF EXISTS tmp; END $$ LANGUAGE plpgsql;", "rs/revert/0010_a.down.sql": "SELECT 1;"}, rust, []),
        ("go non-additive without down fails", {"go/0010_a.up.sql": "-- contract: drop c\nALTER TABLE t DROP COLUMN c;"}, go, ["without a matching revert file"]),
        ("go contract with down passes", {"go/0010_a.up.sql": "-- contract: drop c\nALTER TABLE t DROP COLUMN c;", "go/0010_a.down.sql": "ALTER TABLE t ADD COLUMN c TEXT;"}, go, []),
        ("go additive without down passes", {"go/0010_a.up.sql": "CREATE TABLE t (id TEXT);"}, go, []),
    ]
    failures = []
    for label, files, service, expected in cases:
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            (root / service["dir"]).mkdir(parents=True)
            for relative, text in files.items():
                write(root, relative, text)
            problems = lint(root, [service])
        ok = (not problems) if not expected else all(any(fragment in p for p in problems) for fragment in expected)
        if not ok:
            failures.append(f"self-test '{label}': expected {expected or 'clean'}, got {problems or 'clean'}")
    return failures


root = pathlib.Path(sys.argv[1])
failures = self_test()
if failures:
    print("\n".join(failures), file=sys.stderr)
    raise SystemExit("migration lint self-test failed")

for service in SERVICES:
    if not (root / service["dir"]).is_dir():
        raise SystemExit(f"migration directory is missing: {service['dir']}")

problems = lint(root, SERVICES)
if problems:
    print("\n".join(problems), file=sys.stderr)
    raise SystemExit("migration expand/contract lint failed")
print("migration expand/contract lint ok")
PY
