"""Build the golden sets (the judged queries) of the retrieval-quality evaluation.

Queries are keyed by document TITLE, and titles are resolved to the ids of the organisation being evaluated,
because the two organisations hold the same corpus under different ids, and a re-seeded organisation holds it
under new ones again. Document ids are assigned by documents-api, so a golden file with ids in it is only as
good as the organisation it was written for. CR-T14 therefore changed what is committed and what is derived:

  * the queries and the TITLES of the documents that answer them are the source of truth, here;
  * a title is resolved to exactly one document of the frozen corpus (scripts/eval-corpus-manifest.json), and
    that document to its id by the `corpus_key` that scripts/eval-ingest-corpus.py stores in the document's
    metadata (never by title, which seven READMEs would make ambiguous);
  * the id-keyed files are written afresh, for the organisations as they are today, by

        python3 scripts/eval-build-golden-set.py --out <work-dir>

    (eval-nightly.sh does this at the start of every run, as it re-mines the lexical class, so there is no
    window in which a committed id-keyed file is stale; the golden-v2*.json files committed under scripts/ are
    the ids of the organisations that existed on 2026-08-26 and are no longer authoritative).

Three classes of queries, each its own gated cell, because they respond to changes in different directions and one
blended number hides that (the exact-identifier class is mined, not authored: eval-mine-lexical-queries.py):

  natural_language      golden-v2*.json      87 hand-authored questions (English, with seven in Norwegian)
  norwegian_morphology  golden-v2-no*.json   queries that use another FORM than the text: an inflection, a definite
                                             or plural form, a compound taken apart. The English corpus held two chunks
                                             with Norwegian characters, so this could not be measured at all; the
                                             synthetic Norwegian documents of the corpus exist to make it measurable
  mixed_language        golden-v2-mixed*.json  queries that mix Norwegian and English, as people at work do, against the
                                             Norwegian documents and against the English ones

## The 87 natural-language judgments are the committed ones

The first 87 judgments equal, query for query, the golden-v2.json committed on 2026-08-26 (the title lists below
are chosen so that a test, tests/test_eval_build_golden_set.py, reproduces them from the id-to-title map of that
file). Seven of them were not the title resolution the old builder would have made: the original 28-query set had
judged the document that really answers (the tenant-isolation question names the retrieval engine, not the Control Plane
overview) and the old builder let it win, which is written down here as the title it names. Two queries the old
builder only appended from its v1 file are listed. ONE judgment cannot be reproduced: "how do browser automation
sessions get executed and sandboxed" was judged on a document the committed file does not identify (an id that no title
of the builder names), and is judged on the Model Plane overview, whose browser-agent and sandbox sections answer it.

## Why these are hand-authored

The obvious way to scale a golden set is to have an LLM read each document and
emit questions about it. That is the wrong instrument for what this set is used
for. Generated-from-source questions inherit the source's vocabulary, which
inflates lexical retrieval specifically — so a w_bm25 sweep scored against them
would measure the generator's phrasing habits rather than the retriever. These
are written the way a person actually asks: natural phrasing, no heading text
copied, and deliberately not the document's own wording where a synonym exists.

## Judgment discipline

Every query below is grounded in structure verified from the corpus (its
heading outline) or in a document read directly. A query is only included if its
best answer is unambiguous — the corpus contains near-duplicate families
(DEEP DIVE / STATUS / ROADMAP per plane, nine service audits sharing an outline,
seven files titled README), and a broad question like "which services make up
the data plane" has three defensible answers. Those are omitted rather than
judged arbitrarily: a wrong judgment does not just add noise, it penalises a
retriever for being right. The Norwegian documents cover topics (hygiene routines in food production)
that no English document of the corpus touches, so that their queries have one answer by construction, and the
mixed-language queries that target English documents name facts that, checked by search, only one document holds.

`verbatim_overlap` reports, per query, the fraction of its content words that
appear literally in the judged document. It is a bias check, not a filter —
high overlap on a query about a named service is expected and fine; a whole set
skewing high would mean the queries had drifted toward copying source text.
"""
from __future__ import annotations

import argparse
import json
import os
import re
import shlex
import subprocess
import sys
from pathlib import Path

