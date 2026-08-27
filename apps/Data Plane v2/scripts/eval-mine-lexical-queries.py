"""Mine unambiguous exact-term queries from the corpus, for the lexical arms.

## Why

The 87-query golden set is entirely long natural-language questions: 0 queries
of 3 words or fewer, 7 Norwegian, no identifier lookups. That is the query class
where the dense arm dominates, which is why sweeping `w_bm25` showed the lexical
arm only ever costing nDCG — the set contains none of the queries BM25 and the
keyword arm exist to serve (exact symbols, file paths, config keys; what a
developer actually types into a search box).

## Why mined rather than hand-written

Judgment quality is the whole game. A hand-written identifier query needs me to
*believe* which document defines a symbol. Mining inverts that: take tokens that
occur in exactly ONE document in the corpus, and that document is the answer by
construction. No guessing, and the uniqueness is re-checkable at any time.

Terms appearing in 2 documents keep both as relevant ids (a legitimate
multi-answer judgment). Anything in 3+ documents is dropped as ambiguous.

Run `eval-build-golden-set.py` first; this writes a separate
`golden-lexical*.json` pair that can be evaluated alone (to see the lexical
arms' value in isolation) or concatenated with the main set.

Usage:
  python3 scripts/eval-mine-lexical-queries.py <out-dir> [--limit N]
"""
import json, os, re, subprocess, sys
from collections import defaultdict

PG = "data-plane-v2-postgres-1"
ORGS = {"baseline": "org-corpus-baseline", "contextual": "org-corpus-contextual"}

# Shapes a person would type verbatim and expect an exact hit on. Deliberately
# narrow: tokens a dense embedding tends to smear away and a lexical index nails.
PATTERNS = [
    r"\b[a-z][a-z0-9]*_[a-z0-9_]{3,}\b",          # snake_case: content_tsv
    r"\b[A-Z][A-Z0-9]*_[A-Z0-9_]{3,}\b",          # SCREAMING_CASE: RERANK_TOP_K
    r"\b[a-z0-9_/-]+\.(?:go|rs|py|ts|sql|yaml|yml)\b",   # paths: internal/jobs/executor.go
    r"\b[a-z]+(?:-[a-z]+){1,2}-(?:rs|go)\b",      # service crates: quickwit-adapter-rs
]
# Ubiquitous column/field names — present in nearly every document, so useless
# as a discriminating query even though they match the patterns.
STOP = {
    "org_id", "document_id", "knowledge_id", "created_at", "updated_at",
    "deleted_at", "data_plane", "user_id", "top_k", "top_n", "max_age",
    "chunk_index", "entity_id", "service_id", "principal_type", "trace_id",
}
MIN_LEN = 10
# Nobody types a 60-character path into a search box. Cap length so the mined
# queries stay in the band a person actually enters.
MAX_LEN = 30


