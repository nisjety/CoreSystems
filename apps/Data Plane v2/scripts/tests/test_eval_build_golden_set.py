"""Tests of scripts/eval-build-golden-set.py: the judgments of the retrieval-quality evaluation (CR-T14).

The golden set keys on DOCUMENT IDS, and documents-api assigns the ids: a re-seeded organisation holds the
same corpus under new ids, so id-keyed files committed once are stale the moment the corpus is ingested
again. The builder therefore keys the judgments on TITLES, resolves each title to the document's manifest key
and each key to an id through the key the ingest carries in the document's metadata, and writes the files
afresh for the organisation it is asked about. These tests prove:

  * the 87 natural-language judgments are the ones committed on 2026-08-26, query for query (one documented
    exception), so the committed baseline stays comparable;
  * every title the builder names resolves to exactly one document of the frozen corpus;
  * the two new classes (Norwegian morphology, mixed language) exist, are the size a gate can use, and are what
    they claim to be: an inflected query against an inflected text, and a query that mixes two languages;
  * both organisations get the same queries, with their own ids, and a missing document stops the build.
"""
from __future__ import annotations

import io
import json
import re
import shutil
import tempfile
import unittest
from pathlib import Path

from _scripts import FIXTURES, SCRIPTS, load

build = load("eval-build-golden-set.py")
ingest = load("eval-ingest-corpus.py")

MANIFEST = json.loads((SCRIPTS / "eval-corpus-manifest.json").read_text(encoding="utf-8"))
SNAPSHOT = SCRIPTS / "eval-corpus"
HISTORY = json.loads((FIXTURES / "golden-v2-2026-08-26.json").read_text(encoding="utf-8"))
BY_TITLE = {d["title"]: d for d in MANIFEST["documents"]}


def text_of(title: str) -> str:
    return (SNAPSHOT / BY_TITLE[title]["path"]).read_text(encoding="utf-8")


def words(text: str) -> list[str]:
    return re.findall(r"[a-zæøåäö0-9]+", text.lower())


class FakeResolver:
    """Stands in for the database: the id of each corpus key, per organisation. Ids are made up per run, so that
    a test can tell a file built for one organisation from one built for the other."""

    def __init__(self, prefix: str, drop: set[str] | None = None):
        self.prefix, self.drop = prefix, drop or set()

    def __call__(self, org: str) -> dict:
        return {d["key"]: f"{self.prefix}-{org}-{i:04d}" for i, d in enumerate(MANIFEST["documents"]) if d["key"] not in self.drop}


class TheNaturalLanguageJudgmentsAreTheCommittedOnes(unittest.TestCase):
    def test_every_query_has_the_judgment_it_had_on_2026_08_26(self):
        old_ids = HISTORY["baseline_ids_by_title"]
        unidentified = HISTORY["unidentified_id"]
        built = {query: titles for query, titles in build.QUERIES}
        exceptions = []
        for judged in HISTORY["judgments"]:
            query = judged["query"]
            self.assertIn(query, built, f"the query was dropped: {query!r}")
            expected = judged["relevant_ids"]
            if any(i.startswith(unidentified) for i in expected):
                exceptions.append(query)
                continue
            got = [old_ids[title] for title in built[query]]
            with self.subTest(query=query):
                self.assertEqual(got, expected)
        self.assertEqual(exceptions, ["how do browser automation sessions get executed and sandboxed"],
                         "the one judgment that names a document the file does not identify")

    def test_the_set_has_no_query_the_committed_one_lacked(self):
        committed = {j["query"] for j in HISTORY["judgments"]}
        self.assertEqual({query for query, _ in build.QUERIES}, committed)
        self.assertEqual(len(build.QUERIES), 87)