OUT_DIR = Path(__file__).resolve().parent
MANIFEST = OUT_DIR / "eval-corpus-manifest.json"
SNAPSHOT = OUT_DIR / "eval-corpus"
ORGS = {"baseline": "org-corpus-baseline", "contextual": "org-corpus-contextual"}
POSTGRES = "data-plane-v2-postgres-1"
ORG_RE = re.compile(r"^[A-Za-z0-9_-]{1,128}$")
CORPUS_ID_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{2,63}$")


def docker_argv(*rest):
    """The docker command line; EVAL_DOCKER overrides the executable (the same switch as eval-ingest-corpus.py)."""
    try:
        return [*shlex.split(os.environ.get("EVAL_DOCKER") or "docker"), *rest]
    except ValueError:
        raise BuildError("EVAL_DOCKER is not a command line (an unbalanced quote?); unset it to use docker")


class BuildError(Exception):
    """The golden set cannot be built (exit 1): a title the corpus does not have once, or a document that is not there."""


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
    ("how does the golden judgment set join to retrieval traces", ["data quality go"]),
    ("which retrieval gaps were still open according to the gap analysis", ["gap data"]),
    ("what was the plan for durable retrieval quality improvements", ["retrieval quality and durability plan 2026 08 05"]),
    ("what was the plan for durable retrieval quality improvements in august", ["retrieval quality and durability plan 2026 08 05"]),

    # ---- chunking / embedding / indexing -------------------------------------
    ("how do documents get chunked before embedding", ["index engine rs"]),
    ("which service is responsible for re-driving failed embeddings", ["index engine rs"]),
    ("did the chunking service ever get stuck restarting over and over", ["index engine rs"]),
    ("what did rebuilding the embedding service under compose actually verify", ["embedding engine rs"]),
    ("how do stale embeddings get detected", ["data orchestrator go"]),
    ("what jobs can the data orchestrator run", ["data orchestrator go"]),
    ("does the orchestrator remember what it was doing after a restart", ["data orchestrator go"]),

    # ---- sparse / search index -----------------------------------------------
    ("what does the quickwit adapter rebuild endpoint do", ["quickwit adapter rs"]),
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
    ("what does the control plane own compared to other planes", ["CONTROL PLANE OWNERSHIP"]),
    ("what rules decide which plane gets to own a new capability", ["master ownership matrix"]),
    ("is there one table showing what every plane is responsible for", ["master ownership matrix"]),
    ("what is the overall architecture of the whole platform across planes", ["CODEBASE INFORMATION SYSTEM"]),
    ("what is the overall architecture of the whole platform and its planes", ["CODEBASE INFORMATION SYSTEM"]),
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
    ("what stops one tenant from seeing another tenant's documents", ["retrieval engine rs"]),

    # ---- ADRs -----------------------------------------------------------------
    ("why did we decide to give Spaces their own rows owned by control", ["SPACE AUTHORITY ADR 2026 08 13"]),
    ("could we not have reused an existing workspace identifier for Spaces", ["SPACE AUTHORITY ADR 2026 08 13"]),
    ("does the agent registry answer who is allowed to act, or just who is present", ["CROSS SPACE AGENT REGISTRY ADR 2026 08 19"]),

    # ---- privacy / GDPR -------------------------------------------------------
    ("what happens when an organization requests GDPR erasure of its data", ["embedding engine rs", "gap data"]),
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
    ("hvordan sikrer vi at sletting av kunnskap faktisk fjerner vektorene", ["embedding engine rs"]),
    ("kva slags jobber køyrer datakvalitetstenesta", ["data quality go"]),
    ("kva reglar avgjer kven som eig kva", ["master ownership matrix"]),
    ("hvilke porter er tilgjengelige på verten", ["PORT MAPPING"]),
    ("hvordan setter jeg opp automatisk utrulling fra GitHub", ["WEBHOOK SETUP"]),
    ("hvordan gjenoppretter jeg systemet etter at databasen er ødelagt", ["backup restore"]),
    ("kva gjer vi når ein organisasjon ber om å bli sletta", ["GDPR SUMMARY"]),
]

