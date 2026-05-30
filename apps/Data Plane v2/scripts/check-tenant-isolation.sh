#!/usr/bin/env bash
set -euo pipefail

# Static analysis: every API-facing SQL query on a tenant table must include
# either `org_id` or a primary-key scope predicate that carries org ownership
# via FK (document_id, knowledge_id, page_id, ...).
#
# Pragmatic check, not a formal proof. Background workers operating on
# FK-scoped IDs from NATS events are exempt — the upstream creator already
# proved org ownership.
#
# Run from the Data Plane v2 root:
#   ./scripts/check-tenant-isolation.sh

cd "$(dirname "$0")/.."

exec python3 - "$@" <<'PYEOF'
import os, re, sys
from pathlib import Path

ROOT = Path.cwd()

TENANT_TABLES = [
    "documents", "knowledge_units", "retrieval_traces", "retrieval_candidates",
    "wiki_pages", "wiki_page_versions",
    "graph_entities", "graph_relationships", "graph_claims", "graph_text_units",
    "evals", "eval_runs", "eval_scorecards", "trust_scores",
    "source_logs", "maintenance_logs",
]

EXEMPT_PATHS = [
    "tools/migrator/",
    "infra/postgres/",
    "scripts/",
    "tests/",
    # Background workers receive FK-scoped IDs from org-scoped events
    "services/index-engine-rs/src/builder/",
    "services/embedding-engine-rs/src/batch/",
    "services/embedding-engine-rs/src/stream/",
    "services/embedding-engine-rs/src/qdrant_writer/",
    "services/graph-index-rs/src/extractor/",
    "services/graph-index-rs/src/community/",
    # Cross-org admin aggregations
    "services/data-orchestrator-go/internal/jobs/stale_detector.go",
    # Search functions: subqueries scope via outer org_id
    "services/retrieval-engine-rs/src/search/wiki.rs",
    "services/retrieval-engine-rs/src/search/timeline.rs",
]

SCOPE_PREDICATES = re.compile(
    r"\b(org_id|document_id|knowledge_id|page_id|version_id|"
    r"entity_id|rel_id|claim_id|trace_id|eval_id)\b"
)
SCHEMA_DDL = re.compile(r"\b(CREATE TABLE|ALTER TABLE|CREATE INDEX|DROP TABLE|DROP INDEX|REFERENCES)\b", re.I)
TABLE_REF = re.compile(
    r"\b(FROM|INTO|UPDATE|JOIN)\s+(" + "|".join(TENANT_TABLES) + r")\b",
    re.I,
)

# Match string literals: Rust raw strings r"..." or r#"..."#, regular "...",
# Go raw strings `...`, and double-quoted Go strings.
LITERALS = re.compile(
    r'r#"(?P<rust_raw_hash>(?:[^"]|"(?!#))*)"#'
    r'|r"(?P<rust_raw>[^"]*)"'
    r'|"(?P<dq>(?:\\.|[^"\\])*)"'
    r"|`(?P<backtick>[^`]*)`",
    re.DOTALL,
)


def is_exempt(path: str) -> bool:
    return any(p in path for p in EXEMPT_PATHS)


def find_files() -> list[Path]:
    out = []
    for root in ("services", "tools"):
        rp = ROOT / root
        if not rp.exists():
            continue
        for p in rp.rglob("*"):
            if not p.is_file():
                continue
            if p.suffix not in (".rs", ".go"):
                continue
            if "/target/" in str(p) or "/node_modules/" in str(p):
                continue
            out.append(p)
    return out


def check_literal(text: str) -> bool:
    """Return True if this SQL literal references a tenant table without a scope predicate."""
    if not TABLE_REF.search(text):
        return False
    if SCHEMA_DDL.search(text):
        return False
    return SCOPE_PREDICATES.search(text) is None


violations: list[tuple[str, int, str]] = []

for path in find_files():
    rel = str(path.relative_to(ROOT))
    if is_exempt(rel):
        continue

    src = path.read_text(encoding="utf-8", errors="ignore")
    for m in LITERALS.finditer(src):
        body = next((g for g in m.groups() if g is not None), "")
        if not body:
            continue
        # Must look like SQL (have a verb).
        if not re.search(r"\b(SELECT|INSERT|UPDATE|DELETE)\b", body, re.I):
            continue
        if check_literal(body):
            line = src.count("\n", 0, m.start()) + 1
            violations.append((rel, line, body[:180].replace("\n", " ")))

if violations:
    print("TENANT ISOLATION VIOLATIONS:\n")
    for path, line, snippet in violations:
        print(f"  {path}:{line}: SQL on tenant table without scoping predicate")
        print(f"      {snippet}{'...' if len(snippet) >= 180 else ''}\n")
    print(f"Total flagged: {len(violations)}")
    print("")
    print("If a finding is intentional:")
    print("  - Add explicit org_id scoping (preferred)")
    print("  - Add the file to EXEMPT_PATHS with a comment")
    sys.exit(1)

print("✓ All API-handler SQL on tenant tables is scoped by org_id or a PK predicate.")
PYEOF
