# eval-corpus: the frozen corpus of the retrieval-quality evaluation

Do not edit these files. They are pinned, file by file, by sha256 in `../eval-corpus-manifest.json`, and
`python3 scripts/eval-ingest-corpus.py verify` (also run by the tests) fails on any change, any missing file and
any file the manifest does not list.

* `repo/` is a snapshot of this repository's own markdown documents, cut from git history (the commit named in
  the manifest's `freeze.source_ref`, plus the documents listed in `freeze.additions`), byte for byte. It is the
  corpus the judgments of the golden set were written against; the working tree of today is not, because it
  changes with every commit. Refreeze deliberately, with `eval-ingest-corpus.py freeze`, and say why.
* `synthetic/` holds a few Norwegian documents written for this corpus. The English corpus has two chunks with
  Norwegian characters, so Norwegian morphology (an inflected query against an inflected text) and mixed-language
  queries could not be measured on it. The documents say they are synthetic; the topics (hygiene routines in food
  production) are chosen so that no other document of the corpus answers the same questions.
* `.gitattributes` keeps git from converting line endings: a hash that changes with a checkout is no pin.

How the corpus is ingested, what it costs and why that is dark by default: the header of
`../eval-ingest-corpus.py` and the evaluation section of `docs/contextual-retrieval-runbook.md`.