def psql_json(sql):
    """Return rows as parsed JSON objects.

    Postgres emits the JSON rather than a delimited format, because neither a
    delimiter nor a line break survives this path reliably: `-F"\\t"` through the
    nested docker/sh quoting arrives as the two literal characters `\\` and `t`,
    and document content contains newlines, so line-oriented parsing tears rows
    apart. JSON escapes both, giving exactly one object per output line.
    """
    out = subprocess.run(
        ["docker", "exec", PG, "sh", "-c",
         f'psql -U "$POSTGRES_USER" -d "$POSTGRES_DB" -t -A -c {json.dumps(sql)}'],
        capture_output=True, text=True, timeout=600,
        encoding="utf-8", errors="replace")
    if out.returncode != 0:
        sys.exit(f"psql failed: {(out.stderr or '')[:400]}")
    rows = []
    for line in (out.stdout or "").splitlines():
        line = line.strip()
        if line.startswith("{"):
            try:
                rows.append(json.loads(line))
            except json.JSONDecodeError:
                pass
    return rows


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    out_dir = args[0] if args else os.path.dirname(os.path.abspath(__file__))
    limit = 24
    if "--limit" in sys.argv:
        limit = int(sys.argv[sys.argv.index("--limit") + 1])

    org = ORGS["baseline"]
    rows = psql_json(
        "SELECT json_build_object('d', document_id, 'c', content)::text "
        f"FROM documents WHERE org_id='{org}' AND deleted_at IS NULL;")
    if not rows:
        sys.exit("no documents read — is the corpus ingested in this org?")

    where = defaultdict(set)
    for row in rows:
        doc_id, body = row["d"], row.get("c") or ""
        found = set()
        for pat in PATTERNS:
            for m in re.findall(pat, body):
                # Trailing/leading underscores are regex-boundary artifacts, not
                # part of the identifier, and would not tokenize the same way.
                t = m.strip().strip(".,;:()[]{}\"'`").strip("_")
                if not (MIN_LEN <= len(t) <= MAX_LEN):
                    continue
                if t.lower() in STOP:
                    continue
                # A bare filename with no path (`mod.rs`, `lib.rs`, `test.ts`)
                # is unique in a corpus this size only by accident, and is not
                # an information need anyone actually has — its "answer" would
                # be arbitrary. Require a path separator on file-shaped tokens.
                if "." in t and "/" not in t:
                    continue
                found.add(t)
        for t in found:
            where[t].add(doc_id)

    unique = {t: sorted(d) for t, d in where.items() if 1 <= len(d) <= 2}
    # Ranking is for REALISM, not for length. Order by:
    #   1. single-document hits first — the sharpest possible judgment
    #   2. bare symbols before paths — `content_tsv` is a query someone types,
    #      `services/foo/internal/bar/baz.go` is one they paste at most
    #   3. shorter first within those, since a search box gets short input
    #   2. MORE underscore/segment structure first — a compound identifier
    #      (`graph_text_units`, `embedding_retry_count`) is domain-specific and
    #      a genuine lookup; a short generic one (`is_none`, `db_name`) is
    #      unique here only by accident of corpus size
    #   3. bare symbols before paths, then shorter first
    ranked = sorted(
        unique.items(),
        key=lambda kv: (len(kv[1]), -kv[0].count("_"), "/" in kv[0], len(kv[0])))
    picked = ranked[:limit]

    print(f"documents read             : {len(rows)}")
    print(f"candidate tokens seen      : {len(where)}")
    print(f"unambiguous (1-2 documents): {len(unique)}")
    print(f"selected                   : {len(picked)}\n")

    titles = {
        r["d"]: r["t"]
        for r in psql_json(
            "SELECT json_build_object('d', document_id, 't', title)::text "
            f"FROM documents WHERE org_id='{org}';")
    }

    per_org = {}
    for label, o in ORGS.items():
        by_title = defaultdict(list)
        for r in psql_json(
                "SELECT json_build_object('t', title, 'd', document_id)::text "
                f"FROM documents WHERE org_id='{o}';"):
            by_title[r["t"]].append(r["d"])
        per_org[label] = by_title

    built = {}
    for label in ORGS:
        entries, skipped = [], 0
        for token, docs in picked:
            ids, ok = [], True
            for d in docs:
                cand = per_org[label].get(titles.get(d), [])
                if len(cand) != 1:      # ambiguous title (e.g. seven READMEs)
                    ok = False
                    break
                ids.append(cand[0])
            if ok:
                entries.append({"query": token, "relevant_ids": ids})
            else:
                skipped += 1
        built[label] = (entries, skipped)

    counts = {k: len(v[0]) for k, v in built.items()}
    if len(set(counts.values())) != 1:
        sys.exit(f"orgs disagree on entry count: {counts} — A/B would not compare")

    for label, (entries, skipped) in built.items():
        suffix = "" if label == "baseline" else "-ctx"
        path = os.path.join(out_dir, f"golden-lexical{suffix}.json")
        json.dump(entries, open(path, "w", encoding="utf-8"), indent=1)
        print(f"{label:<11} {len(entries):>3} queries "
              f"({skipped} skipped as title-ambiguous) -> {os.path.basename(path)}")

    print("\nmined queries (each occurs in only 1-2 documents in this corpus):")
    for token, docs in picked:
        where_str = ", ".join(titles.get(d, d)[:30] for d in docs)
        print(f"  {token:<32} -> {where_str}")


if __name__ == "__main__":
    main()