# Queries that use another FORM than the text: (query, [document titles that answer it]). Each is written so that some
# word is an inflection, a definite or plural form, a tense, or a compound of a word the judged document uses in
# another form (a test checks that), and so that no other document of the corpus answers it.
QUERIES_NORWEGIAN = [
    # ---- rengjøring og desinfeksjon -------------------------------------------
    ("hvor lenge vasker man utstyret med det alkaliske middelet", ["no rengjoring og desinfeksjon"]),
    ("hvor lang kontakttid trenger desinfeksjonsmidlene", ["no rengjoring og desinfeksjon"]),
    ("hvorfor skylles utstyret grundig etter syrevaskene", ["no rengjoring og desinfeksjon"]),
    ("hvor varmt skal forskyllevannet være", ["no rengjoring og desinfeksjon"]),
    # ---- avvikshåndtering --------------------------------------------------------
    ("innen hvor mange timer må avvikene meldes", ["no avvikshandtering"]),
    ("hva må en avviksmelding beskrive", ["no avvikshandtering"]),
    ("hvor lenge oppbevares avviksrapportene", ["no avvikshandtering"]),
    ("hvem verifiserer tiltakene og lukker avviket", ["no avvikshandtering"]),
    # ---- opplæring ----------------------------------------------------------------
    ("når skal en nyansatt ha tatt hygienekurset", ["no opplaering av nyansatte"]),
    ("hvilke hansker brukes når kjemikaliene håndteres", ["no opplaering av nyansatte"]),
    ("hvor lenge har en nyansatt mentor", ["no opplaering av nyansatte"]),
    ("hva brukes i tillegg når det er fare for sprutende kjemikalier", ["no opplaering av nyansatte"]),
    # ---- kalibrering av doseringsutstyr ----------------------------------------
    ("hvor ofte kontrolleres doseringspumpene", ["no kalibrering av doseringsutstyr"]),
    ("hvor stort avvik tillates i konsentrasjonen", ["no kalibrering av doseringsutstyr"]),
    ("hvilke bufferløsninger kalibrerer man pH-måleren med", ["no kalibrering av doseringsutstyr"]),
    ("hva gjør man med en måler som ikke lar seg stille inn", ["no kalibrering av doseringsutstyr"]),
    # ---- vann og avløp -------------------------------------------------------------
    ("hvilke pH-verdier gjelder for utslippene", ["no vann og avlop"]),
    ("hvem varsles når utslippsgrensene overskrides", ["no vann og avlop"]),
    ("hvor ofte analyseres prøvene fra avløpet", ["no vann og avlop"]),
    # ---- mikrobiologisk prøvetaking -------------------------------------------
    ("hvor skal overflateprøvene tas", ["no mikrobiologisk provetaking"]),
    ("hvor fort gir hurtigtesten svar", ["no mikrobiologisk provetaking"]),
    ("hva er grenseverdiene for kolonidannende enheter", ["no mikrobiologisk provetaking"]),
    # ---- kjemikalielager -------------------------------------------------------------
    ("hvor ofte må luften byttes i kjemikalierommet", ["no kjemikalielager"]),
    ("hvor nær døren skal øyedusjen stå", ["no kjemikalielager"]),
    # ---- vedlikehold og service ------------------------------------------------
    ("hvor ofte skiftes pakningene i pumpene", ["no vedlikehold og service"]),
    ("hvor ofte smører vi kjedene", ["no vedlikehold og service"]),
    ("hva kreves før utstyret kan brukes igjen etter servicen", ["no vedlikehold og service"]),
    ("hvor raskt skal utstyret til service når vibrasjonene øker", ["no vedlikehold og service"]),
]

