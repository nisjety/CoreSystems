"""Build the expanded golden set for both eval orgs.

Queries are keyed by document TITLE and resolved to per-org document ids, because
the two orgs hold the same corpus under different ids.

## Why these are hand-authored

The obvious way to scale a golden set is to have an LLM read each document and
emit questions about it. That is the wrong instrument for what this set is used
for. Generated-from-source questions inherit the source's vocabulary, which
inflates lexical retrieval specifically — so a w_bm25 sweep scored against them
would measure the generator's phrasing habits rather than the retriever. These
are written the way a person actually asks: natural phrasing, no heading text
copied, and deliberately not the document's own wording where a synonym exists.

## Judgment discipline

Every query below is grounded in structure verified from the live corpus (its
heading outline) or in a document read directly. A query is only included if its
best answer is unambiguous — the corpus contains near-duplicate families
(DEEP DIVE / STATUS / ROADMAP per plane, nine service audits sharing an outline,
seven files titled README), and a broad question like "which services make up
the data plane" has three defensible answers. Those are omitted rather than
judged arbitrarily: a wrong judgment does not just add noise, it penalises a
retriever for being right.

`verbatim_overlap` reports, per query, the fraction of its content words that
appear literally in the judged document. It is a bias check, not a filter —
high overlap on a query about a named service is expected and fine; a whole set
skewing high would mean the queries had drifted toward copying source text.
"""
import json, os, re, subprocess, sys
from collections import Counter

OUT_DIR = os.path.dirname(os.path.abspath(__file__))
ORGS = {"baseline": "org-corpus-baseline", "contextual": "org-corpus-contextual"}

