#!/usr/bin/env python3
"""Seed `eval_golden_judgments` with a known-item golden set.

Plan item P0.5. See README.md for the method and its limitations — in short:
for each selected chunk we derive a query from that chunk's distinctive
vocabulary and record its `knowledge_id` as the relevant answer, giving a real
(if conservative) recall@10 / nDCG@10 / MRR signal instead of the
`min(candidates/10, 1)` proxy.

Read-only by default. `--apply` performs an idempotent upsert.

Deliberately dependency-free (stdlib + `docker exec psql`) so it runs without
provisioning a Python environment or handing a DB password to a new process.
"""

from __future__ import annotations

import argparse
import re
import shlex
import subprocess
import sys
from collections import Counter

PG_CONTAINER = "data-plane-v2-postgres-1"
DELIM = "@@@"

# Chunks shorter than this rarely contain enough distinctive vocabulary to form
# a query that identifies them.
MIN_CHUNK_CHARS = 200
# Terms per generated query. Long enough to be discriminating, short enough that
# it reads like something a person would type.
QUERY_TERMS = 5

# Stopwords for both corpus languages. The live corpus measured ~60% English /
# ~38% Norwegian on 2026-08-05, with the two mixing inside single chunks, so
# both lists are always applied.
STOP_EN = set(
    """a an the and or but if then else of to in for with that this these those from by on at as it
    is are was were be been being have has had do does did will would shall should can could may
    might must not no nor so than too very s t just don now also more most other some such only own
    same how what when where which who whom why all any both each few he her hers him his i me my
    our ours you your yours they them their we us she it its""".split()
)
STOP_NO = set(
    """og er det som ikke til av for en et på med den de vi kan skal har være dette disse eller når
    hvor fra ved om noen alle mer må vil siden også bare etter over under mellom blir ble her der
    han hun de dem deres vår våre din dine jeg meg min mine du deg seg hva hvem hvilken hvorfor
    hvordan slik sånn men så da nå enn ja nei per via samt både hverken verken""".split()
)
STOPWORDS = STOP_EN | STOP_NO

TOKEN_RE = re.compile(r"[a-zA-ZæøåÆØÅ][a-zA-ZæøåÆØÅ\-]{2,}")

# Chrome/navigation/consent vocabulary. These terms can be corpus-rare (so they
# survive the tf/df weighting) while carrying no retrievable meaning — an early
# run produced the query "denne klikke klikk bildet teksten" ("this click click
# the-image the-text") from a page's image caption furniture. A golden set built
# on boilerplate reports confident numbers about nothing.
BOILERPLATE = set(
    """klikk klikke klikker bildet bilde teksten tekst lenke lenken lenker knapp knappen
    meny menyen side siden sider nettside nettsiden hjemmeside forside innhold
    cookie cookies informasjonskapsler samtykke personvern personvernerklaering
    click clicking image images link links button menu page pages website homepage
    content consent privacy policy cookiepolicy newsletter subscribe login logg
    chrome firefox safari edge browser nettleser javascript enabled aktivert
    copyright rights reserved sitemap kontakt contact email e-post telefon phone
    read-more les-mer more mer here her""".split()
)


def is_boilerplate_heavy(terms: list[str]) -> bool:
    """True when a query's terms are mostly page furniture rather than content."""
    if not terms:
        return True
    hits = sum(1 for t in terms if t in BOILERPLATE)
    return hits * 2 >= len(terms)


def normalize_query(text: str) -> str:
    """Mirror of `data-quality-go/internal/eval/golden.go:23`.

    `strings.Join(strings.Fields(strings.ToLower(q)), " ")` — lowercase and
    collapse all whitespace. If this drifts, judgments silently stop joining to
    retrieval traces and every query falls back to the proxy metric.
    """
    return " ".join(text.lower().split())


def psql(sql: str) -> str:
    """Run SQL inside the Postgres container.

    Uses `shlex.quote`, not `repr`: Python's repr switches to single quotes when
    a string contains a double quote, and then backslash-escapes the inner
    single quotes — which `sh` does not unescape the same way. SQL containing
    both quote characters therefore reached the shell malformed
    ("syntax error: unexpected `)`"). The password is read from the container's
    own environment and never crosses the argv boundary.
    """
    proc = subprocess.run(
        [
            "docker", "exec", PG_CONTAINER, "sh", "-c",
            'PGPASSWORD="$POSTGRES_PASSWORD" psql -U "$POSTGRES_USER" '
            '-d "$POSTGRES_DB" -v ON_ERROR_STOP=1 -t -A -c ' + shlex.quote(sql),
        ],
        capture_output=True,
        text=True,
    )
    if proc.returncode != 0:
        sys.exit(f"psql failed: {proc.stderr.strip()[:400]}")
    return proc.stdout


def looks_norwegian(text: str) -> bool:
    low = text.lower()
    if re.search(r"[æøå]", low):
        return True
    toks = TOKEN_RE.findall(low)
    if not toks:
        return False
    no_hits = sum(1 for t in toks if t in STOP_NO and t not in STOP_EN)
    en_hits = sum(1 for t in toks if t in STOP_EN and t not in STOP_NO)
    return no_hits > en_hits