# Queries that mix Norwegian and English: (query, [titles], [the English terms in it]). Against the Norwegian documents
# (where English loanwords and abbreviations are how the trade talks) and against English documents of the corpus
# (where the question is Norwegian and the system's own terms are not translated).
QUERIES_MIXED = [
    ("hvordan kjører vi CIP på lukkede systemer", ["no rengjoring og desinfeksjon"], ["CIP"]),
    ("what is the contact time når vi desinfiserer", ["no rengjoring og desinfeksjon"], ["contact time"]),
    ("hvordan gjør vi root cause analysis etter et avvik", ["no avvikshandtering"], ["root cause analysis"]),
    ("hva er fristen for en corrective action", ["no avvikshandtering"], ["corrective action"]),
    ("hva må nyansatte lære om safety data sheets", ["no opplaering av nyansatte"], ["safety data sheets"]),
    ("hvilket PPE kreves hvis det er fare for sprut", ["no opplaering av nyansatte"], ["PPE"]),
    ("how often må dosing pumps kontrolleres", ["no kalibrering av doseringsutstyr"], ["how often", "dosing pumps"]),
    ("hvilke buffers brukes for å kalibrere pH-meteret", ["no kalibrering av doseringsutstyr"], ["buffers"]),
    ("hvilken pH range må wastewater ha før discharge", ["no vann og avlop"], ["pH range", "wastewater", "discharge"]),
    ("hva er swab-prosedyren for overflateprøver", ["no mikrobiologisk provetaking"], ["swab"]),
    ("når er ATP-resultatet for høyt, hvor mange RLU", ["no mikrobiologisk provetaking"], ["ATP", "RLU"]),
    ("hva krever GHS-merking på beholderne", ["no kjemikalielager"], ["GHS"]),
    ("how many air changes per hour må rommet ha", ["no kjemikalielager"], ["how many air changes per hour"]),
    ("hva er lockout-tagout og hvem fjerner låsen", ["no vedlikehold og service"], ["lockout-tagout"]),
    ("hvilket lubricant er food grade og tillatt", ["no vedlikehold og service"], ["lubricant", "food grade"]),
    ("hvor ofte tas backup av Postgres i production, og hvor lenge beholdes den", ["backup restore"], ["backup", "Postgres", "production"]),
    ("trenger vi backup av Dragonfly og NATS", ["backup restore"], ["backup", "Dragonfly", "NATS"]),
    ("hvor mange points tåler Qdrant single-node før cluster", ["production deploy"], ["points", "Qdrant", "single-node", "cluster"]),
    ("hvor mange replicas kjører retrieval-engine i production", ["production deploy"], ["replicas", "retrieval-engine", "production"]),
    ("hvor mange nodes trenger NATS JetStream cluster i production", ["production deploy"], ["nodes", "NATS JetStream", "cluster", "production"]),
]

# The classes, in the order they are written: (class name as the service's by_class names it, file stem, queries).
CLASSES = [
    ("natural_language", "golden-v2", [(q, t) for q, t in QUERIES]),
    ("norwegian_morphology", "golden-v2-no", [(q, t) for q, t in QUERIES_NORWEGIAN]),
    ("mixed_language", "golden-v2-mixed", [(q, t) for q, t, _ in QUERIES_MIXED]),
]


STOP = set("""a an the is are was were be been being do does did how what which who
whom whose when where why to of in on at for from by with without and or not no
we our us i my it its that this these those can could should would will shall
may might must if then than as into about over under out up down get gets got
have has had there their them they you your me actually really just only also
one two some any all more most much many own does happen happens""".split())


def words(text):
    return [w for w in re.findall(r"[a-zøæåöä0-9_-]{3,}", text.lower()) if w not in STOP]


def title_index(manifest, titles):
    """{title: corpus key} for the titles named; each must be exactly one document of the frozen corpus."""
    by_title = {}
    for doc in manifest["documents"]:
        by_title.setdefault(doc["title"], []).append(doc["key"])
    out = {}
    for title in sorted(set(titles)):
        found = by_title.get(title, [])
        if len(found) != 1:
            raise BuildError(f"the title {title!r} names {len(found)} documents of the corpus, not exactly one "
                             f"({'; '.join(found[:3]) if found else 'none'}): fix the title or the corpus")
        out[title] = found[0]
    return out


class PsqlResolver:
    """{corpus key: document id} of an organisation, read from the key the ingest stored in each document's metadata.
    SELECT only."""

    def __init__(self, corpus_id, run=subprocess.run):
        self.corpus_id, self._run = corpus_id, run

    @staticmethod
    def query_for(org, corpus_id):
        if not ORG_RE.match(org or "") or not CORPUS_ID_RE.match(corpus_id or ""):
            raise BuildError(f"{org!r} / {corpus_id!r} are not an organisation and a corpus id")
        return ("SELECT row_to_json(t) FROM (SELECT metadata->>'corpus_key' AS key, document_id::text AS id FROM documents "
                f"WHERE org_id = '{org}' AND deleted_at IS NULL AND metadata->>'eval_corpus_id' = '{corpus_id}') t")

    def __call__(self, org):
        sql = self.query_for(org, self.corpus_id)
        script = 'psql -U "$POSTGRES_USER" -d "$POSTGRES_DB" -v ON_ERROR_STOP=1 -t -A -c "$1"'
        # utf-8 explicitly: the default Windows codec (cp1252) cannot decode a Norwegian corpus, and fails as a decode
        # error inside a reader thread rather than as a psql error.
        done = self._run(docker_argv("exec", POSTGRES, "sh", "-c", script, "psql", sql),
                         capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=180)
        if done.returncode != 0:
            raise BuildError(f"psql failed: {(done.stderr or '')[:300]}")
        rows = {}
        for line in (done.stdout or "").splitlines():
            if line.startswith("{"):
                row = json.loads(line)
                rows[row["key"]] = row["id"]
        return rows