# (query, [document titles that genuinely answer it])
QUERIES = [
    # ---- retrieval / RAG internals -------------------------------------------
    ("why was the LanguageBind video tower removed and what replaced it", ["video temporal retrieval gap 2026 08 25"]),
    ("can any video embedding model tell a clip from its own reverse", ["video temporal retrieval gap 2026 08 25"]),
    ("is there a benchmark that checks whether retrieval understands the order events happen in", ["video temporal retrieval gap 2026 08 25"]),
    ("we index video but motion is invisible to search — what did we do about it", ["video temporal retrieval gap 2026 08 25"]),
    ("how does retrieval combine dense sparse and graph results into one ranking", ["retrieval engine rs"]),
    ("how does the retrieval service prove who it is when calling the inference service", ["retrieval engine rs"]),
    ("which metrics does the retrieval quality evaluation compute", ["data quality go"]),
    ("why can't I read back the result of an evaluation run", ["data quality go"]),
    ("how does the golden judgment set join to retrieval traces", ["retrieval eval py"]),
    ("which retrieval gaps were still open according to the gap analysis", ["gap data"]),
    ("what was the plan for durable retrieval quality improvements", ["retrieval quality and durability plan 2026 08 05"]),

    # ---- chunking / embedding / indexing -------------------------------------
    ("how do documents get chunked before embedding", ["index engine rs"]),
    ("which service is responsible for re-driving failed embeddings", ["index engine rs"]),
    ("did the chunking service ever get stuck restarting over and over", ["index engine rs"]),
    ("what did rebuilding the embedding service under compose actually verify", ["embedding engine rs"]),
    ("how do stale embeddings get detected", ["data orchestrator go"]),
    ("what jobs can the data orchestrator run", ["data orchestrator go"]),
    ("does the orchestrator remember what it was doing after a restart", ["data orchestrator go"]),

    # ---- sparse / search index -----------------------------------------------
    ("what does the quickwit adapter rebuild endpoint do", ["quickwit read model"]),
    ("how long before a deleted document stops turning up in search results", ["quickwit read model"]),
    ("why does the same source object show up twice in the search index", ["quickwit read model"]),
    ("does the search adapter's schema actually match the database", ["quickwit adapter rs"]),

    # ---- graph ----------------------------------------------------------------
    ("how is the knowledge graph built from documents", ["graph index rs"]),
    ("there was a reproducible bug in the graph service — where exactly was it", ["graph index rs"]),
    ("what node types and relationships does the graph database store", ["graphrag neo4j plan"]),
    ("how do we stop a multi-hop graph walk from surfacing documents someone may not see", ["graphrag neo4j plan"]),

    # ---- visual / audio -------------------------------------------------------
    ("how are page images reranked with a vision model", ["colqwen reranker"]),
    ("what GPU memory does the visual reranker need and what quantization", ["colqwen reranker"]),
    ("why did the first attempts at a visual pipeline design not work", ["visual rag integration plan"]),
    ("which mistakes should we avoid when building the image search path", ["visual rag integration plan"]),
    ("why did we pick LAION CLAP for the audio arm", ["embedding modality and rag audit 2026 08 19"]),
    ("does Azure offer an audio similarity embedding service", ["embedding modality and rag audit 2026 08 19"]),

    # ---- wiki -----------------------------------------------------------------
    ("how does the wiki store approve and version pages", ["wiki store go"]),
    ("listing wiki pages was failing with a server error — what caused it", ["wiki store go"]),
    ("what is weak about how the wiki service scopes requests to a tenant", ["wiki store go"]),
    ("what is inside the event we publish when a wiki version goes live", ["wiki events"]),
    ("how should a consumer treat fields it does not recognise in an event", ["wiki events"]),

    # ---- documents ------------------------------------------------------------
    ("which service owns document creation and idempotent ingest", ["documents api go"]),
    ("how does the documents service work out who is making a request", ["documents api go"]),

    # ---- plane ownership / architecture --------------------------------------
    ("what does the control plane own compared to other planes", ["CONTROL PLANE DEEP DIVE"]),
    ("what rules decide which plane gets to own a new capability", ["master ownership matrix"]),
    ("is there one table showing what every plane is responsible for", ["master ownership matrix"]),
    ("what is the overall architecture of the whole platform across planes", ["CODEBASE INFORMATION SYSTEM"]),
    ("what does convex-core own and what is explicitly not its job", ["APPLICATION PLANE DEEP DIVE"]),
    ("where do realtime collaborative workspace projections live", ["APPLICATION PLANE DEEP DIVE"]),
    ("which services sit on the reasoning and inference side", ["MODEL PLANE DEEP DIVE"]),
    ("what is the model gateway for", ["MODEL PLANE DEEP DIVE"]),
    ("how do browser automation sessions get executed and sandboxed", ["MODEL PLANE DEEP DIVE"]),
    ("what does integration-corev2 actually do", ["INGESTION PLANE DEEP DIVE"]),
    ("what is finspo-core responsible for", ["INGESTION PLANE DEEP DIVE"]),
    ("which part of ingestion handles bringing in existing files", ["INGESTION PLANE DEEP DIVE"]),
    ("what storage and event systems does the data plane sit on", ["DATA PLANE DEEP DIVE"]),

    # ---- control plane identity / events -------------------------------------
    ("which events does the control plane publish when an organization changes", ["CONTROL PLANE OWNERSHIP"]),
    ("why is Convex only allowed to hold a read-only copy", ["CONTROL PLANE OWNERSHIP"]),
    ("how do we keep Convex in step with the authoritative user records", ["CONVEX INTEGRATION SUMMARY"]),
    ("what stops one tenant from seeing another tenant's documents", ["CONTROL PLANE DEEP DIVE"]),

    # ---- ADRs -----------------------------------------------------------------
    ("why did we decide to give Spaces their own rows owned by control", ["SPACE AUTHORITY ADR 2026 08 13"]),
    ("could we not have reused an existing workspace identifier for Spaces", ["SPACE AUTHORITY ADR 2026 08 13"]),
    ("does the agent registry answer who is allowed to act, or just who is present", ["CROSS SPACE AGENT REGISTRY ADR 2026 08 19"]),

    # ---- privacy / GDPR -------------------------------------------------------
    ("what happens when an organization requests GDPR erasure of its data", ["GDPR SUMMARY"]),
    ("which privacy classes do we label data with", ["GDPR SUMMARY"]),
    ("where does the scraping proxy sit in our privacy story", ["GDPR SUMMARY"]),

    # ---- ops: ports, env, deploy ---------------------------------------------
    ("which ports does the base stack expose on the host", ["PORT MAPPING"]),
    ("how does one service address another inside the docker network", ["PORT MAPPING"]),
    ("which env file do I use for local dev versus running in docker", ["ENVIRONMENT FILES"]),
    ("in what order should I start the tiers when bringing the stack up", ["DEPLOYMENT CHECKLIST"]),
    ("what is the rollout strategy when shipping to production", ["production deploy"]),
    ("which secrets have to be present before a production deploy", ["production deploy"]),
    ("how do I get automatic deploys triggered from GitHub", ["WEBHOOK SETUP"]),
    ("my deploy hook is not firing — what should I check", ["WEBHOOK SETUP"]),
    ("how is strict TLS terminated in front of the stack", ["CLOUDFLARE SETUP"]),
    ("which security headers does the edge add", ["CLOUDFLARE SETUP"]),
    ("I am getting a 525 SSL error from the edge — why", ["TOOLS INTEGRATION"]),

    # ---- backup / migration ---------------------------------------------------
    ("how do I restore the data plane from a backup", ["backup restore"]),
    ("the vector store is corrupt but the database is fine — what now", ["backup restore"]),
    ("what exactly does a backup capture", ["backup restore"]),
    ("how do we move traffic from the old version to the new one", ["migration v1 to v2"]),
    ("what is the point of the shadow traffic stage", ["migration v1 to v2"]),

    # ---- Norwegian (bokmål / nynorsk) ---------------------------------------
    ("hvordan sikrer vi at sletting av kunnskap faktisk fjerner vektorene", ["GDPR SUMMARY"]),
    ("kva slags jobber køyrer datakvalitetstenesta", ["data quality go"]),
    ("kva reglar avgjer kven som eig kva", ["master ownership matrix"]),
    ("hvilke porter er tilgjengelige på verten", ["PORT MAPPING"]),
    ("hvordan setter jeg opp automatisk utrulling fra GitHub", ["WEBHOOK SETUP"]),
    ("hvordan gjenoppretter jeg systemet etter at databasen er ødelagt", ["backup restore"]),
    ("kva gjer vi når ein organisasjon ber om å bli sletta", ["GDPR SUMMARY"]),
]