class EveryTitleIsOneDocumentOfTheFrozenCorpus(unittest.TestCase):
    def test_every_title_named_resolves_to_exactly_one_document(self):
        titles = [t for _, ts in build.QUERIES for t in ts] + [t for _, ts in build.QUERIES_NORWEGIAN for t in ts] \
            + [t for _, ts, _ in build.QUERIES_MIXED for t in ts]
        counts: dict[str, int] = {}
        for doc in MANIFEST["documents"]:
            counts[doc["title"]] = counts.get(doc["title"], 0) + 1
        for title in sorted(set(titles)):
            with self.subTest(title=title):
                self.assertEqual(counts.get(title, 0), 1)

    def test_the_title_index_refuses_an_ambiguous_title(self):
        self.assertGreater(sum(1 for d in MANIFEST["documents"] if d["title"] == "README"), 1, "the corpus holds several READMEs")
        with self.assertRaises(build.BuildError):
            build.title_index(MANIFEST, ["README"])
        with self.assertRaises(build.BuildError):
            build.title_index(MANIFEST, ["a title no document has"])


class TheClasses(unittest.TestCase):
    def test_the_sizes_a_gate_can_use(self):
        self.assertEqual(len(build.QUERIES), 87)
        self.assertGreaterEqual(len(build.QUERIES_NORWEGIAN), 24)
        self.assertGreaterEqual(len(build.QUERIES_MIXED), 20)

    def test_no_query_is_in_two_classes_or_twice(self):
        everything = [q for q, _ in build.QUERIES] + [q for q, _ in build.QUERIES_NORWEGIAN] + [q for q, _, _ in build.QUERIES_MIXED]
        self.assertEqual(len(everything), len(set(everything)))

    def test_the_norwegian_morphology_queries_are_judged_on_norwegian_documents_only(self):
        for query, titles in build.QUERIES_NORWEGIAN:
            for title in titles:
                self.assertEqual(BY_TITLE[title]["origin"], "synthetic", f"{query!r} is judged on {title!r}")

    def test_each_norwegian_morphology_query_uses_another_form_than_its_document(self):
        """Not a copy of the text, and not unrelated to it: some word of the query is absent from the judged
        document as written but shares a stem (the first five letters) with a word that is there: an inflection,
        a definite or plural form or a compound that the document used differently."""
        for query, titles in build.QUERIES_NORWEGIAN:
            doc_words = set(words(" ".join(text_of(t) for t in titles)))
            variants = [w for w in words(query)
                        if len(w) >= 6 and w not in doc_words and any(d[:5] == w[:5] for d in doc_words)]
            with self.subTest(query=query):
                self.assertTrue(variants, f"no word of the query is a differently inflected form of a word of {titles}")

    def test_each_mixed_language_query_really_mixes_two_languages(self):
        norwegian = {"hva", "hvordan", "hvor", "hvilken", "hvilke", "når", "og", "for", "med", "til", "er", "på", "av", "etter",
                     "før", "ved", "om", "en", "et", "skal", "må", "vi", "jeg", "kjører", "tas", "brukes", "finner", "lenge",
                     "ofte", "mange", "trenger", "gjelder", "kreves", "gjør", "bruker", "kan", "hvis", "det", "den", "ikke"}
        for query, _, english in build.QUERIES_MIXED:
            tokens = set(words(query))
            with self.subTest(query=query):
                self.assertTrue(english, "the English terms of the query must be declared")
                for term in english:
                    self.assertIn(term.lower(), query.lower(), f"{term!r} is declared English but is not in the query")
                self.assertTrue(tokens & norwegian, "no Norwegian function word: the query is not mixed")

    def test_the_judged_documents_of_the_mixed_class_include_both_kinds(self):
        origins = {BY_TITLE[t]["origin"] for _, titles, _ in build.QUERIES_MIXED for t in titles}
        self.assertEqual(origins, {"repo", "synthetic"})