def fetch_chunks() -> list[dict]:
    sql = (
        f"SELECT knowledge_id || '{DELIM}' || org_id || '{DELIM}' || "
        f"translate(text, chr(10) || chr(13) || chr(9), '   ') "
        "FROM knowledge_units WHERE embedding_status <> 'failed' ORDER BY knowledge_id;"
    )
    out = []
    for line in psql(sql).split("\n"):
        if DELIM not in line:
            continue
        parts = line.split(DELIM)
        if len(parts) != 3:
            continue
        kid, org, text = parts
        if len(text) < MIN_CHUNK_CHARS:
            continue
        out.append({"knowledge_id": kid, "org_id": org, "text": text})
    return out


def build_query(text: str, corpus_df: Counter, total_docs: int) -> str | None:
    """Pick the chunk's most *distinctive* terms.

    Ranks by term frequency in this chunk divided by how many chunks contain the
    term, so corpus-wide boilerplate ("coresystem", "tjenester") loses to terms
    that actually single this chunk out. Without that weighting most generated
    queries collapse onto the same handful of common words.
    """
    tokens = [t.lower() for t in TOKEN_RE.findall(text)]
    tokens = [
        t
        for t in tokens
        if t not in STOPWORDS and t not in BOILERPLATE and len(t) > 3
    ]
    if len(tokens) < QUERY_TERMS:
        return None

    tf = Counter(tokens)
    scored = sorted(
        tf.items(),
        key=lambda kv: (kv[1] / (1 + corpus_df[kv[0]] / total_docs), kv[1]),
        reverse=True,
    )
    # Preserve the order the terms appear in the chunk so the query reads more
    # like prose than a bag of keywords.
    chosen = {t for t, _ in scored[:QUERY_TERMS]}
    ordered, seen = [], set()
    for t in tokens:
        if t in chosen and t not in seen:
            seen.add(t)
            ordered.append(t)
    if len(ordered) < 3 or is_boilerplate_heavy(ordered):
        return None
    return " ".join(ordered)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--apply", action="store_true", help="upsert into eval_golden_judgments")
    ap.add_argument("--dry-run", action="store_true", help="print only (default)")
    ap.add_argument("--limit", type=int, default=60, help="max judged queries (default 60)")
    args = ap.parse_args()

    chunks = fetch_chunks()
    print(f"eligible chunks (>= {MIN_CHUNK_CHARS} chars, not failed): {len(chunks)}")
    if not chunks:
        sys.exit("no eligible chunks; nothing to seed")

    corpus_df: Counter = Counter()
    for c in chunks:
        corpus_df.update(set(t.lower() for t in TOKEN_RE.findall(c["text"])))
    total = len(chunks)

    # Interleave Norwegian and English so a truncated run stays balanced rather
    # than seeding one language only.
    no_c = [c for c in chunks if looks_norwegian(c["text"])]
    en_c = [c for c in chunks if not looks_norwegian(c["text"])]
    print(f"  norwegian: {len(no_c)}   english: {len(en_c)}")

    interleaved: list[dict] = []
    for i in range(max(len(no_c), len(en_c))):
        if i < len(no_c):
            interleaved.append(no_c[i])
        if i < len(en_c):
            interleaved.append(en_c[i])

    seen_queries: set[str] = set()
    rows: list[tuple[str, str, str, str]] = []
    for c in interleaved:
        if len(rows) >= args.limit:
            break
        q = build_query(c["text"], corpus_df, total)
        if not q:
            continue
        qn = normalize_query(q)
        if qn in seen_queries:
            # PK is (org_id, query_norm) — a duplicate would overwrite the
            # earlier judgment and silently shrink the set.
            continue
        seen_queries.add(qn)
        lang = "no" if looks_norwegian(c["text"]) else "en"
        rows.append((c["org_id"], qn, c["knowledge_id"], lang))

    n_no = sum(1 for r in rows if r[3] == "no")
    print(f"\ngenerated {len(rows)} judged queries  (no={n_no}, en={len(rows) - n_no})\n")
    for org, qn, kid, lang in rows[:12]:
        print(f"  [{lang}] {qn[:66]:66s} -> {kid[:8]}…")
    if len(rows) > 12:
        print(f"  … {len(rows) - 12} more")

    if not args.apply:
        print("\nDRY RUN — re-run with --apply to upsert.")
        return

    # `jsonb_build_array('<kid>')` rather than a JSON literal: it keeps double
    # quotes out of the statement entirely, so the SQL survives the shell hop
    # with single-quote escaping alone. Every value is still SQL-escaped.
    values = ",".join(
        "('{}','{}',jsonb_build_array('{}'))".format(
            org.replace("'", "''"),
            qn.replace("'", "''"),
            kid.replace("'", "''"),
        )
        for org, qn, kid, _ in rows
    )
    sql = (
        "INSERT INTO eval_golden_judgments (org_id, query_norm, relevant_ids) VALUES "
        + values
        + " ON CONFLICT (org_id, query_norm) DO UPDATE SET "
        "relevant_ids = EXCLUDED.relevant_ids, updated_at = NOW();"
    )
    psql(sql)
    count = psql("SELECT count(*) FROM eval_golden_judgments;").strip()
    print(f"\napplied. eval_golden_judgments now holds {count} rows.")


if __name__ == "__main__":
    main()