STOP = set("""a an the is are was were be been being do does did how what which who
whom whose when where why to of in on at for from by with without and or not no
we our us i my it its that this these those can could should would will shall
may might must if then than as into about over under out up down get gets got
have has had there their them they you your me actually really just only also
one two some any all more most much many own does happen happens""".split())


def words(text):
    return [w for w in re.findall(r"[a-zøæåöä0-9_-]{3,}", text.lower()) if w not in STOP]


def psql(sql):
    out = subprocess.run(
        ["docker", "exec", "data-plane-v2-postgres-1", "sh", "-c",
         f'psql -U "$POSTGRES_USER" -d "$POSTGRES_DB" -t -A -F"|" -c {json.dumps(sql)}'],
        # utf-8 explicitly: the corpus is Norwegian-first and the default Windows
        # codec (cp1252) cannot decode it, which fails as a decode error inside a
        # reader thread rather than as a psql error.
        capture_output=True, text=True, timeout=180,
        encoding="utf-8", errors="replace")
    if out.returncode != 0:
        sys.exit(f"psql failed: {(out.stderr or '')[:400]}")
    return [l for l in (out.stdout or "").splitlines() if l.strip()]


def load(org):
    """title -> [ids]; content by title, for the overlap check."""
    rows = psql(f"SELECT title, document_id FROM documents "
                f"WHERE org_id='{org}' AND deleted_at IS NULL;")
    by_title = {}
    for line in rows:
        title, doc_id = line.rsplit("|", 1)
        by_title.setdefault(title.strip(), []).append(doc_id.strip())
    return by_title