class BuildingTheFiles(unittest.TestCase):
    def out_dir(self) -> Path:
        path = Path(tempfile.mkdtemp(prefix="evalgolden-"))
        self.addCleanup(shutil.rmtree, path, True)
        return path

    def run_build(self, out: Path, resolver):
        return build.main(["--out", str(out), "--manifest", str(SCRIPTS / "eval-corpus-manifest.json"),
                           "--snapshot", str(SNAPSHOT)], resolver=resolver, stdout=io.StringIO())

    def test_it_writes_a_file_per_class_and_organisation_in_the_shape_the_runner_reads(self):
        out = self.out_dir()
        self.assertEqual(self.run_build(out, FakeResolver("a")), 0)
        names = sorted(p.name for p in out.iterdir())
        self.assertEqual(names, ["golden-v2-ctx.json", "golden-v2-mixed-ctx.json", "golden-v2-mixed.json", "golden-v2-no-ctx.json",
                                 "golden-v2-no.json", "golden-v2.json"])
        for name in names:
            rows = json.loads((out / name).read_text(encoding="utf-8"))
            self.assertTrue(rows)
            for row in rows:
                self.assertEqual(set(row), {"query", "relevant_ids"}, "eval-run-retrieval.py reads exactly these keys")
                self.assertTrue(row["relevant_ids"])

    def test_the_two_organisations_get_the_same_queries_and_their_own_ids(self):
        out = self.out_dir()
        self.run_build(out, FakeResolver("a"))
        for stem in ("golden-v2", "golden-v2-no", "golden-v2-mixed"):
            baseline = json.loads((out / f"{stem}.json").read_text(encoding="utf-8"))
            contextual = json.loads((out / f"{stem}-ctx.json").read_text(encoding="utf-8"))
            self.assertEqual([r["query"] for r in baseline], [r["query"] for r in contextual])
            for a, b in zip(baseline, contextual):
                self.assertEqual(len(a["relevant_ids"]), len(b["relevant_ids"]))
                self.assertTrue(all("org-corpus-baseline" in i for i in a["relevant_ids"]))
                self.assertTrue(all("org-corpus-contextual" in i for i in b["relevant_ids"]))

    def test_the_files_follow_a_reseed_without_a_change_to_the_queries(self):
        first, second = self.out_dir(), self.out_dir()
        self.run_build(first, FakeResolver("a"))
        self.run_build(second, FakeResolver("b"))
        a = json.loads((first / "golden-v2.json").read_text(encoding="utf-8"))
        b = json.loads((second / "golden-v2.json").read_text(encoding="utf-8"))
        self.assertEqual([r["query"] for r in a], [r["query"] for r in b])
        self.assertNotEqual([r["relevant_ids"] for r in a], [r["relevant_ids"] for r in b])

    def test_a_document_that_is_not_in_the_organisation_stops_the_build_and_writes_nothing(self):
        out = self.out_dir()
        missing = BY_TITLE["backup restore"]["key"]
        with self.assertRaises(build.BuildError) as raised:
            self.run_build(out, FakeResolver("a", drop={missing}))
        self.assertIn("backup restore", str(raised.exception))
        self.assertEqual(list(out.iterdir()), [], "a half-built golden set must not be left behind")

    def test_an_organisation_with_none_of_the_corpus_is_refused_with_the_remedy(self):
        out = self.out_dir()
        with self.assertRaises(build.BuildError) as raised:
            self.run_build(out, lambda org: {})
        self.assertIn("eval-ingest-corpus.py", str(raised.exception))

    def test_the_resolution_query_is_by_corpus_key_and_read_only(self):
        sql = build.PsqlResolver.query_for("org-corpus-baseline", "dpv2-eval-corpus-v1")
        self.assertIn("metadata->>'corpus_key'", sql)
        self.assertIn("org_id = 'org-corpus-baseline'", sql)
        self.assertIn("metadata->>'eval_corpus_id' = 'dpv2-eval-corpus-v1'", sql)
        self.assertIsNone(re.search(r"\b(INSERT|UPDATE|DELETE|DROP|ALTER|TRUNCATE)\b", sql.upper()))
        with self.assertRaises(build.BuildError):
            build.PsqlResolver.query_for("org'; --", "x")


if __name__ == "__main__":
    unittest.main()