def build_files(manifest, resolver):
    """{file name: rows} for every class and both organisations, or a BuildError, and nothing half built."""
    all_titles = [t for _, _, queries in CLASSES for _, titles in queries for t in titles]
    keys = title_index(manifest, all_titles)
    files = {}
    for label, org in ORGS.items():
        ids = resolver(org)
        wanted = {title: keys[title] for title in set(all_titles)}
        if not any(key in ids for key in wanted.values()):
            raise BuildError(f"{org}: none of the corpus' documents is there: ingest the corpus first "
                             f"(python3 scripts/eval-ingest-corpus.py plan, then apply)")
        absent = sorted(title for title, key in wanted.items() if key not in ids)
        if absent:
            raise BuildError(f"{org}: {len(absent)} of the {len(wanted)} documents the judgments name are not there: "
                             f"{', '.join(absent[:6])}{' ...' if len(absent) > 6 else ''}. Is the ingest finished "
                             f"(eval-ingest-corpus.py status --wait)?")
        for _, stem, queries in CLASSES:
            name = f"{stem}{'' if label == 'baseline' else '-ctx'}.json"
            files[name] = [{"query": query, "relevant_ids": [ids[keys[title]] for title in titles]} for query, titles in queries]
    return files


def overlap_report(manifest, snapshot, out):
    by_title = {d["title"]: d for d in manifest["documents"]}
    overlaps = []
    for query, titles in QUERIES:
        doc = by_title[titles[0]]
        text = (snapshot / doc["path"]).read_text(encoding="utf-8").lower()
        vocabulary = set(re.findall(r"[a-zøæåöä0-9_-]{3,}", text))
        w = words(query)
        overlaps.append(sum(1 for x in w if x in vocabulary) / len(w) if w else 0.0)
    overlaps.sort()
    mean = sum(overlaps) / len(overlaps)
    print(f"\nverbatim_overlap (natural language)  mean={mean:.2f}  median={overlaps[len(overlaps) // 2]:.2f} "
          f"p10={overlaps[len(overlaps) // 10]:.2f} p90={overlaps[int(len(overlaps) * 0.9)]:.2f}", file=out)
    print("(1.00 would mean every content word was lifted from the judged document)", file=out)
    print(f"noise floor: 1 query = {100.0 / len(QUERIES):.2f} points of recall@10 (was {100.0 / 28:.2f} at 28 queries)", file=out)


def main(argv=None, *, resolver=None, stdout=None):
    out = stdout or sys.stdout
    if hasattr(out, "reconfigure"):
        out.reconfigure(encoding="utf-8", errors="replace")
    parser = argparse.ArgumentParser(description="Build the golden sets for both evaluation organisations.")
    parser.add_argument("--out", default=str(OUT_DIR), help="where the golden files are written (default: scripts/)")
    parser.add_argument("--manifest", default=str(MANIFEST))
    parser.add_argument("--snapshot", default=str(SNAPSHOT))
    args = parser.parse_args(sys.argv[1:] if argv is None else argv)
    manifest = json.loads(Path(args.manifest).read_text(encoding="utf-8"))
    resolve = resolver or PsqlResolver(manifest["corpus_id"])
    try:
        files = build_files(manifest, resolve)
    except BuildError as exc:
        if resolver is not None:
            raise
        print(f"eval-build-golden-set.py: {exc}", file=sys.stderr)
        return 1
    target = Path(args.out)
    target.mkdir(parents=True, exist_ok=True)
    for name, rows in sorted(files.items()):
        (target / name).write_text(json.dumps(rows, indent=1, ensure_ascii=False) + "\n", encoding="utf-8", newline="\n")
    for label in ORGS:
        for cls, stem, queries in CLASSES:
            print(f"{label:<11} {len(queries):>3} {cls} queries -> {stem}{'' if label == 'baseline' else '-ctx'}.json", file=out)
    overlap_report(manifest, Path(args.snapshot), out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