def main():
    dupes = [q for q, n in Counter(q for q, _ in QUERIES).items() if n > 1]
    if dupes:
        sys.exit(f"duplicate queries: {dupes}")

    built = {}
    for label, org in ORGS.items():
        by_title = load(org)

        # The original 28-query set wins on any query it already covers.
        #
        # v2 restates those queries verbatim, and resolving them afresh by title
        # produced *worse* judgments on seven of them: title resolution drifts
        # toward the topical overview document, while the original judgments name
        # the document that actually describes the mechanism. "what stops one
        # tenant from seeing another tenant's documents" is answered by
        # `retrieval engine rs` (where org scoping is enforced), not by
        # CONTROL PLANE DEEP DIVE; "how does the golden judgment set join to
        # retrieval traces" is answered by `data quality go`, where that join is
        # implemented. Reusing them also keeps v2 comparable with the runs
        # already measured against v1.
        prior_path = os.path.join(
            OUT_DIR, f"golden{'' if label == 'baseline' else '-ctx'}.json")
        prior = {}
        if os.path.exists(prior_path):
            prior = {g["query"]: g["relevant_ids"] for g in json.load(open(prior_path, encoding="utf-8"))}

        golden, skipped, reused = [], [], 0
        for query, titles in QUERIES:
            if query in prior:
                golden.append({"query": query, "relevant_ids": prior[query]})
                reused += 1
                continue
            ids = []
            for t in titles:
                found = by_title.get(t, [])
                if len(found) != 1:
                    skipped.append((query, t, len(found)))
                    ids = None
                    break
                ids.append(found[0])
            if ids:
                golden.append({"query": query, "relevant_ids": ids})

        # Make v2 a strict superset of v1: append any v1 query this list did not
        # restate, taking its text and judgment verbatim from the v1 file.
        #
        # Verbatim from the file rather than retyped here for two reasons. It
        # guarantees the superset property holds by construction instead of by my
        # proofreading, and it sidesteps source-encoding damage — a Norwegian
        # query retyped into this source arrived as "k<U+FFFD>yrer" instead of
        # "køyrer", which silently reads as a different query and would have
        # quietly dropped one of the two non-English queries in the set.
        have = {q["query"] for q in golden}
        for query, ids in prior.items():
            if query not in have:
                golden.append({"query": query, "relevant_ids": ids})
        path = os.path.join(OUT_DIR, f"golden-v2{'' if label == 'baseline' else '-ctx'}.json")
        json.dump(golden, open(path, "w", encoding="utf-8"), indent=1)
        built[label] = (path, len(golden), skipped, reused)

    for label, (path, n, skipped, reused) in built.items():
        print(f"{label:<11} {n:>3} queries ({reused} reused from v1, {n - reused} new) -> {os.path.basename(path)}")
        for q, t, c in skipped:
            print(f"    SKIPPED ({c} matches for {t!r}): {q[:52]}")

    counts = {label: n for label, (_, n, _, _) in built.items()}
    if len(set(counts.values())) != 1:
        sys.exit(f"orgs disagree on query count: {counts} — A/B would not be comparable")

    # Agreement check against the original 28-query set. v2 restates those
    # queries verbatim, so any judgment that disagrees means one of the two sets
    # is wrong about the same question — worth knowing before it silently shifts
    # a metric that gets compared across runs.
    old_path = os.path.join(OUT_DIR, "golden.json")
    if os.path.exists(old_path):
        old = {g["query"]: set(g["relevant_ids"]) for g in json.load(open(old_path, encoding="utf-8"))}
        new = {g["query"]: set(g["relevant_ids"])
               for g in json.load(open(built["baseline"][0], encoding="utf-8"))}
        shared = sorted(set(old) & set(new))
        disagree = [q for q in shared if old[q] != new[q]]
        print(f"\noverlap with the original set: {len(shared)}/{len(old)} queries carried over")
        if disagree:
            print(f"  JUDGMENT DISAGREEMENT on {len(disagree)}:")
            for q in disagree:
                print(f"    {q[:58]}\n      old={sorted(old[q])}\n      new={sorted(new[q])}")
        else:
            print("  judgments agree on every carried-over query")

    # Bias check: how much of each query is literally lifted from its document.
    org = ORGS["baseline"]
    content = {}
    for line in psql(f"SELECT document_id, replace(replace(lower(content), chr(10), ' '), '|', ' ') "
                     f"FROM documents WHERE org_id='{org}' AND deleted_at IS NULL;"):
        doc_id, body = line.split("|", 1)
        content[doc_id.strip()] = set(re.findall(r"[a-zøæåöä0-9_-]{3,}", body))

    golden = json.load(open(built["baseline"][0], encoding="utf-8"))
    overlaps = []
    for g in golden:
        w = words(g["query"])
        doc = content.get(g["relevant_ids"][0], set())
        overlaps.append(sum(1 for x in w if x in doc) / len(w) if w else 0.0)
    overlaps.sort()
    mean = sum(overlaps) / len(overlaps)
    print(f"\nverbatim_overlap  mean={mean:.2f}  median={overlaps[len(overlaps)//2]:.2f} "
          f"p10={overlaps[len(overlaps)//10]:.2f} p90={overlaps[int(len(overlaps)*0.9)]:.2f}")
    print("(1.00 would mean every content word was lifted from the judged document)")
    print(f"\nnoise floor: 1 query = {100.0/len(golden):.2f} points of recall@10 "
          f"(was {100.0/28:.2f} at 28 queries)")


if __name__ == "__main__":
    main()
